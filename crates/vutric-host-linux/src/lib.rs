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
//! | [`paths`] | XDG Downloads (localised), staging, identity, device name, helper binaries | yes |
//!
//! The `vutric-surface` binary owns the GPU and the Wayland surfaces; the daemon starts it on demand
//! (layer-shell) or asks the GNOME extension to (GNOME), and talks to it over [`proto`].

pub mod files;
pub mod gnome;
pub mod i18n;
pub mod menu;
pub mod notify;
pub mod paths;
pub mod proto;
#[cfg(target_os = "linux")]
pub mod sni;
pub mod stats;
