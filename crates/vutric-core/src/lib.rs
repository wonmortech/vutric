//! Vutric wire protocol.
//!
//! Everything here is pure data and pure functions: no sockets, no files, no clocks. The daemon and the mobile
//! client own the I/O and feed bytes through [`frame`]. See `docs/protocol.md` for the normative description.

pub mod adb;
pub mod beacon;
pub mod ble;
pub mod captive;
pub mod cloud;
pub mod code;
pub mod companion;
pub mod effect;
pub mod error;
pub mod filename;
pub mod frame;
pub mod hotspot;
pub mod id;
pub mod message;
pub mod pairing;
pub mod pointer;
pub mod qrstream;
pub mod ring;
pub mod route;
pub mod sas;
pub mod sound;
pub mod storage;
pub mod telemetry;
pub mod telemetry;
pub mod trust;

pub use effect::{EffectId, SendZone};
pub use error::{Error, Result};
pub use id::{EndpointId, FlingId};
pub use message::{
    FlingStart, Gesture, Message, Notice, OfferEntry, Platform, Preview, PreviewFormat,
};
pub use pairing::PairingTicket;

/// QUIC ALPN identifier. Bumped only on a wire-incompatible change.
pub const ALPN: &[u8] = b"vutric/1";

/// Protocol version carried in [`Message::Hello`] and in pairing tickets.
pub const PROTOCOL_VERSION: u16 = 2;
