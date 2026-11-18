//! Wayland hosts for the Vutric overlay (ADR-0090).
//!
//! - [`session`] decides the backend: `zwlr_layer_shell_v1` (KDE Plasma 6, wlroots, COSMIC, Hyprland, niri) or the
//!   GNOME Shell extension. Pure, tested on every OS.
//! - [`geometry`] converts between card rasters and Wayland's logical coordinates. Pure.
//! - `Host` (Linux only) owns the Wayland connection and creates the effect [`Surface`] (click-through, no
//!   keyboard focus, wgpu-compatible) and the card surface (clickable, still no keyboard focus).
//!
//! The Windows equivalent is `vutric-host-win`; both give the daemon the same three things: a transparent effect
//! surface that never takes input, a focus-free card that takes clicks, and the pointer events on that card.

pub mod geometry;
pub mod session;

#[cfg(target_os = "linux")]
mod wl;

#[cfg(target_os = "linux")]
pub use wl::{Error, Host, Mouse, OutputSummary, Surface};
