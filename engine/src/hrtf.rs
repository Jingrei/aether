//! Parametric head-related transfer function model.
//!
//! Based on the structural model of Brown & Duda (1998): a rigid spherical head (frequency-dependent
//! head shadow plus Woodworth interaural time difference), a pinna echo model that gives elevation and
//! front/back cues, plus two refinements: a rear pinna-shadow shelf and an overhead "Blauert band" lift
//! around 8 kHz. Responses are diffuse-field equalized so that the average over all directions is
//! flat, which keeps the renderer free of overall coloration.
//!
//! Everything is computed directly in the frequency domain so that fractional delays are exact and
//! room images can be summed into a single spectrum before one inverse FFT.

use realfft::num_complex::Complex64;
use std::f64::consts::PI;

pub const HEAD_RADIUS: f64 = 0.0875;
pub const SPEED_OF_SOUND: f64 = 343.0;

/// Pinna echo model (Brown & Duda, table 1): reflection coefficient, A, B (delays in samples at 44.1 kHz).
const PINNA: [(f64, f64, f64); 5] = [(0.5, 1.0, 2.0), (-1.0, 5.0, 4.0), (0.5, 5.0, 7.0), (-0.25, 5.0, 11.0), (0.25, 5.0, 13.0)];
const PINNA_STRENGTH: f64 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ear {
    Left,
    Right,
}

/// Unit vector for azimuth/elevation in degrees. x = front, y = left, z = up. Positive azimuth = left.
pub fn direction(az_deg: f64, el_deg: f64) -> [f64; 3] {
    let (az, el) = (az_deg.to_radians(), el_deg.to_radians());
    [el.cos() * az.cos(), el.cos() * az.sin(), el.sin()]
}

/// Precomputed per-direction parameters for one ear.
struct EarGeometry {
    alpha: f64,
    itd: f64,
    pinna_delays: [f64; 5],
    rear_beta: f64,
    overhead_gain: f64,
}

fn geometry(d: [f64; 3], ear: Ear) -> EarGeometry {
    let ear_axis_y = if ear == Ear::Left { 1.0 } else { -1.0 };
    let cos_theta = (d[1] * ear_axis_y).clamp(-1.0, 1.0);
    let theta = cos_theta.acos(); // 0 = source on the ear axis, PI = opposite side

    // Head shadow: single pole/zero whose zero moves with incidence angle.
    const ALPHA_MIN: f64 = 0.1;
    const THETA_MIN: f64 = 150.0 * PI / 180.0;
    let alpha = (1.0 + ALPHA_MIN / 2.0) + (1.0 - ALPHA_MIN / 2.0) * (theta / THETA_MIN * PI).cos();

    // Woodworth ITD, offset so the earliest possible arrival is zero delay.
    let a_c = HEAD_RADIUS / SPEED_OF_SOUND;
    let itd = if theta < PI / 2.0 { -a_c * theta.cos() } else { a_c * (theta - PI / 2.0) } + a_c;

    // Interaural-polar coordinates for the pinna model.
    let lateral = d[1].clamp(-1.0, 1.0).asin().to_degrees();
    let polar = d[2].atan2(d[0]).to_degrees(); // 0 front, 90 up, +-180 back
    let mut pinna_delays = [0.0; 5];
    for (i, &(_, a, b)) in PINNA.iter().enumerate() {
        let samples = a * (lateral.to_radians() / 2.0).cos() * ((90.0 - polar).to_radians()).sin() + b;
        pinna_delays[i] = samples.max(0.5) / 44100.0;
    }

    // Sources behind the head lose some high frequencies to pinna shadowing.
    let behind = (-d[0]).max(0.0);
    let rear_beta = 10f64.powf(-5.0 * behind / 20.0);
    let overhead_gain = 10f64.powf(4.0 * d[2].max(0.0) / 20.0);

    EarGeometry { alpha, itd, pinna_delays, rear_beta, overhead_gain }
}

/// Spherical-head + pinna HRTF model evaluated on the bins of an `n`-point real FFT.
pub struct HrtfModel {
    pub fs: f64,
    pub n: usize,
    df_inv: Vec<f64>,
    overhead_shape: Vec<f64>,
}

impl HrtfModel {
    pub fn new(fs: f64, n: usize) -> Self {
        let bins = n / 2 + 1;
        let overhead_shape = (0..bins)
            .map(|k| {
                let f = (k as f64 * fs / n as f64).max(1.0);
                let o = (f / 8000.0).log2();
                (-(o * o) / (2.0 * 0.35 * 0.35)).exp()
            })
            .collect();
        let mut model = Self { fs, n, df_inv: vec![1.0; bins], overhead_shape };
        model.df_inv = model.diffuse_field_inverse();
        model
    }

    pub fn bins(&self) -> usize {
        self.n / 2 + 1
    }

    /// Average power response over a uniform sphere, smoothed to 1/3 octave and inverted.
    fn diffuse_field_inverse(&self) -> Vec<f64> {
        let bins = self.bins();
        let mut power = vec![0.0; bins];
        let mut tmp = vec![Complex64::new(0.0, 0.0); bins];
        let count = 256;
        let golden = PI * (3.0 - 5f64.sqrt());
        for i in 0..count {
            let z = 1.0 - 2.0 * (i as f64 + 0.5) / count as f64;
            let r = (1.0 - z * z).sqrt();
            let phi = golden * i as f64;
            let d = [r * phi.cos(), r * phi.sin(), z];
            tmp.iter_mut().for_each(|c| *c = Complex64::new(0.0, 0.0));
            self.accumulate_raw(d, Ear::Left, 1.0, 0.0, 0, 1.0, &mut tmp);
            for (p, c) in power.iter_mut().zip(&tmp) {
                *p += c.norm_sqr() / count as f64;
            }
        }
        // 1/3-octave smoothing using prefix sums.
        let mut prefix = vec![0.0; bins + 1];
        for k in 0..bins {
            prefix[k + 1] = prefix[k] + power[k];
        }
        let w = 2f64.powf(1.0 / 6.0);
        (0..bins)
            .map(|k| {
                let lo = ((k as f64 / w).floor() as usize).min(k);
                let hi = (((k as f64 * w).ceil() as usize) + 1).clamp(k + 1, bins);
                let avg = (prefix[hi] - prefix[lo]) / (hi - lo) as f64;
                (1.0 / avg.max(1e-9).sqrt()).clamp(0.25, 4.0)
            })
            .collect()
    }

    /// Adds `gain * H(d, ear) * e^{-j w delay} * wall(w)^order` to `acc`, without diffuse-field EQ.
    #[allow(clippy::too_many_arguments)]
    fn accumulate_raw(&self, d: [f64; 3], ear: Ear, gain: f64, delay: f64, wall_order: u32, wall_fc: f64, acc: &mut [Complex64]) {
        let g = geometry(d, ear);
        let w0 = SPEED_OF_SOUND / HEAD_RADIUS;
        let dw = 2.0 * PI * self.fs / self.n as f64;
        let wr = 2.0 * PI * 3500.0;
        let wc = 2.0 * PI * wall_fc;

        let total_delay = g.itd + delay;
        let step = Complex64::from_polar(1.0, -dw * total_delay);
        let mut phasor = Complex64::new(1.0, 0.0);
        let mut pinna_step = [Complex64::new(0.0, 0.0); 5];
        let mut pinna_ph = [Complex64::new(1.0, 0.0); 5];
        for i in 0..5 {
            pinna_step[i] = Complex64::from_polar(1.0, -dw * g.pinna_delays[i]);
        }

        for (k, out) in acc.iter_mut().enumerate() {
            let w = dw * k as f64;
            let x = w / (2.0 * w0);
            let shadow = Complex64::new(1.0, g.alpha * x) / Complex64::new(1.0, x);

            let mut pinna = Complex64::new(1.0, 0.0);
            for i in 0..5 {
                pinna += pinna_ph[i] * (PINNA[i].0 * PINNA_STRENGTH);
                pinna_ph[i] *= pinna_step[i];
            }

            let xr = w / wr;
            let rear = Complex64::new(1.0, xr * g.rear_beta) / Complex64::new(1.0, xr);
            let overhead = 1.0 + (g.overhead_gain - 1.0) * self.overhead_shape[k];

            let mut h = shadow * pinna * rear * (overhead * gain) * phasor;
            if wall_order > 0 {
                let wall = Complex64::new(1.0, 0.0) / Complex64::new(1.0, w / wc);
                for _ in 0..wall_order {
                    h *= wall;
                }
            }
            *out += h;
            phasor *= step;
        }
    }

    /// Adds a diffuse-field equalized response for direction `d` into `acc`.
    #[allow(clippy::too_many_arguments)]
    pub fn accumulate(&self, d: [f64; 3], ear: Ear, gain: f64, delay: f64, wall_order: u32, wall_fc: f64, acc: &mut [Complex64]) {
        let mut tmp = vec![Complex64::new(0.0, 0.0); acc.len()];
        self.accumulate_raw(d, ear, gain, delay, wall_order, wall_fc, &mut tmp);
        for ((a, t), eq) in acc.iter_mut().zip(&tmp).zip(&self.df_inv) {
            *a += t * eq;
        }
    }
}
