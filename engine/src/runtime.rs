//! Real-time side of the engine: block adaptation, glitch-free processor swaps (crossfaded) and
//! lock-free metering. Nothing here allocates once constructed.

use crate::processor::{Processor, BLOCK};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const FADE: usize = 2048;
const TAP_LEN: usize = 4096;

/// Latest output samples for the spectrum analyzer (written by the audio thread, read by the UI).
pub struct SpectrumTap {
    buf: Vec<AtomicU32>,
    idx: AtomicUsize,
}

impl SpectrumTap {
    fn new() -> Self {
        Self { buf: (0..TAP_LEN).map(|_| AtomicU32::new(0)).collect(), idx: AtomicUsize::new(0) }
    }

    fn push(&self, v: f32) {
        let i = self.idx.load(Ordering::Relaxed);
        self.buf[i % TAP_LEN].store(v.to_bits(), Ordering::Relaxed);
        self.idx.store(i.wrapping_add(1), Ordering::Relaxed);
    }

    /// Copies the most recent `out.len()` samples (oldest first).
    pub fn snapshot(&self, out: &mut [f32]) {
        let end = self.idx.load(Ordering::Relaxed);
        let n = out.len().min(TAP_LEN);
        for (k, o) in out.iter_mut().take(n).enumerate() {
            let i = end.wrapping_sub(n).wrapping_add(k);
            *o = f32::from_bits(self.buf[i % TAP_LEN].load(Ordering::Relaxed));
        }
    }
}

pub struct Meters {
    in_peak: [AtomicU32; 2],
    out_peak: [AtomicU32; 2],
    load: AtomicU32,
    pub tap: SpectrumTap,
}

fn store_max(a: &AtomicU32, v: f32) {
    let cur = f32::from_bits(a.load(Ordering::Relaxed));
    if v > cur {
        a.store(v.to_bits(), Ordering::Relaxed);
    }
}

impl Meters {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            in_peak: [AtomicU32::new(0), AtomicU32::new(0)],
            out_peak: [AtomicU32::new(0), AtomicU32::new(0)],
            load: AtomicU32::new(0),
            tap: SpectrumTap::new(),
        })
    }

    /// Returns and resets the peak levels since the last call: (input L/R, output L/R).
    pub fn take_peaks(&self) -> ([f32; 2], [f32; 2]) {
        let t = |a: &AtomicU32| f32::from_bits(a.swap(0, Ordering::Relaxed));
        ([t(&self.in_peak[0]), t(&self.in_peak[1])], [t(&self.out_peak[0]), t(&self.out_peak[1])])
    }

    /// DSP time as a fraction of real time.
    pub fn load(&self) -> f32 {
        f32::from_bits(self.load.load(Ordering::Relaxed))
    }
}

/// Mailbox between the processor builder and the audio thread.
pub struct ProcessorSlot {
    incoming: Mutex<Option<Box<Processor>>>,
    retired: Mutex<Vec<Box<Processor>>>,
}

impl ProcessorSlot {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { incoming: Mutex::new(None), retired: Mutex::new(Vec::with_capacity(16)) })
    }

    /// Publishes a new processor (replacing any not yet picked up).
    pub fn publish(&self, p: Box<Processor>) {
        let old = self.incoming.lock().unwrap().replace(p);
        drop(old);
    }

    /// Frees processors the audio thread has finished with. Call from a non-real-time thread.
    pub fn collect_garbage(&self) {
        let items: Vec<_> = self.retired.lock().unwrap().drain(..).collect();
        drop(items);
    }

    fn try_take(&self) -> Option<Box<Processor>> {
        self.incoming.try_lock().ok()?.take()
    }

    fn retire(&self, p: Box<Processor>) {
        if let Ok(mut r) = self.retired.try_lock() {
            if r.len() < r.capacity() {
                r.push(p);
                return;
            }
        }
        drop(p); // rare fallback: deallocate in place
    }
}

/// Owned by the audio output callback.
pub struct Runtime {
    slot: Arc<ProcessorSlot>,
    meters: Arc<Meters>,
    fs: f32,
    in_ch: usize,
    current: Option<Box<Processor>>,
    previous: Option<Box<Processor>>,
    fade_pos: usize,
    frame: Vec<f32>,
    input: Vec<Vec<f32>>,
    out: [Vec<f32>; 2],
    prev_out: [Vec<f32>; 2],
    out_pos: usize,
}

impl Runtime {
    pub fn new(slot: Arc<ProcessorSlot>, meters: Arc<Meters>, fs: f32, in_ch: usize) -> Self {
        let in_ch = in_ch.max(1);
        Self {
            slot,
            meters,
            fs,
            in_ch,
            current: None,
            previous: None,
            fade_pos: 0,
            frame: vec![0.0; in_ch],
            input: vec![vec![0.0; BLOCK]; in_ch],
            out: [vec![0.0; BLOCK], vec![0.0; BLOCK]],
            prev_out: [vec![0.0; BLOCK], vec![0.0; BLOCK]],
            out_pos: BLOCK,
        }
    }

    /// Fills interleaved `out` (with `out_ch` channels). `pull` must write one interleaved input frame
    /// of `in_ch` samples (zeros on underrun).
    pub fn render(&mut self, out: &mut [f32], out_ch: usize, mut pull: impl FnMut(&mut [f32])) {
        let out_ch = out_ch.max(1);
        for frame in out.chunks_mut(out_ch) {
            if self.out_pos == BLOCK {
                self.produce(&mut pull);
                self.out_pos = 0;
            }
            let (l, r) = (self.out[0][self.out_pos], self.out[1][self.out_pos]);
            self.out_pos += 1;
            if out_ch == 1 {
                frame[0] = 0.5 * (l + r);
            } else {
                frame[0] = l;
                frame[1] = r;
                frame[2..].iter_mut().for_each(|v| *v = 0.0);
            }
        }
    }

    fn produce(&mut self, pull: &mut impl FnMut(&mut [f32])) {
        let start = Instant::now();
        let mut peak_in = [0.0f32; 2];
        for i in 0..BLOCK {
            pull(&mut self.frame);
            for (c, &v) in self.frame.iter().enumerate() {
                self.input[c][i] = v;
                let m = &mut peak_in[c.min(1)];
                *m = m.max(v.abs());
            }
        }

        if let Some(p) = self.slot.try_take() {
            if p.fs != self.fs || p.in_ch != self.in_ch {
                self.slot.retire(p);
            } else {
                if let Some(old) = self.previous.take() {
                    self.slot.retire(old);
                }
                self.previous = self.current.replace(p);
                self.fade_pos = 0;
            }
        }

        let mut refs: [&[f32]; 16] = [&[]; 16];
        let n_in = self.in_ch.min(16);
        for (c, r) in refs.iter_mut().enumerate().take(n_in) {
            *r = &self.input[c];
        }
        let inputs = &refs[..n_in];

        let [ol, or] = &mut self.out;
        match &mut self.current {
            Some(p) => p.process_block(inputs, ol, or),
            None => {
                ol.iter_mut().for_each(|v| *v = 0.0);
                or.iter_mut().for_each(|v| *v = 0.0);
            }
        }

        if let Some(prev) = &mut self.previous {
            let [pl, pr] = &mut self.prev_out;
            prev.process_block(inputs, pl, pr);
            for i in 0..BLOCK {
                let t = ((self.fade_pos + i) as f32 / FADE as f32).min(1.0);
                let a = (t * std::f32::consts::FRAC_PI_2).sin();
                let b = (t * std::f32::consts::FRAC_PI_2).cos();
                ol[i] = ol[i] * a + pl[i] * b;
                or[i] = or[i] * a + pr[i] * b;
            }
            self.fade_pos += BLOCK;
            if self.fade_pos >= FADE {
                if let Some(old) = self.previous.take() {
                    self.slot.retire(old);
                }
            }
        }

        let mut peak_out = [0.0f32; 2];
        for i in 0..BLOCK {
            peak_out[0] = peak_out[0].max(ol[i].abs());
            peak_out[1] = peak_out[1].max(or[i].abs());
            self.meters.tap.push(0.5 * (ol[i] + or[i]));
        }
        if self.in_ch == 1 {
            peak_in[1] = peak_in[0];
        }
        for c in 0..2 {
            store_max(&self.meters.in_peak[c], peak_in[c]);
            store_max(&self.meters.out_peak[c], peak_out[c]);
        }
        let used = start.elapsed().as_secs_f32() / (BLOCK as f32 / self.fs);
        let prev = self.meters.load();
        self.meters.load.store((prev * 0.95 + used * 0.05).to_bits(), Ordering::Relaxed);
    }
}
