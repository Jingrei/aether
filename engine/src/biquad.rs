//! RBJ "Audio EQ Cookbook" biquads.

use crate::params::{EqFilter, FilterKind};
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Default for Coeffs {
    fn default() -> Self {
        Self::identity()
    }
}

fn prewarp(fs: f64, f: f64, q: f64) -> (f64, f64, f64) {
    let f = f.clamp(5.0, fs * 0.45);
    let w0 = 2.0 * PI * f / fs;
    let alpha = w0.sin() / (2.0 * q.max(0.05));
    (w0.cos(), alpha, w0)
}

impl Coeffs {
    pub const fn identity() -> Self {
        Self { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 }
    }

    fn norm(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    pub fn peaking(fs: f64, f: f64, q: f64, gain_db: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let (cos, alpha, _) = prewarp(fs, f, q);
        Self::norm(1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a)
    }

    pub fn low_shelf(fs: f64, f: f64, q: f64, gain_db: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let (cos, alpha, _) = prewarp(fs, f, q);
        let sa = 2.0 * a.sqrt() * alpha;
        Self::norm(
            a * ((a + 1.0) - (a - 1.0) * cos + sa),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
            a * ((a + 1.0) - (a - 1.0) * cos - sa),
            (a + 1.0) + (a - 1.0) * cos + sa,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos),
            (a + 1.0) + (a - 1.0) * cos - sa,
        )
    }

    pub fn high_shelf(fs: f64, f: f64, q: f64, gain_db: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let (cos, alpha, _) = prewarp(fs, f, q);
        let sa = 2.0 * a.sqrt() * alpha;
        Self::norm(
            a * ((a + 1.0) + (a - 1.0) * cos + sa),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
            a * ((a + 1.0) + (a - 1.0) * cos - sa),
            (a + 1.0) - (a - 1.0) * cos + sa,
            2.0 * ((a - 1.0) - (a + 1.0) * cos),
            (a + 1.0) - (a - 1.0) * cos - sa,
        )
    }

    pub fn lowpass(fs: f64, f: f64, q: f64) -> Self {
        let (cos, alpha, _) = prewarp(fs, f, q);
        Self::norm((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
    }

    pub fn highpass(fs: f64, f: f64, q: f64) -> Self {
        let (cos, alpha, _) = prewarp(fs, f, q);
        Self::norm((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
    }

    pub fn from_filter(fs: f64, f: &EqFilter) -> Self {
        let (fr, q, g) = (f.freq as f64, f.q as f64, f.gain as f64);
        match f.kind {
            FilterKind::Peak => Self::peaking(fs, fr, q, g),
            FilterKind::LowShelf => Self::low_shelf(fs, fr, q, g),
            FilterKind::HighShelf => Self::high_shelf(fs, fr, q, g),
            FilterKind::LowPass => Self::lowpass(fs, fr, q),
            FilterKind::HighPass => Self::highpass(fs, fr, q),
        }
    }

    /// Magnitude response in dB at frequency `f`.
    pub fn magnitude_db(&self, fs: f64, f: f64) -> f64 {
        let w = 2.0 * PI * f / fs;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = -(self.a1 * s1 + self.a2 * s2);
        10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
    }
}

/// Transposed direct form II biquad with f64 state (stable for low-frequency filters).
#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    pub c: Coeffs,
    z1: f64,
    z2: f64,
}

impl Biquad {
    pub fn new(c: Coeffs) -> Self {
        Self { c, z1: 0.0, z2: 0.0 }
    }

    #[inline(always)]
    pub fn tick(&mut self, x: f32) -> f32 {
        let x = x as f64;
        let y = self.c.b0 * x + self.z1;
        self.z1 = self.c.b1 * x - self.c.a1 * y + self.z2;
        self.z2 = self.c.b2 * x - self.c.a2 * y;
        y as f32
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        for s in buf {
            *s = self.tick(*s);
        }
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// One-pole lowpass, `y += a (x - y)`.
#[derive(Clone, Copy, Debug, Default)]
pub struct OnePole {
    a: f32,
    y: f32,
}

impl OnePole {
    pub fn lowpass(fs: f32, fc: f32) -> Self {
        Self { a: 1.0 - (-2.0 * std::f32::consts::PI * fc / fs).exp(), y: 0.0 }
    }

    #[inline(always)]
    pub fn tick(&mut self, x: f32) -> f32 {
        self.y += self.a * (x - self.y);
        self.y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaking_hits_gain_at_center() {
        let c = Coeffs::peaking(48000.0, 1000.0, 1.0, 6.0);
        assert!((c.magnitude_db(48000.0, 1000.0) - 6.0).abs() < 0.01);
        assert!(c.magnitude_db(48000.0, 20.0).abs() < 0.2);
    }

    #[test]
    fn shelves_reach_gain() {
        let ls = Coeffs::low_shelf(48000.0, 100.0, 0.707, 6.0);
        assert!((ls.magnitude_db(48000.0, 20.0) - 6.0).abs() < 0.5);
        let hs = Coeffs::high_shelf(48000.0, 5000.0, 0.707, -4.0);
        assert!((hs.magnitude_db(48000.0, 18000.0) + 4.0).abs() < 0.5);
    }
}
