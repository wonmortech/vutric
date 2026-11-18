//! magic-wormhole adapter (backlog C7, ADR-0067): send to and receive from `wormhole send` / `wormhole receive`
//! and other clients of the same protocol, by a short code.
//!
//! Clean-room from the protocol documents in `magic-wormhole/magic-wormhole-protocols` (commit `566ee9e`,
//! 2026-08-31): `server-protocol.md`, `client-protocol.md`, `transit.md`, `file-transfer-protocol.md`. No client
//! source code was read. Runs inside the sandboxed `vutric-compat` process, like the other adapters (ADR-0054).
//!
//! - [`code`]: wormhole codes;
//! - [`crypto`]: SPAKE2, HKDF subkeys, secretbox;
//! - [`mailbox`]: the mailbox server's WebSocket protocol (fuzzed);
//! - [`wormhole`]: `pake`, `version` and ordered application messages;
//! - [`transit`]: direct or relayed TCP, handshake, encrypted records (fuzzed);
//! - [`transfer`]: the file-transfer application protocol (fuzzed).

pub mod code;
pub mod crypto;
pub mod mailbox;
pub mod transfer;
pub mod transit;
pub mod wormhole;
