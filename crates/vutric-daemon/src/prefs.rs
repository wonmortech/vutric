//! Desktop-only preferences (`desktop.json` beside `settings.json`): how a fling looks on this screen and whether
//! the daemon starts with the session. Receiving rules live in the shared `Settings` (ADR-0007), not here.

use std::path::Path;

use serde::{Deserialize, Serialize};
use vutric_core::{EffectId, SendZone};

pub const FILE: &str = "desktop.json";

/// Shortest and longest effect travel the settings slider offers, seconds.
pub const TRAVEL_RANGE: (f32, f32) = (0.6, 2.0);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopPrefs {
    pub effect: EffectId,
    pub zone: SendZone,
    /// Portal open → card landed, seconds. Hold and dock scale with it.
    pub travel: f32,
    pub autostart: bool,
    /// Monitor to play effects on, by its OS name; `None` = the primary monitor.
    pub monitor: Option<String>,
}

impl Default for DesktopPrefs {
    fn default() -> Self {
        Self {
            effect: EffectId::Halqa,
            zone: SendZone::Right,
            travel: 1.05,
            autostart: true,
            monitor: None,
        }
    }
}

impl DesktopPrefs {
    /// Clamps values a hand-edited or older file might carry.
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        self.travel = if self.travel.is_finite() {
            self.travel.clamp(TRAVEL_RANGE.0, TRAVEL_RANGE.1)
        } else {
            Self::default().travel
        };
        self
    }

    /// The effect actually played: the OS "reduce motion" setting always wins (docs/effects.md).
    #[must_use]
    pub fn effect_for(&self, reduce_motion: bool) -> EffectId {
        if reduce_motion {
            EffectId::Oddiy
        } else {
            self.effect
        }
    }
}

/// Reads the preferences; defaults when the file is missing or damaged (a damaged file is kept as `.bad`).
#[must_use]
pub fn load(path: &Path) -> DesktopPrefs {
    let Ok(text) = std::fs::read_to_string(path) else {
        return DesktopPrefs::default();
    };
    if let Ok(p) = serde_json::from_str::<DesktopPrefs>(&text) {
        p.sanitized()
    } else {
        let mut bad = path.as_os_str().to_owned();
        bad.push(".bad");
        let _ = std::fs::rename(path, std::path::PathBuf::from(bad));
        DesktopPrefs::default()
    }
}

/// Writes atomically (temp file, then rename).
pub fn save(path: &Path, p: &DesktopPrefs) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    let text = serde_json::to_string_pretty(p).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vutric-prefs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join(FILE)
    }

    #[test]
    fn round_trip_and_defaults() {
        let path = temp("rt");
        assert_eq!(load(&path), DesktopPrefs::default());
        let p = DesktopPrefs {
            effect: EffectId::Kometa,
            zone: SendZone::Top,
            travel: 1.6,
            autostart: false,
            monitor: Some(r"\\.\DISPLAY2".into()),
        };
        save(&path, &p).unwrap();
        assert_eq!(load(&path), p);
    }

    #[test]
    fn damaged_file_falls_back_and_is_kept() {
        let path = temp("bad");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load(&path), DesktopPrefs::default());
        let mut bad = path.as_os_str().to_owned();
        bad.push(".bad");
        assert!(std::path::PathBuf::from(bad).exists());
    }

    #[test]
    fn sanitizes_and_respects_reduce_motion() {
        let p: DesktopPrefs = serde_json::from_str(r#"{"travel": 99}"#).unwrap();
        assert_eq!(p.sanitized().travel, TRAVEL_RANGE.1);
        let p = DesktopPrefs::default();
        assert_eq!(p.effect_for(false), EffectId::Halqa);
        assert_eq!(p.effect_for(true), EffectId::Oddiy);
    }
}
