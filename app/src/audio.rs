//! Audio I/O via WASAPI (cpal).
//!
//! Routing for system-wide processing: Windows plays into a virtual device (e.g. VB-Audio "CABLE Input"),
//! Aether captures it ("CABLE Output", or WASAPI loopback of any render device), processes it and plays
//! the result on the real headphones/speakers. The two device clocks are bridged by a ring buffer and a
//! drift-compensating resampler.

use crate::state::App;
use aether_engine::{Runtime, BLOCK};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig};
use rtrb::{Consumer, Producer, RingBuffer};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const LOOPBACK_PREFIX: &str = "Loopback: ";
const TARGET_FILL_MS: f64 = 15.0;

#[derive(Clone, Debug, Default, Serialize)]
pub struct DeviceInfo {
    pub name: String,
    pub channels: u16,
    pub sample_rate: u32,
    pub loopback: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct DeviceList {
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
    pub default_output: Option<String>,
    pub virtual_cable: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct AudioStatus {
    pub running: bool,
    pub error: Option<String>,
    pub warning: Option<String>,
    pub input: Option<String>,
    pub output: Option<String>,
    pub sample_rate: u32,
    pub in_channels: u16,
    pub out_channels: u16,
    pub latency_ms: f32,
}

enum AudioCmd {
    Restart,
    Stop,
}

#[derive(Clone)]
pub struct AudioHandle {
    tx: Sender<AudioCmd>,
}

impl AudioHandle {
    pub fn restart(&self) {
        let _ = self.tx.send(AudioCmd::Restart);
    }

    pub fn stop(&self) {
        let _ = self.tx.send(AudioCmd::Stop);
    }
}

struct Running {
    _input: Option<Stream>,
    _output: Stream,
    output_name: String,
    input_name: Option<String>,
}

fn is_virtual(name: &str) -> bool {
    let n = name.to_lowercase();
    ["cable", "vb-audio", "voicemeeter", "virtual"].iter().any(|k| n.contains(k))
}

fn name_of(d: &Device) -> String {
    d.name().unwrap_or_else(|_| "Unknown device".into())
}

pub fn list_devices() -> DeviceList {
    let host = cpal::default_host();
    let mut list = DeviceList { default_output: host.default_output_device().map(|d| name_of(&d)), ..Default::default() };
    if let Ok(devs) = host.input_devices() {
        for d in devs {
            if let Ok(c) = d.default_input_config() {
                list.inputs.push(DeviceInfo { name: name_of(&d), channels: c.channels(), sample_rate: c.sample_rate().0, loopback: false });
            }
        }
    }
    if let Ok(devs) = host.output_devices() {
        for d in devs {
            if let Ok(c) = d.default_output_config() {
                let info = DeviceInfo { name: name_of(&d), channels: c.channels(), sample_rate: c.sample_rate().0, loopback: false };
                list.inputs.push(DeviceInfo { name: format!("{LOOPBACK_PREFIX}{}", info.name), loopback: true, ..info.clone() });
                list.outputs.push(info);
            }
        }
    }
    list.virtual_cable = list.inputs.iter().any(|d| !d.loopback && d.name.to_lowercase().contains("cable output"));
    list
}

/// Resolves the configured (or automatic) device pair: (output, input, input-is-loopback).
fn resolve(app: &App) -> anyhow::Result<(Device, Option<(Device, String)>)> {
    let cfg = app.config();
    let host = cpal::default_host();
    let outputs: Vec<Device> = host.output_devices()?.collect();
    let output = match &cfg.output_device {
        Some(name) => outputs.iter().find(|d| name_of(d) == *name).cloned().ok_or_else(|| anyhow::anyhow!("Output device '{name}' not found"))?,
        None => {
            let def = host.default_output_device().ok_or_else(|| anyhow::anyhow!("No output device"))?;
            if is_virtual(&name_of(&def)) {
                // Windows is routed into the virtual cable; play the result on the first real device.
                outputs.iter().find(|d| !is_virtual(&name_of(d))).cloned().unwrap_or(def)
            } else {
                def
            }
        }
    };
    let out_name = name_of(&output);

    let input = match &cfg.input_device {
        Some(name) => {
            if let Some(target) = name.strip_prefix(LOOPBACK_PREFIX) {
                let d = outputs.iter().find(|d| name_of(d) == target).cloned().ok_or_else(|| anyhow::anyhow!("Loopback device '{target}' not found"))?;
                Some((d, name.clone()))
            } else {
                let d = host.input_devices()?.find(|d| name_of(d) == *name).ok_or_else(|| anyhow::anyhow!("Input device '{name}' not found"))?;
                Some((d, name.clone()))
            }
        }
        None => {
            if let Some(d) = host.input_devices()?.find(|d| name_of(d).to_lowercase().contains("cable output")) {
                let n = name_of(&d);
                Some((d, n))
            } else if let Some(def) = host.default_output_device().filter(|d| is_virtual(&name_of(d)) && name_of(d) != out_name) {
                let n = format!("{LOOPBACK_PREFIX}{}", name_of(&def));
                Some((def, n))
            } else {
                None
            }
        }
    };
    if let Some((_, n)) = &input {
        if n.strip_prefix(LOOPBACK_PREFIX) == Some(out_name.as_str()) {
            anyhow::bail!("Capturing '{out_name}' while also playing to it would create a feedback loop. Choose a different output.");
        }
    }
    Ok((output, input))
}

/// Linear-interpolating resampler that also trims clock drift by steering the ring-buffer fill level.
struct Bridge {
    cons: Consumer<f32>,
    ch: usize,
    base: f64,
    ratio: f64,
    pos: f64,
    prev: Vec<f32>,
    cur: Vec<f32>,
    primed: bool,
    target: f64,
    counter: usize,
}

impl Bridge {
    fn pull(&mut self, out: &mut [f32]) {
        let ch = self.ch;
        if !self.primed {
            if self.cons.slots() as f64 >= self.target * ch as f64 {
                self.primed = true;
            } else {
                out.iter_mut().for_each(|v| *v = 0.0);
                return;
            }
        }
        while self.pos >= 1.0 {
            if self.cons.slots() < ch {
                self.primed = false;
                out.iter_mut().for_each(|v| *v = 0.0);
                return;
            }
            std::mem::swap(&mut self.prev, &mut self.cur);
            for c in 0..ch {
                self.cur[c] = self.cons.pop().unwrap_or(0.0);
            }
            self.pos -= 1.0;
        }
        let t = self.pos as f32;
        for (c, o) in out.iter_mut().enumerate() {
            *o = if c < ch { self.prev[c] + (self.cur[c] - self.prev[c]) * t } else { 0.0 };
        }
        self.pos += self.ratio;

        self.counter += 1;
        if self.counter >= 512 {
            self.counter = 0;
            let fill = self.cons.slots() as f64 / ch as f64;
            let err = ((fill - self.target) / self.target).clamp(-1.0, 1.0);
            self.ratio = self.base * (1.0 + err * 0.004);
        }
    }
}

fn build_input<T>(dev: &Device, cfg: &StreamConfig, mut prod: Producer<f32>, failed: Arc<AtomicBool>) -> anyhow::Result<Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let stream = dev.build_input_stream(
        cfg,
        move |data: &[T], _| {
            if prod.slots() >= data.len() {
                for &s in data {
                    let _ = prod.push(f32::from_sample(s));
                }
            }
        },
        move |e| {
            eprintln!("input stream error: {e}");
            failed.store(true, Ordering::Relaxed);
        },
        None,
    )?;
    Ok(stream)
}

fn build_output<T>(dev: &Device, cfg: &StreamConfig, mut runtime: Runtime, mut bridge: Option<Bridge>, failed: Arc<AtomicBool>) -> anyhow::Result<Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let out_ch = cfg.channels as usize;
    let mut scratch: Vec<f32> = vec![0.0; 16384 * out_ch];
    let stream = dev.build_output_stream(
        cfg,
        move |data: &mut [T], _| {
            if scratch.len() < data.len() {
                scratch.resize(data.len(), 0.0);
            }
            let buf = &mut scratch[..data.len()];
            runtime.render(buf, out_ch, |frame| match &mut bridge {
                Some(b) => b.pull(frame),
                None => frame.iter_mut().for_each(|v| *v = 0.0),
            });
            for (d, &s) in data.iter_mut().zip(buf.iter()) {
                *d = T::from_sample(s);
            }
        },
        move |e| {
            eprintln!("output stream error: {e}");
            failed.store(true, Ordering::Relaxed);
        },
        None,
    )?;
    Ok(stream)
}

fn start(app: &Arc<App>, failed: &Arc<AtomicBool>) -> anyhow::Result<(Running, AudioStatus)> {
    let (output, input) = resolve(app)?;
    let out_sc = output.default_output_config()?;
    let fs = out_sc.sample_rate().0;
    let out_cfg: StreamConfig = out_sc.config();

    let mut status = AudioStatus { output: Some(name_of(&output)), sample_rate: fs, out_channels: out_cfg.channels, ..Default::default() };
    let mut in_stream = None;
    let mut bridge = None;
    let mut in_ch = 2usize;
    let mut input_name = None;

    if let Some((dev, name)) = input {
        let loopback = name.starts_with(LOOPBACK_PREFIX);
        let in_sc = if loopback { dev.default_output_config()? } else { dev.default_input_config()? };
        let in_cfg: StreamConfig = in_sc.config();
        in_ch = in_cfg.channels as usize;
        let in_fs = in_cfg.sample_rate.0 as f64;
        let (prod, cons) = RingBuffer::<f32>::new((in_fs * 0.5) as usize * in_ch);
        let stream = match in_sc.sample_format() {
            SampleFormat::F32 => build_input::<f32>(&dev, &in_cfg, prod, failed.clone())?,
            SampleFormat::I16 => build_input::<i16>(&dev, &in_cfg, prod, failed.clone())?,
            SampleFormat::I32 => build_input::<i32>(&dev, &in_cfg, prod, failed.clone())?,
            SampleFormat::U16 => build_input::<u16>(&dev, &in_cfg, prod, failed.clone())?,
            f => anyhow::bail!("Unsupported input sample format {f:?}"),
        };
        let base = in_fs / fs as f64;
        bridge = Some(Bridge {
            cons,
            ch: in_ch,
            base,
            ratio: base,
            pos: 1.0,
            prev: vec![0.0; in_ch],
            cur: vec![0.0; in_ch],
            primed: false,
            target: in_fs * TARGET_FILL_MS / 1000.0,
            counter: 0,
        });
        status.input = Some(name.clone());
        status.in_channels = in_ch as u16;
        if in_fs as u32 != fs {
            status.warning = Some(format!("Input runs at {} Hz and output at {} Hz; resampling. Set both to the same rate in Windows Sound settings for best quality.", in_fs, fs));
        }
        input_name = Some(name);
        in_stream = Some(stream);
    } else {
        status.warning = Some("No system-audio source found. Install a virtual audio cable (VB-Audio CABLE) and set it as the Windows default output, or pick a source in Settings. The spatial test sound still works.".into());
    }

    app.engine.set_format(fs as f32, in_ch);
    let runtime = Runtime::new(app.engine.slot.clone(), app.engine.meters.clone(), fs as f32, in_ch);
    let out_stream = match out_sc.sample_format() {
        SampleFormat::F32 => build_output::<f32>(&output, &out_cfg, runtime, bridge, failed.clone())?,
        SampleFormat::I16 => build_output::<i16>(&output, &out_cfg, runtime, bridge, failed.clone())?,
        SampleFormat::I32 => build_output::<i32>(&output, &out_cfg, runtime, bridge, failed.clone())?,
        SampleFormat::U16 => build_output::<u16>(&output, &out_cfg, runtime, bridge, failed.clone())?,
        f => anyhow::bail!("Unsupported output sample format {f:?}"),
    };
    if let Some(s) = &in_stream {
        s.play()?;
    }
    out_stream.play()?;

    status.running = true;
    status.latency_ms = (TARGET_FILL_MS + BLOCK as f64 * 1000.0 / fs as f64 + 20.0) as f32;
    Ok((Running { _input: in_stream, _output: out_stream, output_name: name_of(&output), input_name }, status))
}

fn restart(app: &Arc<App>, running: &mut Option<Running>, failed: &Arc<AtomicBool>) {
    *running = None; // release devices before reopening
    failed.store(false, Ordering::Relaxed);
    *app.devices.lock().unwrap() = list_devices();
    match start(app, failed) {
        Ok((r, status)) => {
            *running = Some(r);
            *app.status.lock().unwrap() = status;
        }
        Err(e) => {
            *app.status.lock().unwrap() = AudioStatus { error: Some(e.to_string()), ..Default::default() };
        }
    }
    app.bump();
}

/// Has the automatic routing target changed (e.g. headphones plugged in, default device switched)?
fn routing_changed(app: &App, running: &Option<Running>) -> bool {
    let cfg = app.config();
    if cfg.input_device.is_some() && cfg.output_device.is_some() {
        return false;
    }
    match (running, resolve(app)) {
        (Some(r), Ok((out, inp))) => name_of(&out) != r.output_name || inp.map(|(_, n)| n) != r.input_name,
        (None, Ok(_)) => true,
        _ => false,
    }
}

pub fn spawn(app: Arc<App>) -> AudioHandle {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("aether-audio".into())
        .spawn(move || {
            let failed = Arc::new(AtomicBool::new(false));
            let mut running = None;
            let mut last_check = Instant::now();
            let mut retry_at: Option<Instant> = None;
            restart(&app, &mut running, &failed);
            loop {
                match rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(AudioCmd::Restart) => {
                        retry_at = None;
                        restart(&app, &mut running, &failed);
                    }
                    Ok(AudioCmd::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {
                        if failed.load(Ordering::Relaxed) || running.is_none() {
                            let at = *retry_at.get_or_insert_with(|| Instant::now() + Duration::from_secs(2));
                            if Instant::now() >= at {
                                retry_at = None;
                                restart(&app, &mut running, &failed);
                            }
                        } else if last_check.elapsed() > Duration::from_secs(3) {
                            last_check = Instant::now();
                            if routing_changed(&app, &running) {
                                restart(&app, &mut running, &failed);
                            }
                        }
                    }
                }
            }
            drop(running);
        })
        .expect("spawn audio thread");
    AudioHandle { tx }
}
