//! `LocalSend` adapter (backlog C1, ADR-0010, ADR-0054): Vutric receives from and sends to the `LocalSend` app.
//!
//! Protocol v2.2, read again from <https://github.com/localsend/protocol> (commit `62bd340`, 2026-08-07) on
//! 2026-11-18; implemented from that document only (clean-room, ADR-0010 §3.1).
//!
//! - [`proto`], messages and strict parsers (fuzzed);
//! - [`tls`], the self-signed HTTPS identity and its fingerprint;
//! - [`server`], receiving (offer, ask the user, staged upload, `422` on a checksum mismatch);
//! - [`client`], sending;
//! - [`discovery`], multicast announce and answer;
//! - the [`ipc`] and [`host`] modules: everything above runs in its own low-privilege process (`vutric-compat-localsend`
//!   binary); the daemon talks to it over JSON lines on stdin/stdout (ADR-0010 §3.4).

pub mod client;
pub mod discovery;
pub mod host;
pub mod ipc;
pub mod proto;
pub mod server;
pub mod tls;
