//! Which desktop we run on and therefore which host draws the overlay (ADR-0090).
//!
//! Detection is pure: it reads the session variables through a closure, so every desktop is tested on any OS.
//! The final word is the compositor itself: a `zwlr_layer_shell_v1` global wins over any name.

/// The desktop environment named by `XDG_CURRENT_DESKTOP` (first match wins, case-insensitive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Desktop {
    Gnome,
    Kde,
    Cosmic,
    Sway,
    Hyprland,
    Wayfire,
    Niri,
    Other(String),
    Unknown,
}

/// The display protocol of the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Wayland,
    X11,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub desktop: Desktop,
    pub display: Display,
}

/// Where the overlay and the cards are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// `zwlr_layer_shell_v1`, layer `overlay`: KDE Plasma 6, wlroots compositors, COSMIC, Hyprland, niri.
    LayerShell,
    /// GNOME Shell: an xdg toplevel launched by the Vutric extension, which retypes it (ADR-0090).
    GnomeShell,
    /// No overlay: files are still received and announced with a notification.
    Unsupported(Unsupported),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsupported {
    /// An X11 session. Out of scope for Phase 2 (owner question, ADR-0090).
    X11,
    /// A Wayland compositor without layer-shell that is not GNOME (for example Weston's desktop shell).
    NoLayerShell,
    /// No graphical session at all (SSH, a system service).
    NoDisplay,
}

fn desktop_from(name: &str) -> Desktop {
    for part in name.split(':').map(str::trim).filter(|p| !p.is_empty()) {
        let d = match part.to_ascii_lowercase().as_str() {
            "gnome" | "gnome-classic" | "ubuntu" | "pop" => Desktop::Gnome,
            "kde" | "plasma" => Desktop::Kde,
            "cosmic" => Desktop::Cosmic,
            "sway" | "swayfx" => Desktop::Sway,
            "hyprland" => Desktop::Hyprland,
            "wayfire" => Desktop::Wayfire,
            "niri" => Desktop::Niri,
            _ => continue,
        };
        return d;
    }
    match name.split(':').map(str::trim).find(|p| !p.is_empty()) {
        Some(p) => Desktop::Other(p.to_owned()),
        None => Desktop::Unknown,
    }
}

impl Session {
    /// Reads `XDG_CURRENT_DESKTOP`, `XDG_SESSION_DESKTOP`, `XDG_SESSION_TYPE`, `WAYLAND_DISPLAY` and `DISPLAY`.
    #[must_use]
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Self {
        let present = |k: &str| get(k).is_some_and(|v| !v.trim().is_empty());
        let desktop = get("XDG_CURRENT_DESKTOP")
            .filter(|v| !v.trim().is_empty())
            .or_else(|| get("XDG_SESSION_DESKTOP"))
            .map_or(Desktop::Unknown, |v| desktop_from(&v));
        let kind = get("XDG_SESSION_TYPE").map(|v| v.trim().to_ascii_lowercase());
        let display = match kind.as_deref() {
            Some("x11") if present("DISPLAY") => Display::X11,
            _ if present("WAYLAND_DISPLAY") => Display::Wayland,
            _ if present("DISPLAY") => Display::X11,
            _ => Display::None,
        };
        Self { desktop, display }
    }

    #[must_use]
    pub fn from_env() -> Self {
        Self::from_vars(|k| std::env::var(k).ok())
    }

    /// The backend once the compositor has been asked whether it offers layer-shell.
    #[must_use]
    pub fn backend(&self, has_layer_shell: bool) -> Backend {
        match self.display {
            Display::None => Backend::Unsupported(Unsupported::NoDisplay),
            Display::X11 => Backend::Unsupported(Unsupported::X11),
            Display::Wayland if has_layer_shell => Backend::LayerShell,
            Display::Wayland if self.desktop == Desktop::Gnome => Backend::GnomeShell,
            Display::Wayland => Backend::Unsupported(Unsupported::NoLayerShell),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(vars: &[(&str, &str)]) -> Session {
        Session::from_vars(|k| {
            vars.iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| (*v).to_owned())
        })
    }

    #[test]
    fn ubuntu_gnome_wayland_uses_the_extension() {
        let s = session(&[
            ("XDG_CURRENT_DESKTOP", "ubuntu:GNOME"),
            ("XDG_SESSION_TYPE", "wayland"),
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("DISPLAY", ":0"),
        ]);
        assert_eq!(s.desktop, Desktop::Gnome);
        assert_eq!(s.display, Display::Wayland);
        assert_eq!(s.backend(false), Backend::GnomeShell);
    }

    #[test]
    fn layer_shell_global_wins_over_the_name() {
        for d in [
            "KDE", "COSMIC", "sway", "Hyprland", "niri", "wayfire", "river",
        ] {
            let s = session(&[
                ("XDG_CURRENT_DESKTOP", d),
                ("XDG_SESSION_TYPE", "wayland"),
                ("WAYLAND_DISPLAY", "wayland-1"),
            ]);
            assert_eq!(s.backend(true), Backend::LayerShell, "{d}");
        }
        let gnome = session(&[("XDG_CURRENT_DESKTOP", "GNOME"), ("WAYLAND_DISPLAY", "w")]);
        assert_eq!(gnome.backend(true), Backend::LayerShell);
    }

    #[test]
    fn names_are_recognised() {
        let d = |v: &str| session(&[("XDG_CURRENT_DESKTOP", v)]).desktop;
        assert_eq!(d("KDE"), Desktop::Kde);
        assert_eq!(d("pop:GNOME"), Desktop::Gnome);
        assert_eq!(d("COSMIC"), Desktop::Cosmic);
        assert_eq!(d("X-Cinnamon"), Desktop::Other("X-Cinnamon".into()));
        assert_eq!(d(""), Desktop::Unknown);
        assert_eq!(
            session(&[("XDG_SESSION_DESKTOP", "plasma")]).desktop,
            Desktop::Kde
        );
    }

    #[test]
    fn x11_and_headless_are_refused_with_a_reason() {
        let x = session(&[
            ("XDG_CURRENT_DESKTOP", "KDE"),
            ("XDG_SESSION_TYPE", "x11"),
            ("DISPLAY", ":0"),
        ]);
        assert_eq!(x.backend(false), Backend::Unsupported(Unsupported::X11));
        let ssh = session(&[]);
        assert_eq!(
            ssh.backend(false),
            Backend::Unsupported(Unsupported::NoDisplay)
        );
        let weston = session(&[("XDG_CURRENT_DESKTOP", "weston"), ("WAYLAND_DISPLAY", "w")]);
        assert_eq!(
            weston.backend(false),
            Backend::Unsupported(Unsupported::NoLayerShell)
        );
    }

    #[test]
    fn session_type_wayland_without_a_socket_falls_back() {
        let s = session(&[("XDG_SESSION_TYPE", "wayland"), ("DISPLAY", ":1")]);
        assert_eq!(s.display, Display::X11);
    }
}
