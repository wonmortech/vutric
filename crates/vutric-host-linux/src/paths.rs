//! Where Vutric keeps things on Linux (ADR-0093 §3): the XDG Downloads folder (localised, for example
//! `~/Yuklanmalar` or the Russian-locale name), the staging folder next to it, the compat adapter's identity, the
//! device name the phone sees, and where the helper binaries are installed.
//!
//! Pure functions over an environment getter and file contents, so they are tested without a desktop; the
//! `*_from_env` wrappers read the real process.

use std::path::{Path, PathBuf};

use crate::files::{config_home, data_home};

/// Folder inside Downloads that received files land in (same name on every platform).
pub const SUBFOLDER: &str = "Vutric";

/// Where the package installs helper binaries, tried in order (Debian/Ubuntu, Fedora/Arch, user install).
pub const LIBEXEC_DIRS: &[&str] = &["/usr/libexec/vutric", "/usr/lib/vutric"];

/// `$XDG_CACHE_HOME`, else `~/.cache`.
#[must_use]
pub fn cache_home(get: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    get("XDG_CACHE_HOME")
        .filter(|v| v.starts_with('/'))
        .map(PathBuf::from)
        .or_else(|| get("HOME").map(|h| PathBuf::from(h).join(".cache")))
}

/// The `XDG_DOWNLOAD_DIR` entry of a `user-dirs.dirs` file (`xdg-user-dirs` format: `KEY="$HOME/Name"` or an
/// absolute quoted path). `None` when the key is missing, malformed, or points at the home folder itself (the
/// spec's way of saying "disabled").
#[must_use]
pub fn parse_user_dirs(contents: &str, key: &str, home: &Path) -> Option<PathBuf> {
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim() != key {
            continue;
        }
        let v = v.trim();
        let v = v.strip_prefix('"')?.strip_suffix('"')?;
        let v = v.replace("\\\"", "\"");
        let path = if let Some(rest) = v.strip_prefix("$HOME") {
            let rest = rest.trim_start_matches('/');
            if rest.is_empty() {
                return None;
            }
            home.join(rest)
        } else if v.starts_with('/') {
            PathBuf::from(v)
        } else {
            return None;
        };
        return (path.as_path() != home).then_some(path);
    }
    None
}

/// The user's Downloads folder: `XDG_DOWNLOAD_DIR` from `user-dirs.dirs` (`user_dirs` = its contents, if read),
/// else `~/Downloads`.
#[must_use]
pub fn downloads(get: impl Fn(&str) -> Option<String>, user_dirs: Option<&str>) -> Option<PathBuf> {
    let home = PathBuf::from(get("HOME")?);
    user_dirs
        .and_then(|c| parse_user_dirs(c, "XDG_DOWNLOAD_DIR", &home))
        .or_else(|| Some(home.join("Downloads")))
}

/// Received files go to `Downloads/Vutric`.
#[must_use]
pub fn received_dir(
    get: impl Fn(&str) -> Option<String>,
    user_dirs: Option<&str>,
) -> Option<PathBuf> {
    downloads(get, user_dirs).map(|d| d.join(SUBFOLDER))
}

/// Partial files are staged in `Downloads/Vutric/.incoming`, on the same file system as the final folder, so
/// landing a file is a `rename`, never a copy (`~/.cache` may be a tmpfs or another disk).
#[must_use]
pub fn staging_dir(received: &Path) -> PathBuf {
    received.join(".incoming")
}

/// The compat adapter's TLS identity (the `LocalSend` fingerprint, ADR-0054): `$XDG_DATA_HOME/vutric/compat-identity`.
#[must_use]
pub fn compat_identity(get: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    data_home(get).map(|d| d.join("vutric/compat-identity"))
}

/// Settings live in `$XDG_CONFIG_HOME/vutric/`.
#[must_use]
pub fn config_dir(get: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    config_home(get).map(|d| d.join("vutric"))
}

/// Logs and caches: `$XDG_CACHE_HOME/vutric/`.
#[must_use]
pub fn cache_dir(get: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    cache_home(get).map(|d| d.join("vutric"))
}

/// `PRETTY_HOSTNAME` from `/etc/machine-info` (what GNOME Settings > About > Device Name writes), unquoted.
#[must_use]
pub fn pretty_hostname(machine_info: &str) -> Option<String> {
    machine_info.lines().find_map(|l| {
        let v = l.trim().strip_prefix("PRETTY_HOSTNAME=")?;
        let v = v
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .or_else(|| v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
            .unwrap_or(v);
        let v = v.replace("\\\"", "\"");
        let v = v.trim();
        (!v.is_empty()).then(|| v.to_owned())
    })
}

/// The name phones show for this computer: the pretty hostname, else the static hostname, else "Linux".
#[must_use]
pub fn device_name(machine_info: Option<&str>, hostname: Option<&str>) -> String {
    machine_info
        .and_then(pretty_hostname)
        .or_else(|| {
            hostname
                .map(str::trim)
                .filter(|h| !h.is_empty() && *h != "localhost")
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "Linux".to_owned())
}

/// Where to find helper binary `name` (`vutric-surface`, `vutric-compat`): next to the running executable first
/// (a tarball or `cargo run`), then the package's libexec folders, then `~/.local/libexec/vutric`.
#[must_use]
pub fn helper_candidates(
    name: &str,
    current_exe: Option<&Path>,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = current_exe.and_then(Path::parent) {
        out.push(dir.join(name));
    }
    out.extend(LIBEXEC_DIRS.iter().map(|d| Path::new(d).join(name)));
    if let Some(h) = home {
        out.push(h.join(".local/libexec/vutric").join(name));
    }
    out
}

#[cfg(target_os = "linux")]
mod imp {
    use std::path::PathBuf;

    fn env(k: &str) -> Option<String> {
        std::env::var(k).ok().filter(|v| !v.is_empty())
    }

    fn user_dirs() -> Option<String> {
        let cfg = super::config_home(env)?;
        std::fs::read_to_string(cfg.join("user-dirs.dirs")).ok()
    }

    /// `Downloads/Vutric` for this user.
    #[must_use]
    pub fn received_dir_from_env() -> Option<PathBuf> {
        super::received_dir(env, user_dirs().as_deref())
    }

    #[must_use]
    pub fn device_name_from_system() -> String {
        let info = std::fs::read_to_string("/etc/machine-info").ok();
        let host = std::fs::read_to_string("/etc/hostname")
            .ok()
            .or_else(|| env("HOSTNAME"));
        super::device_name(info.as_deref(), host.as_deref())
    }

    /// The first installed copy of helper `name`.
    #[must_use]
    pub fn find_helper(name: &str) -> Option<PathBuf> {
        use std::os::unix::fs::PermissionsExt as _;
        let exe = std::env::current_exe().ok();
        let home = env("HOME").map(PathBuf::from);
        super::helper_candidates(name, exe.as_deref(), home.as_deref())
            .into_iter()
            .find(|p| {
                std::fs::metadata(p)
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
    }
}

#[cfg(target_os = "linux")]
pub use imp::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn env(k: &str) -> Option<String> {
        match k {
            "HOME" => Some("/home/ali".to_owned()),
            _ => None,
        }
    }

    const UZ_DIRS: &str = "# written by xdg-user-dirs-update\n\
        XDG_DESKTOP_DIR=\"$HOME/Ish stoli\"\n\
        XDG_DOWNLOAD_DIR=\"$HOME/Yuklanmalar\"\n";

    #[test]
    fn localised_downloads_folder_is_used() {
        assert_eq!(
            received_dir(env, Some(UZ_DIRS)).unwrap(),
            PathBuf::from("/home/ali/Yuklanmalar/Vutric")
        );
        let ru = "XDG_DOWNLOAD_DIR=\"$HOME/\u{0417}\u{0430}\u{0433}\u{0440}\u{0443}\u{0437}\u{043a}\u{0438}\"\n";
        assert_eq!(
            downloads(env, Some(ru)).unwrap(),
            PathBuf::from(
                "/home/ali/\u{0417}\u{0430}\u{0433}\u{0440}\u{0443}\u{0437}\u{043a}\u{0438}"
            )
        );
    }

    #[test]
    fn missing_or_disabled_entries_fall_back_to_downloads() {
        let home = Path::new("/home/ali");
        assert_eq!(
            parse_user_dirs("XDG_DOWNLOAD_DIR=\"$HOME/\"\n", "XDG_DOWNLOAD_DIR", home),
            None
        );
        assert_eq!(
            parse_user_dirs("XDG_DOWNLOAD_DIR=\"$HOME\"\n", "XDG_DOWNLOAD_DIR", home),
            None
        );
        assert_eq!(
            parse_user_dirs("XDG_DOWNLOAD_DIR=relative\n", "XDG_DOWNLOAD_DIR", home),
            None
        );
        assert_eq!(
            parse_user_dirs("#XDG_DOWNLOAD_DIR=\"/x\"\n", "XDG_DOWNLOAD_DIR", home),
            None
        );
        assert_eq!(
            parse_user_dirs("XDG_DOWNLOAD_DIR=\"/data/dl\"\n", "XDG_DOWNLOAD_DIR", home),
            Some(PathBuf::from("/data/dl"))
        );
        assert_eq!(
            downloads(env, None).unwrap(),
            PathBuf::from("/home/ali/Downloads")
        );
        assert_eq!(downloads(|_| None, Some(UZ_DIRS)), None);
    }

    #[test]
    fn staging_sits_on_the_same_file_system() {
        let r = PathBuf::from("/home/ali/Yuklanmalar/Vutric");
        assert_eq!(staging_dir(&r), r.join(".incoming"));
    }

    #[test]
    fn xdg_data_and_cache_paths() {
        assert_eq!(
            compat_identity(env).unwrap(),
            PathBuf::from("/home/ali/.local/share/vutric/compat-identity")
        );
        assert_eq!(
            cache_dir(env).unwrap(),
            PathBuf::from("/home/ali/.cache/vutric")
        );
        assert_eq!(
            config_dir(env).unwrap(),
            PathBuf::from("/home/ali/.config/vutric")
        );
        let custom = |k: &str| (k == "XDG_CACHE_HOME").then(|| "/tmp/c".to_owned());
        assert_eq!(cache_dir(custom).unwrap(), PathBuf::from("/tmp/c/vutric"));
    }

    #[test]
    fn device_name_prefers_the_pretty_hostname() {
        let info = "CHASSIS=laptop\nPRETTY_HOSTNAME=\"Ali's ThinkPad\"\n";
        assert_eq!(device_name(Some(info), Some("tp14\n")), "Ali's ThinkPad");
        assert_eq!(
            device_name(Some("CHASSIS=desktop\n"), Some("tp14\n")),
            "tp14"
        );
        assert_eq!(device_name(None, Some("localhost")), "Linux");
        assert_eq!(device_name(Some("PRETTY_HOSTNAME=\n"), None), "Linux");
        assert_eq!(
            pretty_hostname("PRETTY_HOSTNAME=Ofis\n").as_deref(),
            Some("Ofis")
        );
    }

    #[test]
    fn helpers_are_found_next_to_the_exe_first() {
        let c = helper_candidates(
            "vutric-compat",
            Some(Path::new("/opt/vutric/vutric-daemon")),
            Some(Path::new("/home/ali")),
        );
        assert_eq!(
            c,
            vec![
                PathBuf::from("/opt/vutric/vutric-compat"),
                PathBuf::from("/usr/libexec/vutric/vutric-compat"),
                PathBuf::from("/usr/lib/vutric/vutric-compat"),
                PathBuf::from("/home/ali/.local/libexec/vutric/vutric-compat"),
            ]
        );
    }
}
