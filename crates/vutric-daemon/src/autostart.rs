//! Start with the user's session: `HKCU\...\Run` on Windows, an XDG autostart entry on Linux (a systemd user unit
//! follows with the Linux host, D10). The MSIX build uses its manifest `StartupTask` instead and skips this.

use std::path::Path;

/// The `Run` value name and the `.desktop` file stem.
pub const NAME: &str = "Vutric";
/// Passed by autostart so the daemon starts quietly (no pairing card on first run).
pub const FLAG: &str = "--autostart";

pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// The command line written to the `Run` key: the quoted exe path, then [`FLAG`].
#[must_use]
pub fn run_command(exe: &Path) -> String {
    format!("\"{}\" {FLAG}", exe.display())
}

/// An XDG autostart entry. `Exec` quoting follows the Desktop Entry spec (backslash, quote, dollar, backtick).
#[must_use]
pub fn desktop_entry(exe: &Path) -> String {
    let mut q = String::new();
    for c in exe.to_string_lossy().chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            q.push('\\');
        }
        q.push(c);
    }
    format!(
        "[Desktop Entry]\nType=Application\nName={NAME}\nExec=\"{q}\" {FLAG}\nIcon=vutric\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n"
    )
}

/// True when the stored command starts this exe (a moved or updated install counts as off, so it is rewritten).
#[must_use]
pub fn points_at(stored: &str, exe: &Path) -> bool {
    stored.trim() == run_command(exe)
}

#[cfg(windows)]
#[must_use]
pub fn is_enabled(exe: &Path) -> bool {
    crate::winreg::get_string(RUN_KEY, NAME)
        .ok()
        .flatten()
        .is_some_and(|s| points_at(&s, exe))
}

#[cfg(windows)]
pub fn set(enabled: bool, exe: &Path) -> windows::core::Result<()> {
    if enabled {
        crate::winreg::set_string(RUN_KEY, NAME, &run_command(exe))
    } else {
        crate::winreg::delete_value(RUN_KEY, NAME)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn entry_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
        })?;
    Some(
        base.join("autostart")
            .join(format!("{}.desktop", NAME.to_lowercase())),
    )
}

#[cfg(all(unix, not(target_os = "macos")))]
#[must_use]
pub fn is_enabled(exe: &Path) -> bool {
    entry_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .is_some_and(|s| s == desktop_entry(exe))
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn set(enabled: bool, exe: &Path) -> std::io::Result<()> {
    let p = entry_path().ok_or_else(|| std::io::Error::other("no config folder"))?;
    if enabled {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, desktop_entry(exe))
    } else {
        match std::fs::remove_file(p) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_command_is_quoted() {
        let exe = Path::new(r"C:\Program Files\Vutric\vutric-daemon.exe");
        assert_eq!(
            run_command(exe),
            r#""C:\Program Files\Vutric\vutric-daemon.exe" --autostart"#
        );
        assert!(points_at(&format!("{}  ", run_command(exe)), exe));
        assert!(!points_at(r#""C:\old\vutric-daemon.exe" --autostart"#, exe));
    }

    #[test]
    fn desktop_entry_escapes_exec() {
        let e = desktop_entry(Path::new("/opt/my $app/vutric"));
        assert!(e.contains("Exec=\"/opt/my \\$app/vutric\" --autostart\n"));
        assert!(e.starts_with("[Desktop Entry]\n"));
    }

    #[cfg(windows)]
    #[test]
    fn registry_round_trip_under_a_test_key() {
        let key = format!(r"Software\Wonmor\VutricTest-{}", std::process::id());
        crate::winreg::set_string(&key, "v", "a \"b\" c").unwrap();
        assert_eq!(
            crate::winreg::get_string(&key, "v").unwrap().as_deref(),
            Some("a \"b\" c")
        );
        crate::winreg::delete_value(&key, "v").unwrap();
        assert_eq!(crate::winreg::get_string(&key, "v").unwrap(), None);
        crate::winreg::delete_value(&key, "v").unwrap();
    }
}
