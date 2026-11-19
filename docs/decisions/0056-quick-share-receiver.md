# ADR-0056: Quick Share receiver (Windows first), clean-room, in the compat process

Status: accepted (2026-11-18) · Stream: W3 · Backlog: C3 · Builds on: ADR-0010, ADR-0054

## Context

ADR-0010 puts Quick Share LAN mode in Phase 1 for Windows: an Android phone sends from its normal share sheet,
with nothing installed. Google publishes no specification; the best public description is NearDrop's
`PROTOCOL.md` (Unlicense), plus Google's open UKEY2 description and the field numbers of the protocol's
`.proto` files (Chromium, BSD-3).

## The protocol, read again

`grishka/NearDrop`, `PROTOCOL.md` at commit `f75f5fb` (2025-08-06), read on 2026-11-18:

- mDNS `_FC9F5ED42C8A._tcp`, a 10-byte instance name, TXT `n` with the endpoint info;
- TCP, 4-byte big-endian length per message;
- UKEY2 with P-256 and a SHA-512 commitment;
- AES-256-CBC + HMAC-SHA256 `SecureMessage`, sequence-numbered;
- the payload layer, paired-key frames, introduction, response, chunks;
- keep-alives every 10 s.

## Decision

1. **`crates/vutric-compat-quickshare`**, written from those documents, no code copied:
   - `pb`: a hand-written protobuf reader and writer instead of generated code. The reader never allocates or
     recurses by itself, refuses groups and truncated fields, and every caller keeps only the fields it needs,
     with limits (1000 files, 1024-character names, 4096 fields per message).
   - `ukey2`: server and client handshakes. Checked, in order: message type, version 1, a 32-byte random, the
     P-256 commitment, the next protocol, then the SHA-512 commitment against `ClientFinished`, then the point
     (validated by `ring`). Key schedule as documented: D2D keys, then `ENC:2` / `SIG:1`. The 4-digit code uses
     the documented algorithm (mod 9973, base 31).
   - The channel verifies the HMAC (constant time) before decrypting, and refuses bad padding and any sequence
     number but the next one.
   - Crypto: `ring` for ECDH, HKDF, HMAC and SHA; RustCrypto `aes` 0.8 + `cbc` 0.1 for AES-256-CBC (both already
     in the lock file through other crates).
   - `server` (receive) enforces the order: connection request, UKEY2, connection responses, paired-key
     encryption, paired-key result, introduction, the user's answer, chunks. File chunks are accepted only for
     payload ids from the introduction, after the user accepted, at the next offset, never past the announced
     size. Files are staged as `.vutric-part` and renamed only when complete. Any break ends the connection,
     deletes the parts and reports `Cancelled`. Shared text and links arrive as `Text`.
   - Other limits: at most 8 connections, 10 per address per minute, 1 MiB frames, 16 open byte payloads of at
     most 64 KiB, 45 s of silence. We send a keep-alive every 10 s.
   - `mdns` advertises with `mdns-sd`, only while the receiver runs (receive mode, ADR-0010 §3.3).
   - `client` (send) exists for the tests and for later sending.
2. **The same sandboxed process as LocalSend.** The adapter binary is renamed `vutric-compat`. `Start.quickshare`
   turns the receiver on. Offers reach the daemon as the same `Offer` message with `via: quickshare` and the
   4-digit `code`, which the UI shows next to the sender's. A non-empty `decide` accepts the whole transfer,
   because Quick Share has no per-file answer. All IPC changes are additive (`serde(default)`).
3. **Labelled experimental** until it has survived two Quick Share releases (backlog C3).

## Why not

- **`prost` and generated code:** code generation in the build, and parsers whose allocation we do not
  control. We need about 25 fields.
- **An existing Rust Quick Share crate:** the best known one, `rqs_lib` (Martichou/rquickshare), is GPL-3.0
  (checked 2026-11-18); ADR-0010 forbids copying GPL code.
- **Our own mDNS responder** on top of `simple-dns`: more code that parses network packets, for no gain.

## Measured

`crates/vutric-compat-quickshare/tests/transfer.rs` and `vutric-compat-localsend/tests/roundtrip.rs`, Windows 10:

| Check | Result |
|---|---|
| Two files (3.2 MB in 7 chunks, plus an empty one), byte for byte | pass; both sides show the same code |
| Throughput, loopback, handshake included | 14 MB/s debug; 70-110 MB/s release (3 runs) |
| Reject | the sender gets `Refused(2)` |
| Hostile sender after accept: unknown payload id, offset gap, bytes past the size, leaving half way, a frame outside the channel | each ends the transfer, `Cancelled`, no file left |
| mDNS advert seen by an mDNS browser, TXT `n` decoded back to the name | pass (`--ignored`, needs multicast) |
| Through the sandboxed `vutric-compat` process, the code in the offer equals the sender's | pass |
| Unit tests (protobuf, frames, UKEY2 refusals, tampering, replay, code) | 10 pass |

## Left open

- **M5, a real Android phone:** send a photo, a video and a link from the share sheet; record the Android and
  Quick Share versions; check that the code matches. Until then C3 is `[~]`.
- **Sending to Android:** Android advertises over mDNS only after a BLE wake-up (`fe2c` service data). Windows
  can publish BLE advertisements through WinRT; this belongs to W2's native layer and is not built.
- Bandwidth upgrade (Wi-Fi Direct / hotspot) is ignored; transfers stay on the LAN.
- QR-code mode (`quickshare.google/qrcode#key=`) is not implemented.
- Linux (Phase 2) and macOS (Phase 5) only need the advert and the host wiring; the crate is portable.
