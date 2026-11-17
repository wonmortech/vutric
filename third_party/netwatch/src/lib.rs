//! Networking related utilities

// Vutric vendoring: noq-udp in our lock deprecates UdpSocketState::send; keep upstream code as is, and keep
// the workspace RUSTFLAGS=-D warnings from failing on it.
#![allow(deprecated)]

pub mod interfaces;
pub mod ip;
mod ip_family;
pub mod netmon;
#[cfg(not(wasm_browser))]
mod udp;

pub use self::ip_family::IpFamily;
#[cfg(not(wasm_browser))]
pub use self::udp::{UdpSender, UdpSocket};
