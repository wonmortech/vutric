//! Where received files go: the user's storage settings (ADR-0007 part 4) and the pure rules that turn an
//! offered path into a place on disk. Loading and saving the settings file is the host's job; this module only
//! defines the data, its defaults, the per-device overrides, and the path arithmetic.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::filename;

/// Bumped when a field changes meaning; [`Settings::migrate`] brings older files forward.
pub const SETTINGS_VERSION: u32 = 1;

/// Folder levels kept from the sender's tree, as in the wire limit.
const MAX_DEPTH: usize = 32;

/// What kind of file an entry is, for "split by type" and for the gallery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Photos,
    Videos,
    Documents,
    Archives,
    Audio,
    Other,
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::Photos,
        Kind::Videos,
        Kind::Documents,
        Kind::Archives,
        Kind::Audio,
        Kind::Other,
    ];

    /// From the MIME type first, the file extension second.
    #[must_use]
    pub fn of(path: &str, mime: &str) -> Kind {
        let mime = mime.to_ascii_lowercase();
        if mime.starts_with("image/") {
            return Kind::Photos;
        }
        if mime.starts_with("video/") {
            return Kind::Videos;
        }
        if mime.starts_with("audio/") {
            return Kind::Audio;
        }
        let ext = path
            .rsplit(['/', '\\'])
            .next()
            .and_then(|n| n.rsplit_once('.'))
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "jpg" | "jpeg" | "png" | "heic" | "heif" | "gif" | "webp" | "bmp" | "tif" | "tiff"
            | "avif" | "dng" | "raw" | "cr2" | "nef" | "arw" => Kind::Photos,
            "mp4" | "mov" | "m4v" | "webm" | "mkv" | "avi" | "3gp" | "wmv" => Kind::Videos,
            "mp3" | "m4a" | "aac" | "wav" | "flac" | "ogg" | "opus" | "wma" => Kind::Audio,
            "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" => Kind::Archives,
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp"
            | "txt" | "rtf" | "md" | "csv" | "epub" | "pages" | "numbers" | "key" => {
                Kind::Documents
            }
            _ => Kind::Other,
        }
    }

    /// The default folder name for this kind (Uzbek, the product language).
    #[must_use]
    pub fn folder(self) -> &'static str {
        match self {
            Kind::Photos => "Rasmlar",
            Kind::Videos => "Videolar",
            Kind::Documents => "Hujjatlar",
            Kind::Archives => "Arxivlar",
            Kind::Audio => "Audio",
            Kind::Other => "Boshqa",
        }
    }

    /// Whether a phone puts this kind into the photo library when gallery saving is on.
    #[must_use]
    pub fn goes_to_gallery(self) -> bool {
        matches!(self, Kind::Photos | Kind::Videos)
    }
}

/// An extra folder level under the save folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subfolder {
    #[default]
    None,
    /// The sending device's name.
    Device,
    /// The day it arrived, `2026-11-18`.
    Date,
    /// Device, then day.
    DeviceDate,
}

/// What happens when a file of that name is already there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conflict {
    /// `photo (2).jpg`.
    #[default]
    KeepBoth,
    /// The new file replaces the old one.
    Replace,
    /// The old file stays; the new one is dropped (counted as received).
    Skip,
}

/// Whether an offer is accepted without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode", content = "max_bytes")]
pub enum Accept {
    /// Ask every time.
    #[default]
    Ask,
    /// Accept everything from a trusted (paired) device.
    Trusted,
    /// Accept from a trusted device when the whole offer is at most this many bytes; ask otherwise.
    UpTo(u64),
}

impl Accept {
    /// True when an offer of `bytes` from a device the user trusts (`trusted`) needs no question.
    #[must_use]
    pub fn auto(self, trusted: bool, bytes: u64) -> bool {
        match self {
            Accept::Ask => false,
            Accept::Trusted => trusted,
            Accept::UpTo(max) => trusted && bytes <= max,
        }
    }
}

/// What the host does once files are in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum After {
    #[default]
    Notify,
    OpenFile,
    OpenFolder,
    Nothing,
}

/// The rules for one transfer: the global settings with the sending device's overrides applied.
#[allow(clippy::struct_excessive_bools)] // independent user switches, saved as they appear on screen
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SaveRules {
    /// The save folder; `None` = the platform default (`Downloads/Vutric`, the Files app folder…).
    pub folder: Option<PathBuf>,
    /// Put each kind in its own folder.
    pub split_by_type: bool,
    /// Folder per kind when splitting: relative to the save folder, or absolute. Missing kinds use
    /// [`Kind::folder`].
    pub type_folders: BTreeMap<Kind, PathBuf>,
    pub subfolder: Subfolder,
    /// Keep the sender's folder tree (`Trip/day 1/a.jpg`); off = every file straight in the folder.
    pub keep_tree: bool,
    pub conflict: Conflict,
    /// Phones: photos and videos also go to the photo library.
    pub gallery: bool,
    pub album: String,
    /// Phones: keep the file in the save folder after it went to the gallery.
    pub keep_copy: bool,
}

impl Default for SaveRules {
    fn default() -> Self {
        Self {
            folder: None,
            split_by_type: false,
            type_folders: BTreeMap::new(),
            subfolder: Subfolder::None,
            keep_tree: true,
            conflict: Conflict::KeepBoth,
            gallery: true,
            album: "Vutric".into(),
            keep_copy: true,
        }
    }
}

/// What one received file needs to be placed.
#[derive(Debug, Clone, Copy)]
pub struct Arrival<'a> {
    /// The offered relative path (`/`-separated, hostile).
    pub path: &'a str,
    pub mime: &'a str,
    /// The sending device's display name.
    pub device: &'a str,
    /// The local day, `YYYY-MM-DD`.
    pub date: &'a str,
}

impl SaveRules {
    /// The final path for a received file, before conflict handling. `default_folder` is used when no folder
    /// is set. Every component that came from the peer is sanitized, so the result stays inside the chosen
    /// folder (or inside an absolute type folder the user chose).
    #[must_use]
    pub fn target(&self, default_folder: &Path, a: Arrival<'_>) -> PathBuf {
        let mut out = self
            .folder
            .clone()
            .unwrap_or_else(|| default_folder.to_path_buf());
        if self.split_by_type {
            let kind = Kind::of(a.path, a.mime);
            match self.type_folders.get(&kind) {
                Some(p) if p.is_absolute() => out.clone_from(p),
                Some(p) => out.push(clean_relative(p)),
                None => out.push(kind.folder()),
            }
        }
        match self.subfolder {
            Subfolder::None => {}
            Subfolder::Device => out.push(filename::sanitize(a.device)),
            Subfolder::Date => out.push(filename::sanitize(a.date)),
            Subfolder::DeviceDate => {
                out.push(filename::sanitize(a.device));
                out.push(filename::sanitize(a.date));
            }
        }
        let parts: Vec<String> = a
            .path
            .split(['/', '\\'])
            .filter(|p| !p.is_empty() && *p != "." && *p != "..")
            .take(MAX_DEPTH)
            .map(filename::sanitize)
            .collect();
        match parts.split_last() {
            None => out.push(filename::sanitize("file")),
            Some((name, dirs)) => {
                if self.keep_tree {
                    for d in dirs {
                        out.push(d);
                    }
                }
                out.push(name);
            }
        }
        out
    }
}

/// A user-typed relative folder (`Rasmlar/2026`) with `..`, drive letters and roots removed.
fn clean_relative(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    s.split(['/', '\\'])
        .filter(|c| !c.is_empty() && *c != "." && *c != ".." && !c.ends_with(':'))
        .map(filename::sanitize)
        .collect()
}

/// Overrides for one remembered device; `None` = use the global value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceRules {
    pub folder: Option<PathBuf>,
    pub split_by_type: Option<bool>,
    pub subfolder: Option<Subfolder>,
    pub accept: Option<Accept>,
    pub gallery: Option<bool>,
}

/// Everything the user can set about receiving. One file per device, owned by the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub save: SaveRules,
    pub accept: Accept,
    pub after: After,
    /// Refuse an offer when less than this would remain free on the disk.
    pub min_free_bytes: u64,
    /// Unfinished `.vutric-part` files older than this are deleted.
    pub parts_keep_days: u32,
    /// History entries older than this are deleted; `None` = keep forever, `Some(0)` = no history.
    pub history_days: Option<u32>,
    /// Overrides by the device's endpoint id (64 hex).
    pub devices: BTreeMap<String, DeviceRules>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            save: SaveRules::default(),
            accept: Accept::Ask,
            after: After::Notify,
            min_free_bytes: 1 << 30,
            parts_keep_days: 7,
            history_days: Some(90),
            devices: BTreeMap::new(),
        }
    }
}

impl Settings {
    /// The rules for a transfer from `device` (its endpoint id in hex).
    #[must_use]
    pub fn rules_for(&self, device: &str) -> (SaveRules, Accept) {
        let mut save = self.save.clone();
        let mut accept = self.accept;
        if let Some(d) = self.devices.get(device) {
            if let Some(f) = &d.folder {
                save.folder = Some(f.clone());
            }
            if let Some(v) = d.split_by_type {
                save.split_by_type = v;
            }
            if let Some(v) = d.subfolder {
                save.subfolder = v;
            }
            if let Some(v) = d.gallery {
                save.gallery = v;
            }
            if let Some(v) = d.accept {
                accept = v;
            }
        }
        (save, accept)
    }

    /// Brings a file written by an older version forward and clamps values to sane ranges.
    #[must_use]
    pub fn migrate(mut self) -> Self {
        self.version = SETTINGS_VERSION;
        self.parts_keep_days = self.parts_keep_days.clamp(1, 30);
        if self.save.album.trim().is_empty() {
            "Vutric".clone_into(&mut self.save.album);
        }
        self.save.album = self.save.album.chars().take(64).collect();
        self
    }
}

/// `YYYY-MM-DD` of a Unix time in seconds, in UTC (for hosts with no local calendar at hand).
#[must_use]
pub fn date_of_unix(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a<'x>(path: &'x str, mime: &'x str) -> Arrival<'x> {
        Arrival {
            path,
            mime,
            device: "Nosir PC",
            date: "2026-11-18",
        }
    }

    #[test]
    fn kinds() {
        assert_eq!(Kind::of("x/IMG_1.HEIC", ""), Kind::Photos);
        assert_eq!(Kind::of("a.bin", "video/mp4"), Kind::Videos);
        assert_eq!(Kind::of("r.pdf", "application/pdf"), Kind::Documents);
        assert_eq!(Kind::of("b.7z", "application/octet-stream"), Kind::Archives);
        assert_eq!(Kind::of("song.flac", ""), Kind::Audio);
        assert_eq!(Kind::of("noext", ""), Kind::Other);
    }

    #[test]
    fn default_keeps_the_tree_in_the_default_folder() {
        let r = SaveRules::default();
        let base = Path::new("dl");
        assert_eq!(
            r.target(base, a("Trip/day 1/a.jpg", "image/jpeg")),
            base.join("Trip").join("day 1").join("a.jpg")
        );
    }

    #[test]
    fn split_subfolders_and_flatten() {
        let mut r = SaveRules {
            folder: Some(PathBuf::from("save")),
            split_by_type: true,
            subfolder: Subfolder::DeviceDate,
            keep_tree: false,
            ..SaveRules::default()
        };
        r.type_folders
            .insert(Kind::Documents, PathBuf::from("../../Docs/Work"));
        let save = Path::new("save");
        assert_eq!(
            r.target(Path::new("ignored"), a("Trip/a.jpg", "image/jpeg")),
            save.join("Rasmlar")
                .join("Nosir PC")
                .join("2026-11-18")
                .join("a.jpg")
        );
        assert_eq!(
            r.target(Path::new("ignored"), a("x/report.pdf", "")),
            save.join("Docs")
                .join("Work")
                .join("Nosir PC")
                .join("2026-11-18")
                .join("report.pdf")
        );
    }

    #[test]
    fn hostile_names_stay_inside() {
        let r = SaveRules {
            subfolder: Subfolder::Device,
            ..SaveRules::default()
        };
        let base = Path::new("dl");
        let t = r.target(
            base,
            Arrival {
                path: "../../etc/passwd",
                mime: "",
                device: "../..",
                date: "x",
            },
        );
        assert!(t.starts_with(base));
        assert!(!t.components().any(|c| c.as_os_str() == ".."));
    }

    #[test]
    fn device_overrides_and_accept() {
        let mut s = Settings {
            accept: Accept::UpTo(100),
            ..Settings::default()
        };
        s.devices.insert(
            "ab".into(),
            DeviceRules {
                folder: Some(PathBuf::from("phone")),
                accept: Some(Accept::Trusted),
                gallery: Some(false),
                ..DeviceRules::default()
            },
        );
        let (save, accept) = s.rules_for("ab");
        assert_eq!(save.folder, Some(PathBuf::from("phone")));
        assert!(!save.gallery);
        assert!(accept.auto(true, u64::MAX));
        let (save, accept) = s.rules_for("cd");
        assert_eq!(save.folder, None);
        assert!(accept.auto(true, 100));
        assert!(!accept.auto(true, 101));
        assert!(!accept.auto(false, 1));
    }

    #[test]
    fn dates() {
        assert_eq!(date_of_unix(0), "1970-01-01");
        assert_eq!(date_of_unix(951_782_400), "2000-02-29");
        assert_eq!(date_of_unix(1_795_000_000), "2026-11-18");
    }

    #[test]
    fn migrate_clamps() {
        let s = Settings {
            version: 0,
            parts_keep_days: 900,
            save: SaveRules {
                album: "  ".into(),
                ..SaveRules::default()
            },
            ..Settings::default()
        }
        .migrate();
        assert_eq!(s.version, SETTINGS_VERSION);
        assert_eq!(s.parts_keep_days, 30);
        assert_eq!(s.save.album, "Vutric");
    }
}
