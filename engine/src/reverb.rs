//! 8-line feedback delay network for the diffuse late reverb tail.

use crate::biquad::{Biquad, Coeffs, OnePole};

const BASE_MS: [f32; 8] = [29.7, 37.1, 41.1, 43.7, 53.3, 59.9, 67.1, 73.3];

struct Line {
    buf: Vec<f32>,
    pos: usize,
    gain: f32,
    damp: OnePole,
}

pub struct Fdn {
    lines: Vec<Line>,
    pre: Vec<f32>,
    pre_pos: usize,
    hp: [Biquad; 2],
    out_gain: f32,
}

impl Fdn {
    /// `size` 0..1, `rt60` seconds, `damping` 0..1.
    pub fn new(fs: f32, size: f32, rt60: f32, damping: f32) -> Self {
        let scale = 0.6 + 1.2 * size;
        let rt60 = rt60.max(0.05);
        let fc = 12000.0 * (1.0 - 0.75 * damping);
        let mut energy = 0.0;
        let lines: Vec<Line> = BASE_MS
            .iter()
            .map(|ms| {
                let len = ((ms * scale * fs / 1000.0) as usize).max(16);
                let gain = 10f32.powf(-3.0 * len as f32 / (fs * rt60));
                energy += 1.0 / (1.0 - gain * gain).max(1e-4);
                Line { buf: vec![0.0; len], pos: 0, gain, damp: OnePole::lowpass(fs, fc) }
            })
            .collect();
        // Normalize so a unit-power input gives roughly unit-power output.
        let out_gain = (8.0 / energy).sqrt();
        let pre_len = ((0.008 + 0.025 * size) * fs) as usize + 1;
        let hp = Biquad::new(Coeffs::highpass(fs as f64, 180.0, 0.707));
        Self { lines, pre: vec![0.0; pre_len * 2], pre_pos: 0, hp: [hp, hp], out_gain }
    }

    #[inline]
    pub fn tick(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        let half = self.pre.len() / 2;
        let il = self.pre[self.pre_pos * 2];
        let ir = self.pre[self.pre_pos * 2 + 1];
        self.pre[self.pre_pos * 2] = self.hp[0].tick(in_l);
        self.pre[self.pre_pos * 2 + 1] = self.hp[1].tick(in_r);
        self.pre_pos = (self.pre_pos + 1) % half;

        let mut o = [0.0f32; 8];
        for (i, line) in self.lines.iter_mut().enumerate() {
            o[i] = line.damp.tick(line.buf[line.pos]) * line.gain;
        }
        // Fast Walsh-Hadamard transform (orthogonal mixing matrix once normalized).
        let mut h = 1;
        while h < 8 {
            for i in (0..8).step_by(h * 2) {
                for j in i..i + h {
                    let (a, b) = (o[j], o[j + h]);
                    o[j] = a + b;
                    o[j + h] = a - b;
                }
            }
            h *= 2;
        }
        let norm = 1.0 / 8f32.sqrt();
        let mut out_l = 0.0;
        let mut out_r = 0.0;
        for (i, line) in self.lines.iter_mut().enumerate() {
            let y = line.buf[line.pos];
            if i % 2 == 0 {
                out_l += if i % 4 == 0 { y } else { -y };
            } else {
                out_r += if i % 4 == 1 { y } else { -y };
            }
            let input = if i % 2 == 0 { il } else { ir };
            let sign = if i % 3 == 0 { -1.0 } else { 1.0 };
            line.buf[line.pos] = o[i] * norm + input * sign * 0.5;
            line.pos = (line.pos + 1) % line.buf.len();
        }
        (out_l * self.out_gain, out_r * self.out_gain)
    }
}
