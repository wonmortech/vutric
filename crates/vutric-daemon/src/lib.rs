//! The always-on desktop half of Vutric (ADR-0009 §4): network, transfers, tray, toasts, the overlay and the local
//! IPC the desktop windows use. Everything that can be tested without a desktop session lives in this library;
//! `main.rs` wires it to the OS.

pub mod autostart;
pub mod cards;
pub mod i18n;
pub mod ipc;
pub mod prefs;
pub mod server;
pub mod toast;
#[cfg(windows)]
pub mod winreg;
