//! Application state: persisted configuration, profile management and the command API shared by
//! the web UI and the tray menu.

use crate::audio::{AudioHandle, AudioStatus, DeviceList};
use crate::engine_host::EngineHost;
use aether_engine::{presets, tuning, EngineSettings, HeadphoneTuning, OutputMode, Profile, Quality, TestSignal};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub mode: OutputMode,
    pub quality: Quality,
    pub profile_id: String,
    pub tuning_id: String,
    pub output_gain: f32,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub autostart: bool,
    /// Custom profiles plus user-modified copies of built-in profiles (same id, `builtin: true`).
    pub profiles: Vec<Profile>,
    pub custom_tunings: Vec<HeadphoneTuning>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: OutputMode::Headphones,
            quality: Quality::Best,
            profile_id: "natural".into(),
            tuning_id: "bypass".into(),
            output_gain: 0.0,
            input_device: None,
            output_device: None,
            autostart: false,
            profiles: vec![],
            custom_tunings: vec![],
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    SetEnabled { enabled: bool },
    SetMode { mode: OutputMode },
    SetQuality { quality: Quality },
    SelectProfile { id: String },
    UpdateProfile { profile: Profile },
    CreateProfile { name: String, from: Option<String> },
    DeleteProfile { id: String },
    ImportProfile { profile: Profile },
    SelectTuning { id: String },
    ImportTuning { name: String, text: String },
    DeleteTuning { id: String },
    SetDevices { input: Option<String>, output: Option<String> },
    RefreshDevices,
    SetTest { test: TestSignal },
    SetOutputGain { db: f32 },
    SetAutostart { enabled: bool },
    Quit,
}

#[derive(Serialize)]
pub struct Snapshot {
    pub version: u64,
    pub config: Config,
    pub profiles: Vec<Profile>,
    pub modified: Vec<String>,
    pub tunings: Vec<HeadphoneTuning>,
    pub test: TestSignal,
    pub status: AudioStatus,
    pub devices: DeviceList,
}

pub struct App {
    config: Mutex<Config>,
    test: Mutex<TestSignal>,
    pub status: Mutex<AudioStatus>,
    pub devices: Mutex<DeviceList>,
    path: PathBuf,
    version: AtomicU64,
    pub quit: AtomicBool,
    pub engine: EngineHost,
    audio: OnceLock<AudioHandle>,
}

fn clamp_profile(p: &mut Profile) {
    let s = &mut p.spatial;
    for v in [&mut s.upmix, &mut s.center, &mut s.height, &mut s.room_size, &mut s.reflections, &mut s.reverb, &mut s.damping] {
        *v = v.clamp(0.0, 1.0);
    }
    s.front_angle = s.front_angle.clamp(15.0, 60.0);
    s.surround_angle = s.surround_angle.clamp(80.0, 130.0);
    s.decay = s.decay.clamp(0.1, 3.0);
    s.distance = s.distance.clamp(0.8, 5.0);
    let t = &mut p.tone;
    for v in [&mut t.bass, &mut t.dialog, &mut t.detail, &mut t.leveler] {
        *v = v.clamp(0.0, 1.0);
    }
    t.eq.iter_mut().for_each(|g| *g = g.clamp(-12.0, 12.0));
    t.preamp = t.preamp.clamp(-24.0, 12.0);
    p.name = p.name.trim().chars().take(48).collect();
    if p.name.is_empty() {
        p.name = "Untitled".into();
    }
}

fn new_id(prefix: &str) -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{prefix}_{:x}", t.as_millis())
}

impl App {
    pub fn load() -> Arc<Self> {
        let dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("Aether");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.json");
        let config: Config = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let app = Arc::new(Self {
            config: Mutex::new(config),
            test: Mutex::new(TestSignal::default()),
            status: Mutex::new(AudioStatus::default()),
            devices: Mutex::new(DeviceList::default()),
            path,
            version: AtomicU64::new(1),
            quit: AtomicBool::new(false),
            engine: EngineHost::spawn(),
            audio: OnceLock::new(),
        });
        app.push_engine_settings();
        app
    }

    pub fn attach_audio(&self, audio: AudioHandle) {
        let _ = self.audio.set(audio);
    }

    pub fn config(&self) -> Config {
        self.config.lock().unwrap().clone()
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Relaxed)
    }

    pub fn bump(&self) {
        self.version.fetch_add(1, Ordering::Relaxed);
    }

    pub fn profiles_of(cfg: &Config) -> Vec<Profile> {
        let mut out: Vec<Profile> = presets::builtin_profiles()
            .into_iter()
            .map(|b| cfg.profiles.iter().find(|p| p.builtin && p.id == b.id).cloned().unwrap_or(b))
            .collect();
        out.extend(cfg.profiles.iter().filter(|p| !p.builtin).cloned());
        out
    }

    pub fn tunings_of(cfg: &Config) -> Vec<HeadphoneTuning> {
        let mut t = tuning::generic_tunings();
        t.extend(cfg.custom_tunings.iter().cloned());
        t
    }

    pub fn active_profile(cfg: &Config) -> Profile {
        let all = Self::profiles_of(cfg);
        all.iter().find(|p| p.id == cfg.profile_id).cloned().unwrap_or_else(|| all[0].clone())
    }

    fn engine_settings(&self) -> EngineSettings {
        let cfg = self.config.lock().unwrap();
        let tuning = Self::tunings_of(&cfg).into_iter().find(|t| t.id == cfg.tuning_id).unwrap_or_default();
        EngineSettings {
            enabled: cfg.enabled,
            mode: cfg.mode,
            quality: cfg.quality,
            profile: Self::active_profile(&cfg),
            tuning,
            output_gain: cfg.output_gain,
            test: *self.test.lock().unwrap(),
        }
    }

    fn push_engine_settings(&self) {
        self.engine.update(self.engine_settings());
    }

    fn save(&self) {
        let json = serde_json::to_string_pretty(&*self.config.lock().unwrap()).unwrap_or_default();
        let tmp = self.path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let config = self.config();
        let builtin_ids: Vec<String> = presets::builtin_profiles().into_iter().map(|p| p.id).collect();
        Snapshot {
            version: self.version(),
            profiles: Self::profiles_of(&config),
            modified: config.profiles.iter().filter(|p| p.builtin && builtin_ids.contains(&p.id)).map(|p| p.id.clone()).collect(),
            tunings: Self::tunings_of(&config),
            test: *self.test.lock().unwrap(),
            status: self.status.lock().unwrap().clone(),
            devices: self.devices.lock().unwrap().clone(),
            config,
        }
    }

    /// Stops the audio streams so devices are released before the process exits.
    pub fn shutdown(&self) {
        if let Some(a) = self.audio.get() {
            a.stop();
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }

    fn restart_audio(&self) {
        if let Some(a) = self.audio.get() {
            a.restart();
        }
    }

    /// Applies a command from the UI or tray. Returns an error message for invalid requests.
    pub fn apply(&self, cmd: Command) -> Result<(), String> {
        let mut persist = true;
        let mut restart = false;
        {
            let mut cfg = self.config.lock().unwrap();
            match cmd {
                Command::SetEnabled { enabled } => cfg.enabled = enabled,
                Command::SetMode { mode } => cfg.mode = mode,
                Command::SetQuality { quality } => cfg.quality = quality,
                Command::SelectProfile { id } => {
                    if !Self::profiles_of(&cfg).iter().any(|p| p.id == id) {
                        return Err(format!("unknown profile '{id}'"));
                    }
                    cfg.profile_id = id;
                }
                Command::UpdateProfile { mut profile } => {
                    clamp_profile(&mut profile);
                    let is_builtin = presets::builtin_profiles().iter().any(|b| b.id == profile.id);
                    profile.builtin = is_builtin;
                    if let Some(p) = cfg.profiles.iter_mut().find(|p| p.id == profile.id) {
                        *p = profile;
                    } else if is_builtin {
                        cfg.profiles.push(profile);
                    } else {
                        return Err(format!("unknown profile '{}'", profile.id));
                    }
                }
                Command::CreateProfile { name, from } => {
                    let base_id = from.unwrap_or_else(|| cfg.profile_id.clone());
                    let mut p = Self::profiles_of(&cfg).into_iter().find(|p| p.id == base_id).unwrap_or_default();
                    p.id = new_id("custom");
                    p.name = name;
                    p.builtin = false;
                    p.category = "Custom".into();
                    clamp_profile(&mut p);
                    cfg.profile_id = p.id.clone();
                    cfg.profiles.push(p);
                }
                Command::DeleteProfile { id } => {
                    cfg.profiles.retain(|p| p.id != id);
                    if !Self::profiles_of(&cfg).iter().any(|p| p.id == cfg.profile_id) {
                        cfg.profile_id = "natural".into();
                    }
                }
                Command::ImportProfile { mut profile } => {
                    profile.id = new_id("custom");
                    profile.builtin = false;
                    profile.category = "Custom".into();
                    clamp_profile(&mut profile);
                    cfg.profile_id = profile.id.clone();
                    cfg.profiles.push(profile);
                }
                Command::SelectTuning { id } => {
                    if !Self::tunings_of(&cfg).iter().any(|t| t.id == id) {
                        return Err(format!("unknown tuning '{id}'"));
                    }
                    cfg.tuning_id = id;
                }
                Command::ImportTuning { name, text } => {
                    let (preamp, filters) = tuning::parse_parametric(&text)?;
                    let name: String = name.trim().chars().take(64).collect();
                    let t = HeadphoneTuning { id: new_id("tuning"), name: if name.is_empty() { "Imported".into() } else { name }, preamp, filters };
                    cfg.tuning_id = t.id.clone();
                    cfg.custom_tunings.push(t);
                }
                Command::DeleteTuning { id } => {
                    cfg.custom_tunings.retain(|t| t.id != id);
                    if cfg.tuning_id == id {
                        cfg.tuning_id = "bypass".into();
                    }
                }
                Command::SetDevices { input, output } => {
                    cfg.input_device = input.filter(|s| !s.is_empty());
                    cfg.output_device = output.filter(|s| !s.is_empty());
                    restart = true;
                }
                Command::RefreshDevices => {
                    persist = false;
                    restart = true;
                }
                Command::SetTest { test } => {
                    let mut t = test;
                    t.azimuth = t.azimuth.clamp(-180.0, 180.0);
                    t.elevation = t.elevation.clamp(0.0, 90.0);
                    *self.test.lock().unwrap() = t;
                    persist = false;
                }
                Command::SetOutputGain { db } => cfg.output_gain = db.clamp(-24.0, 12.0),
                Command::SetAutostart { enabled } => {
                    crate::platform::set_autostart(enabled).map_err(|e| e.to_string())?;
                    cfg.autostart = enabled;
                }
                Command::Quit => {
                    self.quit.store(true, Ordering::Relaxed);
                    persist = false;
                }
            }
        }
        if persist {
            self.save();
        }
        self.push_engine_settings();
        if restart {
            self.restart_audio();
        }
        self.bump();
        Ok(())
    }
}
