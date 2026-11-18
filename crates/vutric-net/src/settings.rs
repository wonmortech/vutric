//! The user's receiving settings on disk: one JSON file per device (ADR-0007 part 4). A missing or damaged file
//! means the defaults, never an error the user has to fix; a damaged file is kept beside as `.bad` for support.
//! A file written by a newer Vutric keeps what this version does not know when it is saved again (T9, ADR-0063).

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
    let mut value = serde_json::to_value(s).context("encode settings")?;
    if let Some(on_disk) = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .filter(|v| version_of(v) > vutric_core::storage::SETTINGS_VERSION)
    {
        value = keep_newer(on_disk, value);
    }
    let text = serde_json::to_string_pretty(&value).context("encode settings")?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("save {}", path.display()))
}

fn version_of(v: &serde_json::Value) -> u32 {
    v.get("version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0)
}

/// Our values over a newer file: objects merge key by key, so fields only the newer version knows survive and
/// its version number stays; for everything we know, ours wins.
fn keep_newer(newer: serde_json::Value, ours: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match (newer, ours) {
        (Value::Object(mut n), Value::Object(o)) => {
            for (k, v) in o {
                if k == "version" {
                    continue;
                }
                let merged = match n.remove(&k) {
                    Some(old) => keep_newer(old, v),
                    None => v,
                };
                n.insert(k, merged);
            }
            Value::Object(n)
        }
        (_, ours) => ours,
    }
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

    #[test]
    fn a_newer_file_keeps_what_we_do_not_know() {
        let dir = std::env::temp_dir().join(format!("vutric-settings-new-{}", std::process::id()));
        let path = dir.join(FILE);
        std::fs::create_dir_all(&dir).unwrap();
        let newer = r#"{"version":9,"future_top":[1,2],"save":{"split_by_type":true,"future_inner":"x"},"accept":{"mode":"trusted"}}"#;
        std::fs::write(&path, newer).unwrap();
        let mut s = load(&path);
        assert!(s.save.split_by_type, "what we know is read");
        assert_eq!(s.accept, Accept::Trusted);
        s.parts_keep_days = 3;
        s.save.split_by_type = false;
        save(&path, &s.migrate()).unwrap();

        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["version"], 9, "the newer version number stays");
        assert_eq!(v["future_top"], serde_json::json!([1, 2]));
        assert_eq!(v["save"]["future_inner"], "x", "nested unknown fields stay");
        assert_eq!(v["save"]["split_by_type"], false, "our change is written");
        assert_eq!(v["parts_keep_days"], 3);
        assert_eq!(load(&path).parts_keep_days, 3, "and reads back");

        // An older or same-version file is simply replaced.
        std::fs::write(&path, r#"{"version":1,"stale":true}"#).unwrap();
        save(&path, &Settings::default()).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(v.get("stale").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
