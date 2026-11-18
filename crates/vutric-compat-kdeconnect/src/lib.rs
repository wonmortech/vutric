//! KDE Connect adapter, share plugin only (backlog C4, ADR-0062): files, texts and links to and from KDE Connect
//! (Linux, Windows, Android) and `GSConnect`, protocol version 8.
//!
//! Clean-room from `kdeconnect-meta/protocol.md`; no code from the GPL clients. The parsers see untrusted bytes
//! and run inside the `vutric-compat` process with the other adapters (ADR-0054).

pub mod link;
pub mod payload;
pub mod proto;
pub mod receiver;
pub mod tls;
pub mod trust;
