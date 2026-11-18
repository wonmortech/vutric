//! Quick Share (formerly Nearby Share) over the LAN (backlog C3, ADR-0010, ADR-0056): Android's share sheet sends
//! to Vutric.
//!
//! Written from `NearDrop`'s `PROTOCOL.md` (commit `f75f5fb`, 2025-08-06, read 2026-11-18), Google's public UKEY2
//! description, and the field numbers of the protocol's `.proto` files; no code copied (clean-room, ADR-0010
//! §3.1). Labelled experimental until it has survived two Quick Share releases (backlog C3).
//!
//! - [`pb`], a bounded protobuf reader and writer;
//! - [`msg`], the messages (UKEY2, `SecureMessage`, offline and sharing frames);
//! - [`ukey2`], the handshake, the 4-digit code and the encrypted channel;
//! - [`endpoint`], the mDNS name and endpoint info;
//! - [`conn`], framing and the payload layer;
//! - [`server`], receiving; [`client`], sending; [`mdns`], advertising.

pub mod client;
pub mod conn;
pub mod endpoint;
pub mod mdns;
pub mod msg;
pub mod pb;
pub mod server;
pub mod ukey2;
