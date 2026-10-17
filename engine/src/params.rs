//! User-facing settings: output modes, quality tiers, spatial profiles and headphone tuning.

use serde::{Deserialize, Serialize};

/// Where the processed audio is going. Mirrors the Headphone / Speaker / Stereo-bypass split
/// used by commercial spatial renderers.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    /// Binaural rendering of a virtual 7.1.4 speaker room.
    Headphones,
    /// Binaural rendering followed by crosstalk cancellation for a pair of near-field speakers.
    Speakers,
    /// No virtualization; tone shaping only.
    Stereo,
}

/// Rendering quality / power tier.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    /// Direct HRTF paths only, short filters, no height layer. Lowest CPU.
    Battery,
    /// First-order room reflections.
    Balanced,
    /// Second-order reflections, height layer, longest filters.
    Best,
}

/// Parameters that shape the virtual room and speaker layout. All 0..1 unless noted.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SpatialParams {
    /// How much stereo content is spread into center / surround / height channels.
    pub upmix: f32,
    /// Strength of center (dialog) extraction from stereo.
    pub center: f32,
    /// Front speaker angle in degrees (each side of center).
    pub front_angle: f32,
    /// Side surround angle in degrees.
    pub surround_angle: f32,
    /// Level of the height layer.
    pub height: f32,
    /// Virtual room size.
    pub room_size: f32,
    /// Early reflection level.
    pub reflections: f32,
    /// Late reverb level.
    pub reverb: f32,
    /// Late reverb decay time (RT60) in seconds.
    pub decay: f32,
    /// Virtual speaker distance in metres.
    pub distance: f32,
    /// High-frequency absorption of the room surfaces.
    pub damping: f32,
}

impl Default for SpatialParams {
    fn default() -> Self {
        Self {
            upmix: 0.6,
            center: 0.6,
            front_angle: 30.0,
            surround_angle: 110.0,
            height: 0.35,
            room_size: 0.4,
            reflections: 0.4,
            reverb: 0.15,
            decay: 0.35,
            distance: 2.0,
            damping: 0.5,
        }
    }
}

pub const EQ_BANDS: [f32; 10] = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

/// Enhancement and EQ settings applied after spatial rendering.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ToneParams {
    /// Bass enhancement (low shelf + psychoacoustic harmonics).
    pub bass: f32,
    /// Dialog clarity (center / mid presence lift).
    pub dialog: f32,
    /// High-frequency detail, useful for footsteps and positional cues.
    pub detail: f32,
    /// Volume leveler amount (slow AGC).
    pub leveler: f32,
    /// 10-band graphic EQ gains in dB, at [`EQ_BANDS`].
    pub eq: [f32; 10],
    /// Gain applied before the limiter, in dB.
    pub preamp: f32,
}

impl Default for ToneParams {
    fn default() -> Self {
        Self { bass: 0.2, dialog: 0.2, detail: 0.2, leveler: 0.0, eq: [0.0; 10], preamp: 0.0 }
    }
}

/// A named, savable sound profile (what commercial apps call a "mode").
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub builtin: bool,
    pub spatial: SpatialParams,
    pub tone: ToneParams,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            id: "custom".into(),
            name: "Custom".into(),
            category: "Custom".into(),
            description: String::new(),
            builtin: false,
            spatial: SpatialParams::default(),
            tone: ToneParams::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FilterKind {
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct EqFilter {
    pub kind: FilterKind,
    pub freq: f32,
    pub q: f32,
    pub gain: f32,
}

/// Headphone frequency-response correction.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct HeadphoneTuning {
    pub id: String,
    pub name: String,
    pub preamp: f32,
    pub filters: Vec<EqFilter>,
}

impl Default for HeadphoneTuning {
    fn default() -> Self {
        crate::tuning::bypass()
    }
}

/// Positioned test sound used by the spatial editor.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TestSignal {
    pub enabled: bool,
    /// Degrees, positive = left, 0 = front.
    pub azimuth: f32,
    /// Degrees, 0 = ear level, 90 = overhead.
    pub elevation: f32,
}

impl Default for TestSignal {
    fn default() -> Self {
        Self { enabled: false, azimuth: 70.0, elevation: 0.0 }
    }
}

/// Everything the DSP needs to build a processor.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EngineSettings {
    pub enabled: bool,
    pub mode: OutputMode,
    pub quality: Quality,
    pub profile: Profile,
    pub tuning: HeadphoneTuning,
    /// Final output gain in dB.
    pub output_gain: f32,
    pub test: TestSignal,
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: OutputMode::Headphones,
            quality: Quality::Best,
            profile: crate::presets::builtin_profiles().remove(0),
            tuning: HeadphoneTuning::default(),
            output_gain: 0.0,
            test: TestSignal::default(),
        }
    }
}
