//! Paired KDE Connect devices and their pinned certificates, in one small JSON file the adapter owns.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::proto;
use crate::tls::fingerprint;

/// Most paired devices kept; a longer file is cut.
pub const MAX_DEVICES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pinned {
    pub name: String,
    /// SHA-256 of the certificate seen when pairing, lower-case hex.
    pub cert_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Paired, and the certificate is the pinned one.
    Paired,
    /// Never paired: only pairing packets are read.
    Unknown,
    /// Paired before with another certificate: the connection is dropped.
    Mismatch,
}

#[derive(Debug, Default)]
pub struct Trust {
    path: Option<PathBuf>,
    devices: BTreeMap<String, Pinned>,
}

impl Trust {
    /// Loads `path`; a missing or damaged file is an empty store.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        let devices: BTreeMap<String, Pinned> = std::fs::read(path)
            .ok()
            .filter(|b| b.len() <= 1 << 20)
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let devices = devices
            .into_iter()
            .filter(|(id, p)| proto::valid_device_id(id) && p.cert_sha256.len() == 64)
            .take(MAX_DEVICES)
            .collect();
        Self {
            path: Some(path.to_owned()),
            devices,
        }
    }

    /// An in-memory store (tests).
    #[must_use]
    pub fn memory() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn check(&self, device_id: &str, cert: &[u8]) -> Verdict {
        match self.devices.get(device_id) {
            None => Verdict::Unknown,
            Some(p) if p.cert_sha256 == fingerprint(cert) => Verdict::Paired,
            Some(_) => Verdict::Mismatch,
        }
    }

    pub fn pin(&mut self, device_id: &str, name: &str, cert: &[u8]) -> Result<()> {
        if self.devices.len() >= MAX_DEVICES && !self.devices.contains_key(device_id) {
            anyhow::bail!("too many paired devices");
        }
        self.devices.insert(
            device_id.to_owned(),
            Pinned {
                name: proto::clean_name(name),
                cert_sha256: fingerprint(cert),
            },
        );
        self.save()
    }

    pub fn forget(&mut self, device_id: &str) -> Result<()> {
        if self.devices.remove(device_id).is_some() {
            self.save()?;
        }
        Ok(())
    }

    #[must_use]
    pub fn devices(&self) -> &BTreeMap<String, Pinned> {
        &self.devices
    }

    fn save(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&self.devices)?)
            .with_context(|| format!("write {}", tmp.display()))?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_checks_and_survives_a_restart() {
        let path =
            std::env::temp_dir().join(format!("vutric-kdc-trust-{}.json", std::process::id()));
        let id = proto::new_device_id();
        let mut t = Trust::load(&path);
        assert_eq!(t.check(&id, b"cert"), Verdict::Unknown);
        t.pin(&id, "Pixel (8)", b"cert").unwrap();
        let t = Trust::load(&path);
        assert_eq!(t.check(&id, b"cert"), Verdict::Paired);
        assert_eq!(t.check(&id, b"other"), Verdict::Mismatch);
        assert_eq!(t.devices()[&id].name, "Pixel 8");
        std::fs::write(&path, b"{not json").unwrap();
        assert!(Trust::load(&path).devices().is_empty());
        let _ = std::fs::remove_file(path);
    }
}
