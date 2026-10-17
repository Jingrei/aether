//! Headphone tuning: generic correction curves and import of AutoEQ / Equalizer APO
//! "ParametricEQ.txt" files (an open, community-measured database of headphone corrections).

use crate::params::{EqFilter, FilterKind, HeadphoneTuning};

fn f(kind: FilterKind, freq: f32, q: f32, gain: f32) -> EqFilter {
    EqFilter { kind, freq, q, gain }
}

pub fn bypass() -> HeadphoneTuning {
    HeadphoneTuning { id: "bypass".into(), name: "Bypass".into(), preamp: 0.0, filters: vec![] }
}

/// Generic presets. These are gentle, broad corrections, not per-model measurements.
pub fn generic_tunings() -> Vec<HeadphoneTuning> {
    use FilterKind::*;
    vec![
        bypass(),
        HeadphoneTuning {
            id: "generic_over_ear".into(),
            name: "Generic Over-Ear".into(),
            preamp: -3.0,
            filters: vec![f(LowShelf, 105.0, 0.7, 2.5), f(Peak, 3000.0, 1.2, 1.5), f(Peak, 6500.0, 3.0, -2.0), f(HighShelf, 10000.0, 0.7, -0.5)],
        },
        HeadphoneTuning {
            id: "generic_on_ear".into(),
            name: "Generic On-Ear".into(),
            preamp: -3.5,
            filters: vec![f(LowShelf, 105.0, 0.7, 3.5), f(Peak, 2500.0, 1.0, 2.0), f(Peak, 7000.0, 3.0, -2.5)],
        },
        HeadphoneTuning {
            id: "generic_in_ear".into(),
            name: "Generic In-Ear".into(),
            preamp: -2.5,
            filters: vec![f(LowShelf, 105.0, 0.7, 1.0), f(Peak, 2800.0, 1.0, 2.5), f(Peak, 8500.0, 2.5, -2.0)],
        },
        HeadphoneTuning {
            id: "generic_earbuds".into(),
            name: "Generic Earbuds".into(),
            preamp: -4.0,
            filters: vec![f(LowShelf, 105.0, 0.7, 4.0), f(Peak, 3200.0, 1.0, 1.5), f(HighShelf, 9000.0, 0.7, 1.5)],
        },
    ]
}

/// Parses Equalizer APO / AutoEQ parametric text, e.g.
/// `Preamp: -6.2 dB` and `Filter 1: ON PK Fc 105 Hz Gain -2.4 dB Q 0.70`.
pub fn parse_parametric(text: &str) -> Result<(f32, Vec<EqFilter>), String> {
    let mut preamp = 0.0;
    let mut filters = Vec::new();
    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("preamp:") {
            preamp = number_before(rest, "db").ok_or_else(|| format!("line {}: bad preamp", lineno + 1))?;
            continue;
        }
        if !lower.starts_with("filter") {
            continue;
        }
        let tokens: Vec<&str> = lower.split_whitespace().collect();
        if !tokens.contains(&"on") {
            continue;
        }
        let kind = tokens.iter().find_map(|t| match *t {
            "pk" | "peq" => Some(FilterKind::Peak),
            "ls" | "lsc" | "lsq" => Some(FilterKind::LowShelf),
            "hs" | "hsc" | "hsq" => Some(FilterKind::HighShelf),
            "lp" | "lpq" => Some(FilterKind::LowPass),
            "hp" | "hpq" => Some(FilterKind::HighPass),
            _ => None,
        });
        let Some(kind) = kind else { continue };
        let value_after = |key: &str| -> Option<f32> {
            let i = tokens.iter().position(|t| *t == key)?;
            tokens.get(i + 1)?.parse().ok()
        };
        let freq = value_after("fc").ok_or_else(|| format!("line {}: missing Fc", lineno + 1))?;
        let gain = value_after("gain").unwrap_or(0.0);
        let default_q = if matches!(kind, FilterKind::Peak) { 1.0 } else { 0.707 };
        let q = value_after("q").unwrap_or(default_q);
        filters.push(EqFilter { kind, freq, q, gain });
    }
    if filters.is_empty() {
        return Err("no filters found (expected lines like 'Filter 1: ON PK Fc 105 Hz Gain -2.4 dB Q 0.70')".into());
    }
    Ok((preamp, filters))
}

fn number_before(s: &str, unit: &str) -> Option<f32> {
    s.split(unit).next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_autoeq() {
        let text = "Preamp: -6.4 dB\nFilter 1: ON LSC Fc 105 Hz Gain 6.5 dB Q 0.70\nFilter 2: ON PK Fc 3378 Hz Gain -3.3 dB Q 2.29\nFilter 3: OFF PK Fc 100 Hz Gain 1 dB Q 1\nFilter 4: ON HSC Fc 10000 Hz Gain -1.2 dB Q 0.70";
        let (pre, f) = parse_parametric(text).unwrap();
        assert_eq!(pre, -6.4);
        assert_eq!(f.len(), 3);
        assert_eq!(f[0].kind, FilterKind::LowShelf);
        assert_eq!(f[1].freq, 3378.0);
        assert_eq!(f[1].q, 2.29);
        assert_eq!(f[2].gain, -1.2);
    }
}
