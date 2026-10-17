//! The internal 7.1.4 speaker bed, input channel mapping and object panning.

use crate::params::SpatialParams;

pub const BED: usize = 12;

pub const L: usize = 0;
pub const R: usize = 1;
pub const C: usize = 2;
pub const LFE: usize = 3;
pub const LS: usize = 4;
pub const RS: usize = 5;
pub const LB: usize = 6;
pub const RB: usize = 7;
pub const LTF: usize = 8;
pub const RTF: usize = 9;
pub const LTR: usize = 10;
pub const RTR: usize = 11;

pub const HEIGHT_ELEVATION: f32 = 40.0;

/// (azimuth, elevation) in degrees for every bed channel. LFE is unused (not virtualized).
pub fn bed_directions(sp: &SpatialParams) -> [(f32, f32); BED] {
    let fa = sp.front_angle.clamp(15.0, 60.0);
    let sa = sp.surround_angle.clamp(80.0, 130.0);
    let ba = (sa + 40.0).min(155.0);
    let e = HEIGHT_ELEVATION;
    [
        (fa, 0.0),
        (-fa, 0.0),
        (0.0, 0.0),
        (0.0, 0.0),
        (sa, 0.0),
        (-sa, 0.0),
        (ba, 0.0),
        (-ba, 0.0),
        (45.0, e),
        (-45.0, e),
        (135.0, e),
        (-135.0, e),
    ]
}

/// Maps a device channel count (WAVEFORMATEXTENSIBLE ordering) onto bed channels.
pub fn input_map(channels: usize) -> Vec<Option<usize>> {
    let m: &[usize] = match channels {
        1 => &[C],
        2 => &[L, R],
        3 => &[L, R, C],
        4 => &[L, R, LB, RB],
        5 => &[L, R, C, LS, RS],
        6 => &[L, R, C, LFE, LS, RS],
        8 => &[L, R, C, LFE, LB, RB, LS, RS],
        10 => &[L, R, C, LFE, LS, RS, LTF, RTF, LTR, RTR],
        12 => &[L, R, C, LFE, LB, RB, LS, RS, LTF, RTF, LTR, RTR],
        _ => &[L, R],
    };
    (0..channels).map(|i| m.get(i).copied()).collect()
}

/// ITU-style stereo downmix coefficients (left, right) for a bed channel.
pub fn downmix(ch: usize) -> (f32, f32) {
    const H: f32 = std::f32::consts::FRAC_1_SQRT_2;
    match ch {
        L => (1.0, 0.0),
        R => (0.0, 1.0),
        C | LFE => (H, H),
        LS | LB | LTF | LTR => (H, 0.0),
        RS | RB | RTF | RTR => (0.0, H),
        _ => (0.0, 0.0),
    }
}

fn wrap(a: f32) -> f32 {
    let mut a = a % 360.0;
    if a > 180.0 {
        a -= 360.0;
    }
    if a <= -180.0 {
        a += 360.0;
    }
    a
}

/// Constant-power pairwise panning around a ring of (channel, azimuth) speakers.
fn pan_ring(ring: &[(usize, f32)], az: f32, scale: f32, gains: &mut [f32; BED]) {
    let az = wrap(az);
    let mut sorted: Vec<(usize, f32)> = ring.to_vec();
    sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    let n = sorted.len();
    for i in 0..n {
        let (c0, a0) = sorted[i];
        let (c1, mut a1) = sorted[(i + 1) % n];
        if i + 1 == n {
            a1 += 360.0;
        }
        let mut x = az;
        if x < a0 {
            x += 360.0;
        }
        if x >= a0 && x <= a1 {
            let t = if a1 > a0 { (x - a0) / (a1 - a0) } else { 0.0 };
            let th = t * std::f32::consts::FRAC_PI_2;
            gains[c0] += th.cos() * scale;
            gains[c1] += th.sin() * scale;
            return;
        }
    }
}

/// Panning gains that place a point source at (azimuth, elevation) on the bed.
pub fn pan_object(sp: &SpatialParams, az: f32, el: f32, gains: &mut [f32; BED]) {
    gains.iter_mut().for_each(|g| *g = 0.0);
    let dirs = bed_directions(sp);
    let ear: Vec<(usize, f32)> = [C, L, LS, LB, RB, RS, R].iter().map(|&c| (c, dirs[c].0)).collect();
    let top: Vec<(usize, f32)> = [LTF, LTR, RTR, RTF].iter().map(|&c| (c, dirs[c].0)).collect();
    let e = (el.max(0.0) / HEIGHT_ELEVATION).min(1.0) * std::f32::consts::FRAC_PI_2;
    pan_ring(&ear, az, e.cos(), gains);
    pan_ring(&top, az, e.sin(), gains);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pan_hits_speakers_exactly() {
        let sp = SpatialParams::default();
        let mut g = [0.0; BED];
        pan_object(&sp, 30.0, 0.0, &mut g);
        assert!((g[L] - 1.0).abs() < 1e-4);
        pan_object(&sp, 180.0, 0.0, &mut g);
        assert!((g[LB] - g[RB]).abs() < 1e-3 && g[LB] > 0.6);
        pan_object(&sp, 0.0, 90.0, &mut g);
        assert!((g[LTF] - g[RTF]).abs() < 1e-3 && g[C] < 1e-4);
    }
}
