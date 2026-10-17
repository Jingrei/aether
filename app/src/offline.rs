//! `aether render`: process an audio file offline (useful for A/B comparisons and testing).

use aether_engine::{presets, render_offline, tuning, EngineSettings, OutputMode, Quality};

pub const USAGE: &str = "\
Aether Sound

USAGE:
    aether                       Start the app (tray icon + control panel)
    aether --background          Start minimized to the tray
    aether devices               List audio devices
    aether render IN.wav OUT.wav [--profile ID] [--mode headphones|speakers|stereo]
                                 [--quality best|balanced|battery] [--tuning ID] [--bypass]
    aether profiles              List built-in profile and tuning ids
";

fn arg_value(args: &[String], key: &str) -> Option<String> {
    args.iter().position(|a| a == key).and_then(|i| args.get(i + 1)).cloned()
}

pub fn render(args: &[String]) -> anyhow::Result<()> {
    let (Some(inp), Some(outp)) = (args.first(), args.get(1)) else {
        anyhow::bail!("{USAGE}");
    };
    let mut settings = EngineSettings::default();
    if let Some(id) = arg_value(args, "--profile") {
        settings.profile = presets::builtin_profiles().into_iter().find(|p| p.id == id).ok_or_else(|| anyhow::anyhow!("unknown profile '{id}'"))?;
    }
    if let Some(m) = arg_value(args, "--mode") {
        settings.mode = match m.as_str() {
            "headphones" => OutputMode::Headphones,
            "speakers" => OutputMode::Speakers,
            "stereo" => OutputMode::Stereo,
            _ => anyhow::bail!("unknown mode '{m}'"),
        };
    }
    if let Some(q) = arg_value(args, "--quality") {
        settings.quality = match q.as_str() {
            "best" => Quality::Best,
            "balanced" => Quality::Balanced,
            "battery" => Quality::Battery,
            _ => anyhow::bail!("unknown quality '{q}'"),
        };
    }
    if let Some(id) = arg_value(args, "--tuning") {
        settings.tuning = tuning::generic_tunings().into_iter().find(|t| t.id == id).ok_or_else(|| anyhow::anyhow!("unknown tuning '{id}'"))?;
    }
    settings.enabled = !args.iter().any(|a| a == "--bypass");

    let mut reader = hound::WavReader::open(inp)?;
    let spec = reader.spec();
    let ch = spec.channels as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()?
        }
    };
    let frames = samples.len() / ch;
    let planar: Vec<Vec<f32>> = (0..ch).map(|c| (0..frames).map(|i| samples[i * ch + c]).collect()).collect();

    let start = std::time::Instant::now();
    let [l, r] = render_offline(&settings, spec.sample_rate as f32, &planar);
    let secs = frames as f64 / spec.sample_rate as f64;

    let out_spec = hound::WavSpec { channels: 2, sample_rate: spec.sample_rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(outp, out_spec)?;
    for i in 0..frames {
        w.write_sample(l[i])?;
        w.write_sample(r[i])?;
    }
    w.finalize()?;
    let el = start.elapsed().as_secs_f64();
    println!(
        "Rendered {:.1}s of {}-channel audio with '{}' ({:?}, {:?}) in {:.2}s ({:.0}x real time) -> {}",
        secs, ch, settings.profile.name, settings.mode, settings.quality, el, secs / el.max(1e-9), outp
    );
    Ok(())
}

pub fn list_profiles() {
    println!("Profiles:");
    for p in presets::builtin_profiles() {
        println!("  {:<18} {}", p.id, p.name);
    }
    println!("Headphone tunings:");
    for t in tuning::generic_tunings() {
        println!("  {:<18} {}", t.id, t.name);
    }
}

pub fn list_devices() {
    let d = crate::audio::list_devices();
    println!("Default output: {}", d.default_output.unwrap_or_default());
    println!("Virtual cable detected: {}", d.virtual_cable);
    println!("Outputs:");
    for o in d.outputs {
        println!("  {} ({} ch, {} Hz)", o.name, o.channels, o.sample_rate);
    }
    println!("Sources:");
    for i in d.inputs {
        println!("  {} ({} ch, {} Hz)", i.name, i.channels, i.sample_rate);
    }
}
