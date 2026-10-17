//! Uniformly partitioned overlap-save convolution: N input channels, each filtered by a pair of
//! impulse responses and summed into a stereo output. All filter spectra are precomputed, so the
//! real-time path only does one forward FFT per active input and two inverse FFTs per block.

use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use std::sync::Arc;

/// Frequency-domain filter set, shareable between processors (cached by the plan builder).
pub struct FilterBank {
    pub block: usize,
    pub inputs: usize,
    pub parts: usize,
    /// Indexed [input][ear][part][bin].
    spectra: Vec<Complex32>,
}

impl FilterBank {
    /// `irs[i]` is the (left, right) response pair for input `i`. Empty pairs mean "silent input".
    pub fn new(block: usize, irs: &[[Vec<f32>; 2]]) -> Self {
        let bins = block + 1;
        let max_len = irs.iter().flat_map(|p| p.iter().map(Vec::len)).max().unwrap_or(0).max(1);
        let parts = max_len.div_ceil(block);
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(2 * block);
        let mut spectra = vec![Complex32::new(0.0, 0.0); irs.len() * 2 * parts * bins];
        let mut time = vec![0.0f32; 2 * block];
        let mut spec = vec![Complex32::new(0.0, 0.0); bins];
        for (i, pair) in irs.iter().enumerate() {
            for (e, ir) in pair.iter().enumerate() {
                for p in 0..parts {
                    time.iter_mut().for_each(|t| *t = 0.0);
                    let start = p * block;
                    if start < ir.len() {
                        let end = (start + block).min(ir.len());
                        time[..end - start].copy_from_slice(&ir[start..end]);
                    }
                    fft.process(&mut time, &mut spec).expect("fft");
                    let off = ((i * 2 + e) * parts + p) * bins;
                    spectra[off..off + bins].copy_from_slice(&spec);
                }
            }
        }
        Self { block, inputs: irs.len(), parts, spectra }
    }

    fn part(&self, input: usize, ear: usize, part: usize) -> &[Complex32] {
        let bins = self.block + 1;
        let off = ((input * 2 + ear) * self.parts + part) * bins;
        &self.spectra[off..off + bins]
    }
}

pub struct Convolver {
    bank: Arc<FilterBank>,
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
    /// Frequency-domain delay line, [input][part][bin], ring-indexed by `head`.
    fdl: Vec<Complex32>,
    head: usize,
    /// Previous input block per channel (overlap-save history).
    history: Vec<f32>,
    /// Blocks since each input last carried signal; inputs silent longer than the filter are skipped.
    silent: Vec<usize>,
    time: Vec<f32>,
    acc: [Vec<Complex32>; 2],
    out_time: Vec<f32>,
    scratch_f: Vec<Complex32>,
    scratch_i: Vec<Complex32>,
}

impl Convolver {
    pub fn new(bank: Arc<FilterBank>) -> Self {
        let b = bank.block;
        let bins = b + 1;
        let mut planner = RealFftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(2 * b);
        let inv = planner.plan_fft_inverse(2 * b);
        let scratch_f = fwd.make_scratch_vec();
        let scratch_i = inv.make_scratch_vec();
        Self {
            fdl: vec![Complex32::new(0.0, 0.0); bank.inputs * bank.parts * bins],
            head: 0,
            history: vec![0.0; bank.inputs * b],
            silent: vec![usize::MAX / 2; bank.inputs],
            time: vec![0.0; 2 * b],
            acc: [vec![Complex32::new(0.0, 0.0); bins], vec![Complex32::new(0.0, 0.0); bins]],
            out_time: vec![0.0; 2 * b],
            scratch_f,
            scratch_i,
            fwd,
            inv,
            bank,
        }
    }

    pub fn block(&self) -> usize {
        self.bank.block
    }

    /// Processes exactly one block. `inputs.len()` must equal the bank's input count and every
    /// slice must be `block` long. Output is written (not added) to `out_l` / `out_r`.
    pub fn process(&mut self, inputs: &[&[f32]], out_l: &mut [f32], out_r: &mut [f32]) {
        let b = self.bank.block;
        let bins = b + 1;
        let parts = self.bank.parts;
        self.head = (self.head + 1) % parts;
        self.acc.iter_mut().for_each(|a| a.iter_mut().for_each(|c| *c = Complex32::new(0.0, 0.0)));

        for (i, input) in inputs.iter().enumerate().take(self.bank.inputs) {
            let active = input.iter().any(|&x| x != 0.0);
            self.silent[i] = if active { 0 } else { self.silent[i].saturating_add(1) };
            let fdl_off = i * parts * bins;
            if self.silent[i] > parts + 1 {
                continue;
            }

            // Overlap-save input: [previous block | current block].
            let hist = &mut self.history[i * b..(i + 1) * b];
            self.time[..b].copy_from_slice(hist);
            self.time[b..].copy_from_slice(input);
            hist.copy_from_slice(input);
            let slot = &mut self.fdl[fdl_off + self.head * bins..fdl_off + (self.head + 1) * bins];
            if self.silent[i] == 0 || self.time.iter().any(|&x| x != 0.0) {
                self.fwd.process_with_scratch(&mut self.time, slot, &mut self.scratch_f).expect("fft");
            } else {
                slot.iter_mut().for_each(|c| *c = Complex32::new(0.0, 0.0));
            }

            for p in 0..parts {
                let idx = (self.head + parts - p) % parts;
                let x = &self.fdl[fdl_off + idx * bins..fdl_off + (idx + 1) * bins];
                for (e, acc) in self.acc.iter_mut().enumerate() {
                    let h = self.bank.part(i, e, p);
                    for k in 0..bins {
                        acc[k] += x[k] * h[k];
                    }
                }
            }
        }

        let scale = 1.0 / (2 * b) as f32;
        for (e, out) in [out_l, out_r].into_iter().enumerate() {
            let acc = &mut self.acc[e];
            acc[0].im = 0.0;
            acc[b].im = 0.0;
            let _ = self.inv.process_with_scratch(acc, &mut self.out_time, &mut self.scratch_i);
            for (o, &t) in out.iter_mut().zip(&self.out_time[b..]) {
                *o = t * scale;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_direct_convolution() {
        let b = 64;
        let ir_l: Vec<f32> = (0..300).map(|i| ((i * 7919) % 101) as f32 / 101.0 - 0.5).collect();
        let ir_r: Vec<f32> = (0..300).map(|i| ((i * 104729) % 97) as f32 / 97.0 - 0.5).collect();
        let ir2: Vec<f32> = (0..50).map(|i| if i == 10 { 1.0 } else { 0.0 }).collect();
        let bank = Arc::new(FilterBank::new(b, &[[ir_l.clone(), ir_r.clone()], [ir2.clone(), ir2.clone()]]));
        let mut conv = Convolver::new(bank);

        let n = b * 12;
        let x0: Vec<f32> = (0..n).map(|i| ((i * 31) % 17) as f32 / 17.0 - 0.5).collect();
        let x1: Vec<f32> = (0..n).map(|i| ((i * 13) % 11) as f32 / 11.0 - 0.5).collect();
        let mut yl = vec![0.0; n];
        let mut yr = vec![0.0; n];
        for blk in 0..n / b {
            let r = blk * b..(blk + 1) * b;
            let (mut l, mut rr) = (vec![0.0; b], vec![0.0; b]);
            conv.process(&[&x0[r.clone()], &x1[r.clone()]], &mut l, &mut rr);
            yl[r.clone()].copy_from_slice(&l);
            yr[r].copy_from_slice(&rr);
        }
        let direct = |x: &[f32], h: &[f32], i: usize| (0..h.len()).filter(|&k| k <= i).map(|k| x[i - k] * h[k]).sum::<f32>();
        for i in 0..n {
            let el = direct(&x0, &ir_l, i) + direct(&x1, &ir2, i);
            let er = direct(&x0, &ir_r, i) + direct(&x1, &ir2, i);
            assert!((yl[i] - el).abs() < 1e-3, "L {i}: {} vs {}", yl[i], el);
            assert!((yr[i] - er).abs() < 1e-3, "R {i}: {} vs {}", yr[i], er);
        }
    }
}
