//! Vutric on the Linux desktop (ADR-0090, ADR-0091).
//!
//! | Module | What | Tested without a desktop |
//! |---|---|---|
//! | [`proto`] | daemon ↔ `vutric-surface` frames | yes |
//! | [`stats`] | frame pacing numbers | yes |
//! | [`menu`] | tray menu model, dbusmenu layout | yes |
//! | [`notify`] | "files arrived" notifications | content yes; sending on a session bus in CI |
//! | `sni` | `StatusNotifierItem` + dbusmenu over zbus (Linux) | on a session bus in CI |
//! | [`gnome`] | the GNOME Shell extension's D-Bus API | contract yes |
//! | [`files`] | systemd user unit, Nautilus script, Dolphin menu | yes |
//! | [`i18n`] | Linux-only strings, Uzbek default | yes |
//! | [`cards`] | the CPU card raster (ADR-0032) to the layer-shell surface or the GNOME extension | yes |
//! | `link` | the daemon's end of the `vutric-surface` socket: start, accept, frames (Linux) | socket path yes; flow in CI via `surface_driver` |
//! | `desktop` | overlay plan, tray (SNI or extension), autostart choice: what the daemon's Linux branch calls (Linux) | plan yes |
//! | [`fonts`] | the desktop's UI font files for the CPU cards | yes |
//! | [`paths`] | XDG Downloads (localised), staging, identity, device name, helper binaries | yes |
//!
//! The `vutric-surface` binary owns the GPU and the Wayland surfaces; the daemon starts it on demand
//! (layer-shell) or asks the GNOME extension to (GNOME), and talks to it over [`proto`].

pub mod cards;
#[cfg(target_os = "linux")]
pub mod desktop;
pub mod files;
pub mod fonts;
pub mod gnome;
pub mod i18n;
#[cfg(target_os = "linux")]
pub mod link;
pub mod menu;
pub mod notify;
pub mod paths;
pub mod proto;
#[cfg(target_os = "linux")]
pub mod sni;
pub mod stats;
