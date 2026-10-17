//! The complete per-settings signal chain. A `Processor` is built off the audio thread (it allocates)
//! and then handed to the real-time runtime, which only calls `process_block`.

use crate::biquad::{Biquad, Coeffs};
use crate::convolver::{Convolver, FilterBank};
use crate::enhance::{BassEnhancer, Leveler, Limiter, StereoEq, Xtc};
use crate::hrtf::HrtfModel;
use crate::layout::{self, *};
use crate::params::{EngineSettings, OutputMode, Quality, SpatialParams, EQ_BANDS};
use crate::reverb::Fdn;
use crate::room::{synth_brir, RoomSpec};
use crate::upmix::Upmixer;
use std::sync::Arc;

/// Internal processing block size (frames). Also the engine's added latency.
pub const BLOCK: usize = 128;
const ONSET: usize = 32;

struct QualitySpec {
    len: usize,
    order: i32,
    heights: bool,
    reverb: bool,
}

fn quality_spec(q: Quality) -> QualitySpec {
    match q {
        Quality::Battery => QualitySpec { len: 512, order: 0, heights: false, reverb: false },
        Quality::Balanced => QualitySpec { len: 2048, order: 1, heights: true, reverb: true },
        Quality::Best => QualitySpec { len: 4096, order: 2, heights: true, reverb: true },
    }
}

/// Precomputed binaural filters for one spatial configuration.
pub struct SpatialFilters {
    bank: Arc<FilterBank>,
    channels: Vec<usize>,
    norm: f32,
}

#[derive(Clone, PartialEq)]
struct SpatialKey {
    fs: u32,
    quality: Quality,
    params: [f32; 6],
}

/// Caches the most recent spatial filter set so tone-only changes rebuild instantly.
#[derive(Default)]
pub struct FilterCache {
    entry: Option<(SpatialKey, Arc<SpatialFilters>)>,
}

impl FilterCache {
    fn get(&mut self, fs: f32, quality: Quality, sp: &SpatialParams) -> Arc<SpatialFilters> {
        let key = SpatialKey {
            fs: fs as u32,
            quality,
            params: [sp.front_angle, sp.surround_angle, sp.room_size, sp.reflections, sp.damping, sp.distance],
        };
        if let Some((k, v)) = &self.entry {
            if *k == key {
                return v.clone();
            }
        }
        let v = Arc::new(build_spatial_filters(fs, quality, sp));
        self.entry = Some((key, v.clone()));
        v
    }
}

fn build_spatial_filters(fs: f32, quality: Quality, sp: &SpatialParams) -> SpatialFilters {
    let q = quality_spec(quality);
    let nfft = (q.len * 2).max(1024);
    let model = HrtfModel::new(fs as f64, nfft);
    let room = RoomSpec {
        size: sp.room_size.clamp(0.0, 1.0) as f64,
        reflections: sp.reflections.clamp(0.0, 1.0) as f64,
        damping: sp.damping.clamp(0.0, 1.0) as f64,
        distance: sp.distance.clamp(0.8, 5.0) as f64,
        order: q.order,
    };
    let dirs = layout::bed_directions(sp);
    let channels: Vec<usize> = (0..BED).filter(|&c| c != LFE && (q.heights || c < LTF)).collect();
    let irs: Vec<[Vec<f32>; 2]> = std::thread::scope(|s| {
        let handles: Vec<_> = channels
            .iter()
            .map(|&c| {
                let (az, el) = dirs[c];
                let (model, room) = (&model, &room);
                s.spawn(move || synth_brir(model, az as f64, el as f64, room, q.len, ONSET))
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("brir thread")).collect()
    });

    // Loudness normalization: uncorrelated front L/R input should come out at unit power per ear.
    let e = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>();
    let li = channels.iter().position(|&c| c == L).unwrap();
    let ri = channels.iter().position(|&c| c == R).unwrap();
    let ear_power = e(&irs[li][0]) + e(&irs[ri][0]);
    let norm = 1.0 / ear_power.max(1e-6).sqrt();

    SpatialFilters { bank: Arc::new(FilterBank::new(BLOCK, &irs)), channels, norm }
}

/// Pink-noise bursts used by the spatial sound editor.
struct TestGen {
    rng: u32,
    pink: [f32; 7],
    pos: usize,
    period: usize,
    on: usize,
    ramp: usize,
    gains: [f32; BED],
    stereo: (f32, f32),
}

impl TestGen {
    fn new(fs: f32, sp: &SpatialParams, az: f32, el: f32) -> Self {
        let mut gains = [0.0; BED];
        layout::pan_object(sp, az, el, &mut gains);
        let p = (az.to_radians().sin() * 0.5 + 0.5) * std::f32::consts::FRAC_PI_2;
        Self {
            rng: 0x1234_5678,
            pink: [0.0; 7],
            pos: 0,
            period: (0.5 * fs) as usize,
            on: (0.32 * fs) as usize,
            ramp: (0.008 * fs) as usize,
            gains,
            stereo: (p.sin(), p.cos()),
        }
    }

    #[inline]
    fn next(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        let w = (self.rng as f32 / u32::MAX as f32) * 2.0 - 1.0;
        let b = &mut self.pink;
        b[0] = 0.99886 * b[0] + w * 0.0555179;
        b[1] = 0.99332 * b[1] + w * 0.0750759;
        b[2] = 0.96900 * b[2] + w * 0.1538520;
        b[3] = 0.86650 * b[3] + w * 0.3104856;
        b[4] = 0.55000 * b[4] + w * 0.5329522;
        b[5] = -0.7616 * b[5] - w * 0.0168980;
        let pink = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362;
        b[6] = w * 0.115926;

        let t = self.pos;
        self.pos = (self.pos + 1) % self.period;
        let env = if t >= self.on {
            0.0
        } else if t < self.ramp {
            t as f32 / self.ramp as f32
        } else if t > self.on - self.ramp {
            (self.on - t) as f32 / self.ramp as f32
        } else {
            1.0
        };
        pink * 0.05 * env
    }
}

struct SpatialStage {
    conv: Convolver,
    channels: Vec<usize>,
    norm: f32,
    upmix: Upmixer,
    params: SpatialParams,
    fold_heights: bool,
    dialog: Vec<Biquad>,
    center_gain: f32,
    fdn: Option<Fdn>,
    reverb_gain: f32,
    lfe_lp: Biquad,
    xtc: Option<Xtc>,
}

struct PostChain {
    detail: StereoEq,
    bass: BassEnhancer,
    eq: StereoEq,
    tuning: StereoEq,
    leveler: Leveler,
    gain: f32,
    limiter: Limiter,
}

impl PostChain {
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        self.detail.process(l, r);
        self.bass.process(l, r);
        self.eq.process(l, r);
        self.tuning.process(l, r);
        self.leveler.process(l, r);
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            *a *= self.gain;
            *b *= self.gain;
        }
        self.limiter.process(l, r);
    }
}

pub struct Processor {
    pub fs: f32,
    pub in_ch: usize,
    map: Vec<Option<usize>>,
    enabled: bool,
    spatial: Option<SpatialStage>,
    stereo_dialog: (Vec<Biquad>, f32),
    test: Option<TestGen>,
    post: PostChain,
    bed: Vec<Vec<f32>>,
    tmp: Vec<f32>,
}

fn db(x: f32) -> f32 {
    10f32.powf(x / 20.0)
}

fn dialog_filters(fs: f64, amount: f32) -> Vec<Biquad> {
    let a = amount as f64;
    vec![
        Biquad::new(Coeffs::highpass(fs, 80.0 + 60.0 * a, 0.707)),
        Biquad::new(Coeffs::peaking(fs, 250.0, 1.0, -2.5 * a)),
        Biquad::new(Coeffs::peaking(fs, 2800.0, 0.9, 6.0 * a)),
    ]
}

impl Processor {
    pub fn new(settings: &EngineSettings, fs: f32, in_ch: usize, cache: &mut FilterCache) -> Self {
        let fsd = fs as f64;
        let sp = &settings.profile.spatial;
        let tone = &settings.profile.tone;
        let q = quality_spec(settings.quality);
        let spatial_mode = settings.enabled && settings.mode != OutputMode::Stereo;

        let spatial = spatial_mode.then(|| {
            let filters = cache.get(fs, settings.quality, sp);
            SpatialStage {
                conv: Convolver::new(filters.bank.clone()),
                channels: filters.channels.clone(),
                norm: filters.norm,
                upmix: Upmixer::new(fs),
                params: sp.clone(),
                fold_heights: !q.heights,
                dialog: dialog_filters(fsd, tone.dialog),
                center_gain: db(tone.dialog * 3.0),
                fdn: (q.reverb && sp.reverb > 0.0).then(|| Fdn::new(fs, sp.room_size, sp.decay.clamp(0.1, 3.0), sp.damping)),
                reverb_gain: sp.reverb * 0.7,
                lfe_lp: Biquad::new(Coeffs::lowpass(fsd, 120.0, 0.707)),
                xtc: (settings.mode == OutputMode::Speakers).then(|| Xtc::new(fs, 10.0, 0.85)),
            }
        });

        let harmonics = if settings.mode == OutputMode::Speakers { 1.0 } else { 0.35 };
        let mut eq = Vec::new();
        for (i, &g) in tone.eq.iter().enumerate() {
            if g.abs() > 0.01 {
                eq.push(Coeffs::peaking(fsd, EQ_BANDS[i] as f64, 1.41, g as f64));
            }
        }
        let use_tuning = settings.mode != OutputMode::Speakers;
        let tuning: Vec<Coeffs> = if use_tuning { settings.tuning.filters.iter().map(|f| Coeffs::from_filter(fsd, f)).collect() } else { vec![] };
        let tuning_pre = if use_tuning { settings.tuning.preamp } else { 0.0 };
        let detail = if tone.detail > 0.01 {
            StereoEq::new([
                Coeffs::peaking(fsd, 3200.0, 1.2, 2.5 * tone.detail as f64),
                Coeffs::high_shelf(fsd, 5500.0, 0.7, 4.0 * tone.detail as f64),
            ])
        } else {
            StereoEq::default()
        };

        let post = PostChain {
            detail,
            bass: BassEnhancer::new(fs, tone.bass, harmonics),
            eq: StereoEq::new(eq),
            tuning: StereoEq::new(tuning),
            leveler: Leveler::new(fs, tone.leveler),
            gain: db(tone.preamp + tuning_pre + settings.output_gain),
            limiter: Limiter::new(fs, -1.0),
        };

        let test = (settings.enabled && settings.test.enabled).then(|| TestGen::new(fs, sp, settings.test.azimuth, settings.test.elevation));

        Self {
            fs,
            in_ch,
            map: layout::input_map(in_ch),
            enabled: settings.enabled,
            spatial,
            stereo_dialog: (dialog_filters(fsd, tone.dialog), db(tone.dialog * 3.0)),
            test,
            post,
            bed: vec![vec![0.0; BLOCK]; BED],
            tmp: vec![0.0; BLOCK],
        }
    }

    /// Processes one block of `BLOCK` frames. `input` holds `in_ch` planar channels.
    pub fn process_block(&mut self, input: &[&[f32]], out_l: &mut [f32], out_r: &mut [f32]) {
        let Self { map, enabled, spatial, stereo_dialog, test, post, bed, tmp, .. } = self;
        let n = out_l.len();

        if !*enabled {
            downmix_into(map, input, out_l, out_r);
            return;
        }

        match spatial {
            Some(st) => {
                bed.iter_mut().for_each(|c| c.iter_mut().for_each(|v| *v = 0.0));
                if input.len() <= 2 {
                    let l = input.first().copied().unwrap_or(&[]);
                    let r = input.get(1).copied().unwrap_or(l);
                    if l.len() == n {
                        st.upmix.process(l, r, st.params.upmix, st.params.center, st.params.height, bed);
                    }
                } else {
                    for (i, ch) in input.iter().enumerate() {
                        if let Some(Some(b)) = map.get(i) {
                            bed[*b].iter_mut().zip(ch.iter()).for_each(|(d, s)| *d += s);
                        }
                    }
                }
                if let Some(t) = test {
                    for i in 0..n {
                        let s = t.next();
                        for (c, g) in t.gains.iter().enumerate() {
                            if *g != 0.0 {
                                bed[c][i] += s * g;
                            }
                        }
                    }
                }
                for f in &mut st.dialog {
                    f.process(&mut bed[C]);
                }
                bed[C].iter_mut().for_each(|v| *v *= st.center_gain);
                if st.fold_heights {
                    for (h, e) in [(LTF, L), (RTF, R), (LTR, LB), (RTR, RB)] {
                        for i in 0..n {
                            let v = bed[h][i];
                            bed[e][i] += v * 0.8;
                        }
                    }
                }

                let mut refs: [&[f32]; BED] = [&[]; BED];
                for (j, &c) in st.channels.iter().enumerate() {
                    refs[j] = &bed[c];
                }
                st.conv.process(&refs[..st.channels.len()], out_l, out_r);
                for i in 0..n {
                    out_l[i] *= st.norm;
                    out_r[i] *= st.norm;
                }

                if let Some(fdn) = &mut st.fdn {
                    let g = st.reverb_gain;
                    for i in 0..n {
                        let left = bed[L][i] + bed[LS][i] + bed[LB][i] + 0.7 * bed[C][i];
                        let right = bed[R][i] + bed[RS][i] + bed[RB][i] + 0.7 * bed[C][i];
                        let (a, b) = fdn.tick(left * 0.5, right * 0.5);
                        out_l[i] += a * g;
                        out_r[i] += b * g;
                    }
                }

                for i in 0..n {
                    let lfe = st.lfe_lp.tick(bed[LFE][i]) * 0.5;
                    out_l[i] += lfe;
                    out_r[i] += lfe;
                }
                if let Some(x) = &mut st.xtc {
                    x.process(out_l, out_r);
                }
            }
            None => {
                downmix_into(map, input, out_l, out_r);
                if let Some(t) = test {
                    for i in 0..n {
                        let s = t.next();
                        out_l[i] += s * t.stereo.0;
                        out_r[i] += s * t.stereo.1;
                    }
                }
                let (filters, gain) = stereo_dialog;
                if !filters.is_empty() && *gain > 1.0001 {
                    for i in 0..n {
                        tmp[i] = 0.5 * (out_l[i] + out_r[i]);
                    }
                    for f in filters.iter_mut() {
                        f.process(tmp);
                    }
                    for i in 0..n {
                        let mid = 0.5 * (out_l[i] + out_r[i]);
                        let side = 0.5 * (out_l[i] - out_r[i]);
                        let m = mid + (tmp[i] * *gain - mid) * 0.5;
                        out_l[i] = m + side;
                        out_r[i] = m - side;
                    }
                }
            }
        }

        post.process(out_l, out_r);
    }
}

fn downmix_into(map: &[Option<usize>], input: &[&[f32]], out_l: &mut [f32], out_r: &mut [f32]) {
    out_l.iter_mut().for_each(|v| *v = 0.0);
    out_r.iter_mut().for_each(|v| *v = 0.0);
    if input.len() == 1 {
        out_l.copy_from_slice(input[0]);
        out_r.copy_from_slice(input[0]);
        return;
    }
    for (i, ch) in input.iter().enumerate() {
        let Some(Some(b)) = map.get(i) else { continue };
        let (gl, gr) = layout::downmix(*b);
        for (j, &s) in ch.iter().enumerate() {
            out_l[j] += s * gl;
            out_r[j] += s * gr;
        }
    }
}
