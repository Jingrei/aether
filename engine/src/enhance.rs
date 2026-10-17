//! Post-render enhancement: EQ chains, bass enhancement, crosstalk cancellation, leveler, limiter.

use crate::biquad::{Biquad, Coeffs};

/// A cascade of biquads applied identically to both channels.
#[derive(Clone, Default)]
pub struct StereoEq {
    filters: Vec<[Biquad; 2]>,
}

impl StereoEq {
    pub fn new(coeffs: impl IntoIterator<Item = Coeffs>) -> Self {
        Self { filters: coeffs.into_iter().map(|c| [Biquad::new(c), Biquad::new(c)]).collect() }
    }

    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        for [fl, fr] in &mut self.filters {
            fl.process(l);
            fr.process(r);
        }
    }
}

/// Low shelf plus psychoacoustic "missing fundamental" harmonics, which make bass audible on
/// small drivers that cannot reproduce it.
pub struct BassEnhancer {
    shelf: [Biquad; 2],
    split: Biquad,
    band: [Biquad; 2],
    harmonics: f32,
    drive: f32,
}

impl BassEnhancer {
    pub fn new(fs: f32, amount: f32, harmonics: f32) -> Self {
        let fs = fs as f64;
        let shelf = Biquad::new(Coeffs::low_shelf(fs, 95.0, 0.6, 7.0 * amount as f64));
        Self {
            shelf: [shelf, shelf],
            split: Biquad::new(Coeffs::lowpass(fs, 110.0, 0.707)),
            band: [Biquad::new(Coeffs::highpass(fs, 140.0, 0.707)), Biquad::new(Coeffs::lowpass(fs, 650.0, 0.707))],
            harmonics: harmonics * amount,
            drive: 4.0,
        }
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        for i in 0..l.len() {
            if self.harmonics > 0.0 {
                let low = self.split.tick(0.5 * (l[i] + r[i]));
                let nl = (low * self.drive).tanh() / self.drive + low.abs() * 0.5;
                let hp = self.band[0].tick(nl);
                let h = self.band[1].tick(hp) * self.harmonics;
                l[i] += h;
                r[i] += h;
            }
            l[i] = self.shelf[0].tick(l[i]);
            r[i] = self.shelf[1].tick(r[i]);
        }
    }
}

/// Recursive crosstalk canceller for a closely spaced speaker pair (laptop / desktop speakers).
/// Each output subtracts a delayed, head-shadowed copy of the other output.
pub struct Xtc {
    delay: usize,
    buf: [Vec<f32>; 2],
    pos: usize,
    shadow: [Biquad; 2],
    keep_lows: [Biquad; 2],
    g: f32,
}

impl Xtc {
    pub fn new(fs: f32, speaker_angle_deg: f32, strength: f32) -> Self {
        let th = speaker_angle_deg.to_radians();
        let itd = crate::hrtf::HEAD_RADIUS as f32 / crate::hrtf::SPEED_OF_SOUND as f32 * (th + th.sin());
        let delay = ((itd * fs).round() as usize).max(1);
        let shadow = Biquad::new(Coeffs::lowpass(fs as f64, 3000.0, 0.5));
        let hp = Biquad::new(Coeffs::highpass(fs as f64, 250.0, 0.707));
        Self { delay, buf: [vec![0.0; delay + 1], vec![0.0; delay + 1]], pos: 0, shadow: [shadow, shadow], keep_lows: [hp, hp], g: 0.75 * strength }
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = self.delay + 1;
        for i in 0..l.len() {
            let read = (self.pos + 1) % n;
            let fb_l = self.shadow[0].tick(self.keep_lows[0].tick(self.buf[1][read]));
            let fb_r = self.shadow[1].tick(self.keep_lows[1].tick(self.buf[0][read]));
            let yl = l[i] - self.g * fb_l;
            let yr = r[i] - self.g * fb_r;
            self.buf[0][self.pos] = yl;
            self.buf[1][self.pos] = yr;
            self.pos = read;
            l[i] = yl;
            r[i] = yr;
        }
    }
}

/// Slow automatic gain control that evens out loud and quiet passages.
pub struct Leveler {
    amount: f32,
    power: f32,
    gain_db: f32,
    p_coef: f32,
    up: f32,
    down: f32,
}

impl Leveler {
    pub fn new(fs: f32, amount: f32) -> Self {
        Self {
            amount,
            power: 0.0,
            gain_db: 0.0,
            p_coef: 1.0 - (-1.0 / (0.4 * fs)).exp(),
            up: 1.0 - (-1.0 / (2.5 * fs)).exp(),
            down: 1.0 - (-1.0 / (0.25 * fs)).exp(),
        }
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        if self.amount <= 0.0 {
            return;
        }
        const TARGET_DB: f32 = -20.0;
        for i in 0..l.len() {
            self.power += self.p_coef * (0.5 * (l[i] * l[i] + r[i] * r[i]) - self.power);
            let level_db = 10.0 * self.power.max(1e-12).log10();
            if level_db > -55.0 {
                let want = ((TARGET_DB - level_db) * self.amount).clamp(-14.0, 12.0 * self.amount);
                let c = if want > self.gain_db { self.up } else { self.down };
                self.gain_db += c * (want - self.gain_db);
            }
            let g = 10f32.powf(self.gain_db / 20.0);
            l[i] *= g;
            r[i] *= g;
        }
    }
}

/// Look-ahead peak limiter.
pub struct Limiter {
    ceiling: f32,
    delay: Vec<[f32; 2]>,
    pos: usize,
    gain: f32,
    smooth: f32,
    release: f32,
    peaks: Vec<f32>,
}

impl Limiter {
    pub fn new(fs: f32, ceiling_db: f32) -> Self {
        let la = ((0.0015 * fs) as usize).max(8);
        Self {
            ceiling: 10f32.powf(ceiling_db / 20.0),
            delay: vec![[0.0; 2]; la],
            pos: 0,
            gain: 1.0,
            smooth: 1.0,
            release: 1.0 - (-1.0 / (0.08 * fs)).exp(),
            peaks: vec![0.0; la],
        }
    }

    pub fn latency(&self) -> usize {
        self.delay.len()
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = self.delay.len();
        let attack = 1.0 - (-1.0 / (n as f32 / 4.0)).exp();
        for i in 0..l.len() {
            let [dl, dr] = self.delay[self.pos];
            self.delay[self.pos] = [l[i], r[i]];
            self.peaks[self.pos] = l[i].abs().max(r[i].abs());
            self.pos = (self.pos + 1) % n;

            let peak = self.peaks.iter().fold(0.0f32, |m, &p| m.max(p));
            let need = if peak > self.ceiling { self.ceiling / peak } else { 1.0 };
            self.gain = if need < self.gain { need } else { self.gain + self.release * (need - self.gain) };
            self.smooth += attack * (self.gain - self.smooth);
            let g = self.smooth;
            l[i] = (dl * g).clamp(-0.999, 0.999);
            r[i] = (dr * g).clamp(-0.999, 0.999);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_holds_ceiling() {
        let mut lim = Limiter::new(48000.0, -1.0);
        let mut l: Vec<f32> = (0..9600).map(|i| (i as f32 * 0.05).sin() * 3.0).collect();
        let mut r = l.clone();
        lim.process(&mut l, &mut r);
        let ceil = 10f32.powf(-1.0 / 20.0);
        assert!(l[200..].iter().all(|v| v.abs() <= ceil * 1.02));
    }

    #[test]
    fn xtc_is_stable() {
        let mut x = Xtc::new(48000.0, 10.0, 1.0);
        let mut l = vec![0.0; 48000];
        let mut r = vec![0.0; 48000];
        l[0] = 1.0;
        x.process(&mut l, &mut r);
        assert!(l[40000..].iter().chain(&r[40000..]).all(|v| v.abs() < 1e-4));
    }
}
