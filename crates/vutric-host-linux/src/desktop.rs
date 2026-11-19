//! The Linux desktop backends in one place (ADR-0093 §2): what the daemon's Linux branch calls instead of the
//! Windows tray, toast, overlay and autostart. W2 owns the daemon; this module is the surface it plugs into, so
//! the Linux code stays in W4's crates (patch in `docs/streams/w4-notes.md`, issue #33).
//!
//! - [`plan`]: which overlay this session gets (layer-shell, GNOME with the extension, or none and why);
//! - [`TrayHandle`]: `StatusNotifierItem` where a tray host runs, the extension's panel menu on plain GNOME;
//! - [`autostart_via`]: systemd user unit, or W2's XDG autostart entry when no systemd user manager runs.

use vutric_host_wlr::session::{Desktop, Display, Session, Unsupported};

use crate::gnome::Extension;

/// Where the throw effect and the cards are drawn in this session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayPlan {
    /// Spawn `vutric-surface`; it draws on a `zwlr_layer_shell_v1` overlay layer. If the compositor turns out
    /// not to offer layer-shell, the surface answers `FromSurface::Error` and the daemon falls back to
    /// notifications only.
    LayerShell,
    /// Ask the extension to launch `vutric-surface` (ADR-0090 §2); cards go through `ShowCard`.
    Gnome,
    /// GNOME without a usable extension: files are received, one notification asks to turn the extension on
    /// (`linux.gnome.extension.*` keys) or to update it.
    GnomeNeedsExtension(Extension),
    /// No overlay at all (X11, no display): files are received and announced with notifications.
    NotificationsOnly(Unsupported),
}

/// Decides from the session variables and the extension's `Ping` answer (`gnome::probe`; pass
/// [`Extension::Missing`] off GNOME).
#[must_use]
pub fn plan(session: &Session, extension: &Extension) -> OverlayPlan {
    match session.display {
        Display::None => OverlayPlan::NotificationsOnly(Unsupported::NoDisplay),
        Display::X11 => OverlayPlan::NotificationsOnly(Unsupported::X11),
        Display::Wayland if session.desktop == Desktop::Gnome => match extension {
            Extension::Ready { .. } => OverlayPlan::Gnome,
            other => OverlayPlan::GnomeNeedsExtension(other.clone()),
        },
        Display::Wayland => OverlayPlan::LayerShell,
    }
}

/// How the autostart switch is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutostartVia {
    /// `files::set_autostart` (systemd user unit, restarts on crash).
    Systemd,
    /// W2's existing XDG autostart `.desktop` entry. Never both: systemd turns XDG entries into units too, and
    /// the daemon would start twice (ADR-0091).
    XdgAutostart,
}

#[must_use]
pub fn autostart_via(systemd_user_manager: bool) -> AutostartVia {
    if systemd_user_manager {
        AutostartVia::Systemd
    } else {
        AutostartVia::XdgAutostart
    }
}

/// The tray on Linux: an SNI icon, or the extension's panel menu, or nothing (the daemon still runs; the
/// settings window is reachable from the app grid).
#[derive(Debug)]
pub enum TrayHandle {
    Sni(crate::sni::Tray),
    Extension(crate::gnome::ShellProxy<'static>),
    None,
}

impl TrayHandle {
    /// Tries `StatusNotifierItem` first (KDE, COSMIC, waybar, Ubuntu's `AppIndicator` extension), then the Vutric
    /// extension's menu (plain GNOME). Menu clicks arrive on `actions` as `pair`, `settings`, `toggle_pause`,
    /// `quit` or `activate` either way.
    pub async fn start(
        bus: Option<&zbus::Connection>,
        model: crate::menu::TrayModel,
        lang: &str,
        actions: tokio::sync::mpsc::UnboundedSender<String>,
    ) -> Self {
        let paused = model.paused;
        if let Ok(tray) = crate::sni::Tray::spawn(model, actions.clone()).await {
            return Self::Sni(tray);
        }
        let Some(bus) = bus else {
            return Self::None;
        };
        let Ok(shell) = crate::gnome::ShellProxy::new(bus).await else {
            return Self::None;
        };
        if shell.set_status(paused, lang).await.is_err() {
            return Self::None;
        }
        if let Ok(mut clicks) = shell.receive_menu_activated().await {
            tokio::spawn(async move {
                use futures_util::StreamExt as _;
                while let Some(sig) = clicks.next().await {
                    let Ok(args) = sig.args() else { continue };
                    if actions.send(args.id().clone()).is_err() {
                        return;
                    }
                }
            });
        }
        Self::Extension(shell)
    }

    /// New pause state or language. Errors are ignored: a tray that went away is not worth stopping for.
    pub async fn update(&self, model: crate::menu::TrayModel, lang: &str) {
        match self {
            Self::Sni(t) => {
                let _ = t.set_model(model).await;
            }
            Self::Extension(shell) => {
                let _ = shell.set_status(model.paused, lang).await;
            }
            Self::None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(desktop: Desktop, display: Display) -> Session {
        Session { desktop, display }
    }

    #[test]
    fn the_overlay_follows_the_session() {
        let ready = Extension::Ready {
            shell: "46.0".into(),
        };
        assert_eq!(
            plan(&s(Desktop::Kde, Display::Wayland), &Extension::Missing),
            OverlayPlan::LayerShell
        );
        assert_eq!(
            plan(&s(Desktop::Unknown, Display::Wayland), &Extension::Missing),
            OverlayPlan::LayerShell
        );
        assert_eq!(
            plan(&s(Desktop::Gnome, Display::Wayland), &ready),
            OverlayPlan::Gnome
        );
        assert_eq!(
            plan(
                &s(Desktop::Gnome, Display::Wayland),
                &Extension::TooOld { api: 0 }
            ),
            OverlayPlan::GnomeNeedsExtension(Extension::TooOld { api: 0 })
        );
        assert_eq!(
            plan(&s(Desktop::Gnome, Display::X11), &ready),
            OverlayPlan::NotificationsOnly(Unsupported::X11)
        );
        assert_eq!(
            plan(&s(Desktop::Kde, Display::None), &ready),
            OverlayPlan::NotificationsOnly(Unsupported::NoDisplay)
        );
    }

    #[test]
    fn autostart_never_uses_both() {
        assert_eq!(autostart_via(true), AutostartVia::Systemd);
        assert_eq!(autostart_via(false), AutostartVia::XdgAutostart);
    }
}
