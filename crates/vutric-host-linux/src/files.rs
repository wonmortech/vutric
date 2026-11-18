//! Session integration files (ADR-0091): the systemd user unit, the Nautilus script and the Dolphin service menu.
//! The packaged copies live in `packaging/linux/`; these functions install per-user copies for a tarball or a
//! portable build and keep the autostart switch in one place.

use std::path::{Path, PathBuf};

use crate::i18n::{Lang, t};

pub const UNIT_NAME: &str = "vutric.service";
pub const SYSTEMD_UNIT: &str = include_str!("../../../packaging/linux/systemd/vutric.service");
pub const NAUTILUS_SCRIPT: &str = include_str!("../../../packaging/linux/nautilus/vutric-send.sh");
pub const DOLPHIN_MENU: &str = include_str!("../../../packaging/linux/kio/vutric-send.desktop");
pub const DESKTOP_ENTRY: &str =
    include_str!("../../../packaging/linux/applications/io.wonmor.Vutric.desktop");

/// Where the package puts the unit; a per-user unit is only written when this one is missing.
pub const SYSTEM_UNIT_PATH: &str = "/usr/lib/systemd/user/vutric.service";

/// Quotes a path for `ExecStart=`: double quotes, backslash escapes, and `%`/`$` doubled so systemd expands
/// neither specifiers nor variables.
#[must_use]
pub fn systemd_quote(p: &Path) -> String {
    let mut out = String::from("\"");
    for c in p.to_string_lossy().chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '%' => out.push_str("%%"),
            '$' => out.push_str("$$"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The packaged unit with `ExecStart` pointing at `exe` (for installs outside `/usr/bin`).
#[must_use]
pub fn unit_for(exe: &Path) -> String {
    SYSTEMD_UNIT
        .lines()
        .map(|l| {
            if l.starts_with("ExecStart=") {
                format!("ExecStart={} --autostart", systemd_quote(exe))
            } else {
                l.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// `$XDG_DATA_HOME`, else `~/.local/share`.
#[must_use]
pub fn data_home(get: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    get("XDG_DATA_HOME")
        .filter(|v| v.starts_with('/'))
        .map(PathBuf::from)
        .or_else(|| get("HOME").map(|h| PathBuf::from(h).join(".local/share")))
}

/// `$XDG_CONFIG_HOME`, else `~/.config`.
#[must_use]
pub fn config_home(get: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    get("XDG_CONFIG_HOME")
        .filter(|v| v.starts_with('/'))
        .map(PathBuf::from)
        .or_else(|| get("HOME").map(|h| PathBuf::from(h).join(".config")))
}

/// Nautilus shows a script under its file name, so the name is the localised label.
#[must_use]
pub fn nautilus_script_path(data: &Path, lang: Lang) -> PathBuf {
    data.join("nautilus/scripts")
        .join(t(lang, "linux.send_with"))
}

#[must_use]
pub fn dolphin_menu_path(data: &Path) -> PathBuf {
    data.join("kio/servicemenus/vutric-send.desktop")
}

#[must_use]
pub fn user_unit_path(config: &Path) -> PathBuf {
    config.join("systemd/user").join(UNIT_NAME)
}

#[cfg(target_os = "linux")]
mod imp {
    use std::io;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;
    use std::process::Command;

    use super::{
        DOLPHIN_MENU, Lang, NAUTILUS_SCRIPT, SYSTEM_UNIT_PATH, UNIT_NAME, config_home, data_home,
        dolphin_menu_path, nautilus_script_path, unit_for, user_unit_path,
    };

    fn env(k: &str) -> Option<String> {
        std::env::var(k).ok().filter(|v| !v.is_empty())
    }

    fn write_executable(path: &Path, body: &str) -> io::Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(path, body)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
    }

    /// Installs "Vutric bilan yuborish" for Nautilus (always per user, Nautilus has no system scripts folder)
    /// and for Dolphin (per user only when the package's system-wide menu is missing). Idempotent.
    pub fn install_file_manager_entries(lang: Lang) -> io::Result<()> {
        let data = data_home(env).ok_or_else(|| io::Error::other("no data folder"))?;
        for other in [Lang::Uz, Lang::En] {
            if other != lang {
                let _ = std::fs::remove_file(nautilus_script_path(&data, other));
            }
        }
        write_executable(&nautilus_script_path(&data, lang), NAUTILUS_SCRIPT)?;
        let system = ["/usr/share/kio/servicemenus/vutric-send.desktop"];
        if !system.iter().any(|p| Path::new(p).exists()) {
            // KF6 only runs per-user service menus that are executable.
            write_executable(&dolphin_menu_path(&data), DOLPHIN_MENU)?;
        }
        Ok(())
    }

    /// True when a systemd user manager runs this session.
    #[must_use]
    pub fn systemd_available() -> bool {
        env("XDG_RUNTIME_DIR").is_some_and(|d| Path::new(&d).join("systemd").is_dir())
    }

    fn systemctl(args: &[&str]) -> io::Result<()> {
        let st = Command::new("systemctl")
            .arg("--user")
            .args(args)
            .status()?;
        if st.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("systemctl --user {args:?}: {st}")))
        }
    }

    /// The autostart switch through systemd. Writes a per-user unit for `exe` when no packaged unit exists.
    pub fn set_autostart(enabled: bool, exe: &Path) -> io::Result<()> {
        if enabled && !Path::new(SYSTEM_UNIT_PATH).exists() {
            let cfg = config_home(env).ok_or_else(|| io::Error::other("no config folder"))?;
            let p = user_unit_path(&cfg);
            if let Some(d) = p.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(&p, unit_for(exe))?;
            systemctl(&["daemon-reload"])?;
        }
        systemctl(&[if enabled { "enable" } else { "disable" }, UNIT_NAME])
    }

    #[must_use]
    pub fn autostart_enabled() -> bool {
        Command::new("systemctl")
            .args(["--user", "is-enabled", "--quiet", UNIT_NAME])
            .status()
            .is_ok_and(|s| s.success())
    }
}

#[cfg(target_os = "linux")]
pub use imp::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_is_a_user_service_tied_to_the_session() {
        assert!(SYSTEMD_UNIT.contains("\nWantedBy=graphical-session.target\n"));
        assert!(SYSTEMD_UNIT.contains("\nPartOf=graphical-session.target\n"));
        assert!(SYSTEMD_UNIT.contains("\nExecStart=/usr/bin/vutric-daemon --autostart\n"));
        assert!(SYSTEMD_UNIT.contains("\nRestart=on-failure\n"));
    }

    #[test]
    fn exec_start_is_quoted() {
        let u = unit_for(Path::new("/opt/my apps/100%/v$x/vutric-daemon"));
        assert!(
            u.contains("\nExecStart=\"/opt/my apps/100%%/v$$x/vutric-daemon\" --autostart\n"),
            "{u}"
        );
        assert_eq!(u.matches("ExecStart=").count(), 1);
        assert_eq!(
            systemd_quote(Path::new(r#"/a"b\c"#)),
            r#""/a\"b\\c""#.to_owned()
        );
    }

    #[test]
    fn paths_follow_xdg() {
        let env = |k: &str| match k {
            "HOME" => Some("/home/ali".to_owned()),
            "XDG_DATA_HOME" => Some("relative/is/ignored".to_owned()),
            _ => None,
        };
        let data = data_home(env).unwrap();
        assert_eq!(data, PathBuf::from("/home/ali/.local/share"));
        assert_eq!(
            nautilus_script_path(&data, Lang::Uz),
            PathBuf::from("/home/ali/.local/share/nautilus/scripts/Vutric bilan yuborish")
        );
        assert_eq!(
            dolphin_menu_path(&data),
            PathBuf::from("/home/ali/.local/share/kio/servicemenus/vutric-send.desktop")
        );
        let cfg = config_home(|k| (k == "XDG_CONFIG_HOME").then(|| "/x/cfg".to_owned())).unwrap();
        assert_eq!(
            user_unit_path(&cfg),
            PathBuf::from("/x/cfg/systemd/user/vutric.service")
        );
    }

    #[test]
    fn file_manager_entries_call_the_daemon() {
        assert!(NAUTILUS_SCRIPT.starts_with("#!/bin/sh\n"));
        assert!(NAUTILUS_SCRIPT.contains("exec vutric-daemon send -- \"$@\""));
        assert!(DOLPHIN_MENU.contains("\nName=Vutric bilan yuborish\n"));
        assert!(DOLPHIN_MENU.contains("\nName[en]=Send with Vutric\n"));
        assert!(DOLPHIN_MENU.contains("\nExec=vutric-daemon send -- %F\n"));
        assert!(DESKTOP_ENTRY.contains("\nIcon=io.wonmor.Vutric\n"));
    }
}
