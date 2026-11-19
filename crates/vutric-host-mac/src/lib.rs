//! Turns `AppKit` windows into the Vutric overlay on macOS (roadmap Phase 5, ADR-0110):
//!
//! - a borderless `NSPanel` with `NSWindowStyleMaskNonactivatingPanel`: showing it never activates the app and it
//!   never becomes key, so typing in the frontmost app is never interrupted;
//! - `ignoresMouseEvents`: every click falls through;
//! - on every Space and over full-screen apps (`canJoinAllSpaces | fullScreenAuxiliary | canJoinAllApplications`),
//!   at the status-bar level by default ([`OverlayLevel`]);
//! - transparent: clear background, no shadow; the pixels come from wgpu's Metal backend through a `CAMetalLayer`
//!   on the panel's content view.
//!
//! [`policy`] and [`pacing`] are pure and tested on every OS; the `AppKit` calls exist on macOS only.

pub mod compat;
pub mod pacing;
pub mod policy;

pub use policy::OverlayLevel;

#[cfg(target_os = "macos")]
pub mod card;
#[cfg(target_os = "macos")]
mod imp;
#[cfg(target_os = "macos")]
pub mod link;

#[cfg(target_os = "macos")]
pub use imp::*;
