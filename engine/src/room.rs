//! Binaural room impulse response synthesis: direct HRTF path plus image-source early reflections
//! from a shoebox room, all summed in the frequency domain.

use crate::hrtf::{direction, Ear, HrtfModel, SPEED_OF_SOUND};
use realfft::num_complex::Complex64;
use realfft::RealFftPlanner;

#[derive(Clone, Copy, Debug)]
pub struct RoomSpec {
    /// 0..1, mapped to a 4x3.5 m booth up to a 14x11.5 m hall.
    pub size: f64,
    /// 0..1 early reflection level.
    pub reflections: f64,
    /// 0..1 high-frequency absorption.
    pub damping: f64,
    /// Speaker distance in metres.
    pub distance: f64,
    /// Maximum image-source order (0 = anechoic).
    pub order: i32,
}

impl RoomSpec {
    pub fn dims(&self) -> [f64; 3] {
        [4.0 + 10.0 * self.size, 3.5 + 8.0 * self.size, 2.6 + 2.4 * self.size]
    }

    /// Listener position inside the room (slightly off-center to avoid coincident reflections).
    pub fn listener(&self) -> [f64; 3] {
        let d = self.dims();
        [0.55 * d[0], 0.47 * d[1], 1.2]
    }
}

fn image_coord(n: i32, len: f64, s: f64) -> f64 {
    if n.rem_euclid(2) == 0 {
        n as f64 * len + s
    } else {
        (n + 1) as f64 * len - s
    }
}

/// Synthesizes a left/right BRIR of `len` samples for a speaker at (`az`, `el`) degrees.
/// The direct sound arrives `onset` samples into the response.
pub fn synth_brir(model: &HrtfModel, az: f64, el: f64, room: &RoomSpec, len: usize, onset: usize) -> [Vec<f32>; 2] {
    let fs = model.fs;
    let dims = room.dims();
    let l = room.listener();
    let dir = direction(az, el);

    // Keep the speaker inside the room with a 0.3 m margin.
    let mut max_t = f64::INFINITY;
    for i in 0..3 {
        if dir[i].abs() > 1e-9 {
            let wall = if dir[i] > 0.0 { dims[i] } else { 0.0 };
            max_t = max_t.min((wall - l[i]) / dir[i]);
        }
    }
    let dist = room.distance.min(max_t - 0.3).max(0.5);
    let s = [l[0] + dir[0] * dist, l[1] + dir[1] * dist, l[2] + dir[2] * dist];

    let reflectance: f64 = 0.82;
    let er_gain = room.reflections * 1.1;
    let wall_fc = 14000.0 * (1.0 - 0.8 * room.damping);
    let onset_s = onset as f64 / fs;
    let max_delay = (len as f64 * 0.95) / fs;

    let bins = model.bins();
    let mut spec = [vec![Complex64::new(0.0, 0.0); bins], vec![Complex64::new(0.0, 0.0); bins]];
    let o = room.order;
    for nx in -o..=o {
        for ny in -o..=o {
            for nz in -o..=o {
                let order = nx.abs() + ny.abs() + nz.abs();
                if order > o {
                    continue;
                }
                let p = [image_coord(nx, dims[0], s[0]), image_coord(ny, dims[1], s[1]), image_coord(nz, dims[2], s[2])];
                let v = [p[0] - l[0], p[1] - l[1], p[2] - l[2]];
                let d = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                let delay = onset_s + (d - dist) / SPEED_OF_SOUND;
                if delay > max_delay {
                    continue;
                }
                let u = [v[0] / d, v[1] / d, v[2] / d];
                let mut gain = dist / d;
                if order > 0 {
                    gain *= reflectance.powi(order) * er_gain;
                }
                for (e, ear) in [Ear::Left, Ear::Right].into_iter().enumerate() {
                    model.accumulate(u, ear, gain, delay, order as u32, wall_fc, &mut spec[e]);
                }
            }
        }
    }

    let mut planner = RealFftPlanner::<f64>::new();
    let ifft = planner.plan_fft_inverse(model.n);
    let fade = (len / 10).max(16);
    spec.map(|mut sp| {
        sp[0].im = 0.0;
        if let Some(last) = sp.last_mut() {
            last.im = 0.0;
        }
        let mut out = vec![0.0f64; model.n];
        let _ = ifft.process(&mut sp, &mut out);
        let scale = 1.0 / model.n as f64;
        (0..len)
            .map(|i| {
                let w = if i + fade >= len { 0.5 * (1.0 + (std::f64::consts::PI * (i + fade - len) as f64 / fade as f64).cos()) } else { 1.0 };
                (out[i] * scale * w) as f32
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn energy(x: &[f32]) -> f64 {
        x.iter().map(|&v| (v as f64) * (v as f64)).sum()
    }

    fn onset(x: &[f32]) -> usize {
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        x.iter().position(|v| v.abs() > peak * 0.3).unwrap()
    }

    #[test]
    fn lateral_source_has_ild_and_itd() {
        let model = HrtfModel::new(48000.0, 1024);
        let room = RoomSpec { size: 0.3, reflections: 0.0, damping: 0.5, distance: 2.0, order: 0 };
        let [l, r] = synth_brir(&model, 90.0, 0.0, &room, 512, 48);
        let ild = 10.0 * (energy(&l) / energy(&r)).log10();
        assert!(ild > 4.0, "ILD {ild} dB");
        assert!(onset(&l) + 15 < onset(&r), "ITD {} vs {}", onset(&l), onset(&r));

        let [l2, r2] = synth_brir(&model, -90.0, 0.0, &room, 512, 48);
        assert!(energy(&r2) > energy(&l2) * 2.0);
    }

    #[test]
    fn frontal_source_is_symmetric() {
        let model = HrtfModel::new(48000.0, 1024);
        let room = RoomSpec { size: 0.3, reflections: 0.5, damping: 0.5, distance: 2.0, order: 1 };
        let [l, r] = synth_brir(&model, 0.0, 0.0, &room, 512, 48);
        let ild = 10.0 * (energy(&l) / energy(&r)).log10();
        assert!(ild.abs() < 1.5, "ILD {ild} dB");
    }

    #[test]
    fn reflections_add_energy_after_direct() {
        let model = HrtfModel::new(48000.0, 4096);
        let dry = RoomSpec { size: 0.5, reflections: 0.0, damping: 0.5, distance: 2.0, order: 0 };
        let wet = RoomSpec { reflections: 1.0, order: 2, ..dry };
        let [a, _] = synth_brir(&model, 30.0, 0.0, &dry, 2048, 48);
        let [b, _] = synth_brir(&model, 30.0, 0.0, &wet, 2048, 48);
        let tail = |x: &[f32]| energy(&x[400..]);
        assert!(tail(&b) > tail(&a) * 10.0);
    }
}
