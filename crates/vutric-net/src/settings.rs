//! The user's receiving settings on disk: one JSON file per device (ADR-0007 part 4). A missing or damaged file
//! means the defaults, never an error the user has to fix; a damaged file is kept beside as `.bad` for support.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
pub use vutric_core::storage::Settings;

/// The settings file name inside the host's config folder.
pub const FILE: &str = "settings.json";

/// Reads the settings at `path`; defaults when the file is absent or cannot be read.
#[must_use]
pub fn load(path: &Path) -> Settings {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Settings::default();
    };
    if let Ok(s) = serde_json::from_str::<Settings>(&text) {
        s.migrate()
    } else {
        let mut bad = path.as_os_str().to_owned();
        bad.push(".bad");
        let _ = std::fs::rename(path, PathBuf::from(bad));
        crate::say(format!(
            "vutric-net: {} could not be read; using defaults",
            path.display()
        ));
        Settings::default()
    }
}

/// Writes the settings atomically (temp file, then rename), so a crash never leaves half a file.
pub fn save(path: &Path, s: &Settings) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(s).context("encode settings")?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("save {}", path.display()))
}

/// Where a desktop keeps its settings: `%APPDATA%\Vutric`, `~/Library/Application Support/Vutric`, or
/// `$XDG_CONFIG_HOME/vutric` (`~/.config/vutric`).
pub fn desktop_dir() -> Result<PathBuf> {
    let var = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(windows) {
        return var("APPDATA")
            .map(|p| p.join("Vutric"))
            .context("no APPDATA folder");
    }
    let home = var("HOME").context("no home folder")?;
    if cfg!(target_os = "macos") {
        return Ok(home.join("Library/Application Support/Vutric"));
    }
    Ok(var("XDG_CONFIG_HOME")
        .unwrap_or_else(|| home.join(".config"))
        .join("vutric"))
}

#[cfg(test)]
mod tests {
    use vutric_core::storage::{Accept, Conflict, Subfolder};

    use super::*;

    #[test]
    fn round_trip_damaged_and_missing() {
        let dir = std::env::temp_dir().join(format!("vutric-settings-{}", std::process::id()));
        let path = dir.join(FILE);
        assert_eq!(load(&path), Settings::default());

        let mut s = Settings::default();
        s.save.subfolder = Subfolder::DeviceDate;
        s.save.conflict = Conflict::Replace;
        s.accept = Accept::UpTo(50 << 20);
        save(&path, &s).unwrap();
        assert_eq!(load(&path), s);

        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load(&path), Settings::default());
        assert!(dir.join("settings.json.bad").exists());

        // An older file with fewer fields still loads, the rest default.
        std::fs::write(&path, r#"{"version":0,"save":{"split_by_type":true}}"#).unwrap();
        let old = load(&path);
        assert!(old.save.split_by_type);
        assert!(old.save.keep_tree);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
