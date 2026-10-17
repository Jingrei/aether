//! Built-in sound profiles. The line-up mirrors the categories found in commercial spatial
//! audio apps (Natural / Spacious, Gaming and Movies variants); the parameter values are our own.

use crate::params::{Profile, SpatialParams, ToneParams};

fn profile(
    id: &str,
    name: &str,
    category: &str,
    description: &str,
    spatial: SpatialParams,
    tone: ToneParams,
) -> Profile {
    Profile {
        id: id.into(),
        name: name.into(),
        category: category.into(),
        description: description.into(),
        builtin: true,
        spatial,
        tone,
    }
}

pub fn builtin_profiles() -> Vec<Profile> {
    let s = SpatialParams::default();
    let t = ToneParams::default();
    vec![
        profile(
            "natural",
            "Natural",
            "General",
            "A small, well-damped room. Speakers sit just outside your head with minimal coloration.",
            SpatialParams { upmix: 0.55, center: 0.55, height: 0.3, room_size: 0.3, reflections: 0.35, reverb: 0.1, decay: 0.3, distance: 1.8, damping: 0.55, ..s.clone() },
            ToneParams { bass: 0.15, dialog: 0.15, detail: 0.15, ..t.clone() },
        ),
        profile(
            "spacious",
            "Spacious",
            "General",
            "A larger room with wider surrounds and a raised height layer for an open, enveloping sound.",
            SpatialParams { upmix: 0.85, center: 0.6, front_angle: 34.0, surround_angle: 115.0, height: 0.55, room_size: 0.7, reflections: 0.5, reverb: 0.24, decay: 0.55, distance: 2.6, damping: 0.45, ..s.clone() },
            ToneParams { bass: 0.25, dialog: 0.2, detail: 0.2, ..t.clone() },
        ),
        profile(
            "gaming_neutral",
            "Gaming · Neutral",
            "Gaming",
            "Dry and precise. Little room sound so positional cues stay sharp.",
            SpatialParams { upmix: 0.5, center: 0.4, height: 0.35, room_size: 0.2, reflections: 0.2, reverb: 0.04, decay: 0.2, distance: 1.5, damping: 0.6, ..s.clone() },
            ToneParams { bass: 0.1, dialog: 0.1, detail: 0.45, ..t.clone() },
        ),
        profile(
            "gaming_balanced",
            "Gaming · Balanced",
            "Gaming",
            "Positional accuracy with some room for immersion, and a lift for footsteps.",
            SpatialParams { upmix: 0.65, center: 0.45, height: 0.4, room_size: 0.4, reflections: 0.35, reverb: 0.1, decay: 0.3, distance: 1.9, damping: 0.5, ..s.clone() },
            ToneParams { bass: 0.25, dialog: 0.15, detail: 0.55, ..t.clone() },
        ),
        profile(
            "gaming_spacious",
            "Gaming · Spacious",
            "Gaming",
            "A big arena-like stage with wide surrounds.",
            SpatialParams { upmix: 0.8, center: 0.45, front_angle: 36.0, surround_angle: 115.0, height: 0.5, room_size: 0.65, reflections: 0.45, reverb: 0.18, decay: 0.45, distance: 2.4, damping: 0.45, ..s.clone() },
            ToneParams { bass: 0.35, dialog: 0.15, detail: 0.5, ..t.clone() },
        ),
        profile(
            "gaming_spacious2",
            "Gaming · Spacious 2",
            "Gaming",
            "The widest stage: speakers pushed out and back, with strong envelopment.",
            SpatialParams { upmix: 0.9, center: 0.4, front_angle: 42.0, surround_angle: 120.0, height: 0.6, room_size: 0.8, reflections: 0.5, reverb: 0.2, decay: 0.5, distance: 2.8, damping: 0.4, ..s.clone() },
            ToneParams { bass: 0.4, dialog: 0.1, detail: 0.5, ..t.clone() },
        ),
        profile(
            "movies_balanced",
            "Movies · Balanced",
            "Movies",
            "A home-theater room with clear dialog and solid low end.",
            SpatialParams { upmix: 0.8, center: 0.75, height: 0.45, room_size: 0.5, reflections: 0.42, reverb: 0.2, decay: 0.5, distance: 2.4, damping: 0.5, ..s.clone() },
            ToneParams { bass: 0.45, dialog: 0.5, detail: 0.2, ..t.clone() },
        ),
        profile(
            "movies_spacious",
            "Movies · Spacious",
            "Movies",
            "A cinema-sized space with a tall height layer and long decay.",
            SpatialParams { upmix: 0.95, center: 0.75, front_angle: 34.0, surround_angle: 115.0, height: 0.7, room_size: 0.85, reflections: 0.5, reverb: 0.3, decay: 0.8, distance: 3.0, damping: 0.45, ..s.clone() },
            ToneParams { bass: 0.55, dialog: 0.45, detail: 0.2, ..t.clone() },
        ),
        profile(
            "music",
            "Music",
            "Content",
            "Keeps the stereo image intact and places it on speakers in a pleasant room.",
            SpatialParams { upmix: 0.3, center: 0.25, front_angle: 30.0, height: 0.2, room_size: 0.45, reflections: 0.4, reverb: 0.14, decay: 0.45, distance: 2.2, damping: 0.5, ..s.clone() },
            ToneParams { bass: 0.3, dialog: 0.0, detail: 0.25, eq: [1.5, 1.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0], ..t.clone() },
        ),
        profile(
            "voice",
            "Voice",
            "Content",
            "For podcasts and calls: dry, centered and leveled.",
            SpatialParams { upmix: 0.0, center: 0.8, height: 0.0, room_size: 0.2, reflections: 0.25, reverb: 0.03, decay: 0.2, distance: 1.6, damping: 0.6, ..s.clone() },
            ToneParams { bass: 0.0, dialog: 0.8, detail: 0.1, leveler: 0.6, eq: [-6.0, -3.0, -1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0], ..t.clone() },
        ),
        profile(
            "night",
            "Night",
            "Content",
            "Compresses loud scenes and lifts dialog so you can keep the volume low.",
            SpatialParams { upmix: 0.7, center: 0.8, height: 0.3, room_size: 0.4, reflections: 0.35, reverb: 0.12, decay: 0.4, distance: 2.0, damping: 0.55, ..s },
            ToneParams { bass: 0.05, dialog: 0.65, detail: 0.15, leveler: 0.9, ..t },
        ),
    ]
}
