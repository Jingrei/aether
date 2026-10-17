//! Aether spatial audio engine.
//!
//! Signal flow (spatial modes):
//!
//! ```text
//! input (2.0 … 7.1.4) ─► upmix / channel map ─► 7.1.4 bed ─► dialog ─► BRIR convolution ─┬─► XTC (speakers) ─► detail ─► bass ─► EQ ─► tuning ─► leveler ─► limiter
//!                            test object ─┘                   └─► FDN late reverb ─────┘
//! ```

pub mod biquad;
pub mod convolver;
pub mod enhance;
pub mod hrtf;
pub mod layout;
pub mod params;
pub mod presets;
pub mod processor;
pub mod reverb;
pub mod room;
pub mod runtime;
pub mod tuning;
pub mod upmix;

pub use params::*;
pub use processor::{FilterCache, Processor, BLOCK};
pub use runtime::{Meters, ProcessorSlot, Runtime};

/// Renders a planar multichannel buffer offline. Returns stereo (left, right).
pub fn render_offline(settings: &EngineSettings, fs: f32, input: &[Vec<f32>]) -> [Vec<f32>; 2] {
    let mut cache = FilterCache::default();
    let mut p = Processor::new(settings, fs, input.len().max(1), &mut cache);
    let frames = input.first().map_or(0, Vec::len);
    let mut out = [vec![0.0; frames], vec![0.0; frames]];
    let mut blk: Vec<Vec<f32>> = vec![vec![0.0; BLOCK]; input.len()];
    let (mut l, mut r) = (vec![0.0; BLOCK], vec![0.0; BLOCK]);
    let mut pos = 0;
    while pos < frames {
        let n = BLOCK.min(frames - pos);
        for (c, ch) in input.iter().enumerate() {
            blk[c][..n].copy_from_slice(&ch[pos..pos + n]);
            blk[c][n..].iter_mut().for_each(|v| *v = 0.0);
        }
        let refs: Vec<&[f32]> = blk.iter().map(Vec::as_slice).collect();
        p.process_block(&refs, &mut l, &mut r);
        out[0][pos..pos + n].copy_from_slice(&l[..n]);
        out[1][pos..pos + n].copy_from_slice(&r[..n]);
        pos += n;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize, seed: u32) -> Vec<f32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                (s as f32 / u32::MAX as f32 - 0.5) * 0.3
            })
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn every_profile_and_mode_renders_sane_audio() {
        let input = vec![noise(48000, 1), noise(48000, 2)];
        let in_rms = rms(&input[0]);
        for profile in presets::builtin_profiles() {
            for mode in [OutputMode::Headphones, OutputMode::Speakers, OutputMode::Stereo] {
                for quality in [Quality::Battery, Quality::Best] {
                    let s = EngineSettings { mode, quality, profile: profile.clone(), ..Default::default() };
                    let [l, r] = render_offline(&s, 48000.0, &input);
                    assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < 1.0));
                    let ratio = rms(&l[9600..]) / in_rms;
                    assert!((0.2..5.0).contains(&ratio), "{} {:?} {:?}: level ratio {ratio}", profile.id, mode, quality);
                }
            }
        }
    }

    #[test]
    fn disabled_is_bit_exact_passthrough() {
        let input = vec![noise(4096, 3), noise(4096, 4)];
        let s = EngineSettings { enabled: false, ..Default::default() };
        let [l, r] = render_offline(&s, 48000.0, &input);
        assert_eq!(l, input[0]);
        assert_eq!(r, input[1]);
    }

    #[test]
    fn left_surround_channel_is_heard_on_the_left() {
        let n = 24000;
        let mut input = vec![vec![0.0; n]; 8];
        input[6] = noise(n, 5); // 7.1 side-left
        let s = EngineSettings { quality: Quality::Battery, ..Default::default() };
        let [l, r] = render_offline(&s, 48000.0, &input);
        let ild = 20.0 * (rms(&l[4800..]) / rms(&r[4800..])).log10();
        assert!(ild > 3.0, "ILD {ild}");
    }

    #[test]
    fn mono_center_content_is_symmetric() {
        let x = noise(24000, 9);
        let s = EngineSettings::default();
        let [l, r] = render_offline(&s, 48000.0, &[x.clone(), x]);
        let ild = 20.0 * (rms(&l[4800..]) / rms(&r[4800..])).log10();
        assert!(ild.abs() < 1.0, "ILD {ild}");
    }
}
