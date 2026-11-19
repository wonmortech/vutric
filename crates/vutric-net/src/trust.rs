//! The trust store on disk: one JSON file per device, next to the settings (backlog S1).
//!
//! A missing file is an empty store. A damaged file is kept beside as `.bad` and the store starts empty: the
//! user pairs again, which is safe, while guessing at a damaged list could trust the wrong key.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
pub use vutric_core::trust::{
    Admission, EVERYONE_FOR, PairMethod, TrustStore, TrustedDevice, Visibility, admit,
};

/// The trust file name inside the host's config folder (see [`crate::settings::desktop_dir`]).
pub const FILE: &str = "trusted.json";

/// Unix seconds now, the clock [`admit`] and [`Visibility`] take.
#[must_use]
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Reads the store at `path`; empty when absent or unreadable.
#[must_use]
pub fn load(path: &Path) -> TrustStore {
    let Ok(text) = std::fs::read_to_string(path) else {
        return TrustStore::default();
    };
    if let Ok(s) = serde_json::from_str::<TrustStore>(&text) {
        s.migrate()
    } else {
        let mut bad = path.as_os_str().to_owned();
        bad.push(".bad");
        let _ = std::fs::rename(path, PathBuf::from(bad));
        crate::say(format!(
            "vutric-net: {} could not be read; trusted devices must pair again",
            path.display()
        ));
        TrustStore::default()
    }
}

/// Writes the store atomically (temp file, then rename), owner-only on Unix.
pub fn save(path: &Path, s: &TrustStore) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(s).context("encode trust store")?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    {
        use std::io::Write as _;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        let mut f = opts
            .open(&tmp)
            .with_context(|| format!("write {}", tmp.display()))?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path).with_context(|| format!("save {}", path.display()))
}

#[cfg(test)]
mod tests {
    use vutric_core::{EndpointId, Platform};

    use super::*;

    #[test]
    fn round_trip_damaged_and_missing() {
        let dir = std::env::temp_dir().join(format!("vutric-trust-{}", std::process::id()));
        let path = dir.join(FILE);
        assert!(load(&path).is_empty());

        let mut s = TrustStore::default();
        s.trust(
            EndpointId([3; 32]),
            "Pixel 8",
            Some(Platform::Android),
            PairMethod::Ticket,
            now(),
        );
        save(&path, &s).unwrap();
        assert_eq!(load(&path), s);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(&"03".repeat(32)), "{text}");

        std::fs::write(&path, "{ nope").unwrap();
        assert!(load(&path).is_empty());
        assert!(dir.join("trusted.json.bad").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
