//! Stereo to 7.1.4 upmixer.
//!
//! * Center: correlation-steered extraction of the mid signal (panned-center content such as dialog).
//! * Surrounds: the side (L-R) ambience, delayed and decorrelated with allpass chains, lowpassed.
//! * Heights: high-passed, decorrelated front content at a low level to lift the sound stage.

use crate::biquad::{Biquad, Coeffs};
use crate::layout::*;

struct Allpass {
    buf: Vec<f32>,
    pos: usize,
    g: f32,
}

impl Allpass {
    fn new(fs: f32, ms: f32, g: f32) -> Self {
        Self { buf: vec![0.0; ((ms * fs / 1000.0) as usize).max(1)], pos: 0, g }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let d = self.buf[self.pos];
        let w = x + self.g * d;
        self.buf[self.pos] = w;
        self.pos = (self.pos + 1) % self.buf.len();
        d - self.g * w
    }
}

struct Decorrelator {
    delay: Vec<f32>,
    pos: usize,
    aps: [Allpass; 3],
    lp: Biquad,
    hp: Biquad,
}

impl Decorrelator {
    fn new(fs: f32, delay_ms: f32, ap_ms: [f32; 3], lp: f32, hp: f32) -> Self {
        Self {
            delay: vec![0.0; ((delay_ms * fs / 1000.0) as usize).max(1)],
            pos: 0,
            aps: ap_ms.map(|ms| Allpass::new(fs, ms, 0.55)),
            lp: Biquad::new(Coeffs::lowpass(fs as f64, lp as f64, 0.707)),
            hp: Biquad::new(Coeffs::highpass(fs as f64, hp as f64, 0.707)),
        }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let d = self.delay[self.pos];
        self.delay[self.pos] = x;
        self.pos = (self.pos + 1) % self.delay.len();
        let mut y = d;
        for ap in &mut self.aps {
            y = ap.tick(y);
        }
        self.hp.tick(self.lp.tick(y))
    }
}

pub struct Upmixer {
    pll: f32,
    prr: f32,
    plr: f32,
    smooth: f32,
    gain_smooth: f32,
    cg: f32,
    surround: [Decorrelator; 4],
    height: [Decorrelator; 4],
}

impl Upmixer {
    pub fn new(fs: f32) -> Self {
        Self {
            pll: 0.0,
            prr: 0.0,
            plr: 0.0,
            smooth: 1.0 - (-1.0 / (0.04 * fs)).exp(),
            gain_smooth: 1.0 - (-1.0 / (0.08 * fs)).exp(),
            cg: 0.0,
            surround: [
                Decorrelator::new(fs, 9.0, [3.1, 4.7, 7.3], 7500.0, 120.0),
                Decorrelator::new(fs, 10.5, [2.9, 5.3, 6.7], 7500.0, 120.0),
                Decorrelator::new(fs, 16.0, [3.7, 5.9, 8.3], 6000.0, 150.0),
                Decorrelator::new(fs, 17.5, [4.1, 6.1, 7.9], 6000.0, 150.0),
            ],
            height: [
                Decorrelator::new(fs, 3.0, [1.3, 2.3, 3.4], 16000.0, 1800.0),
                Decorrelator::new(fs, 3.6, [1.7, 2.1, 3.1], 16000.0, 1800.0),
                Decorrelator::new(fs, 12.0, [2.2, 3.3, 4.4], 12000.0, 2000.0),
                Decorrelator::new(fs, 13.0, [2.6, 3.1, 4.9], 12000.0, 2000.0),
            ],
        }
    }

    /// Writes `l`/`r` into the bed (which must already be zeroed).
    pub fn process(&mut self, l: &[f32], r: &[f32], amount: f32, center: f32, height: f32, bed: &mut [Vec<f32>]) {
        let amb = amount * 0.85;
        let hgt = height * amount * 0.45;
        for i in 0..l.len() {
            let (xl, xr) = (l[i], r[i]);
            self.pll += self.smooth * (xl * xl - self.pll);
            self.prr += self.smooth * (xr * xr - self.prr);
            self.plr += self.smooth * (xl * xr - self.plr);
            let corr = self.plr / (self.pll * self.prr).sqrt().max(1e-9);
            let target = center * amount.max(0.35) * corr.max(0.0).powi(2);
            self.cg += self.gain_smooth * (target - self.cg);

            let mid = 0.5 * (xl + xr);
            let side = 0.5 * (xl - xr);
            let c = self.cg * mid;
            let fl = xl - c;
            let fr = xr - c;
            bed[L][i] = fl;
            bed[R][i] = fr;
            bed[C][i] = c * std::f32::consts::SQRT_2;
            if amb > 0.0 {
                bed[LS][i] = self.surround[0].tick(side) * amb;
                bed[RS][i] = -self.surround[1].tick(side) * amb;
                bed[LB][i] = self.surround[2].tick(side) * amb * 0.7;
                bed[RB][i] = -self.surround[3].tick(side) * amb * 0.7;
            }
            if hgt > 0.0 {
                bed[LTF][i] = self.height[0].tick(fl) * hgt;
                bed[RTF][i] = self.height[1].tick(fr) * hgt;
                bed[LTR][i] = self.height[2].tick(side) * hgt * 0.6;
                bed[RTR][i] = -self.height[3].tick(side) * hgt * 0.6;
            }
        }
    }
}
