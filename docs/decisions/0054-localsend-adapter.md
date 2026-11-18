# ADR-0054: The LocalSend adapter, in its own process

Status: accepted (2026-11-18) · Stream: W3 · Backlog: C1, C9 · Builds on: ADR-0010

## Context

ADR-0010 makes LocalSend the first foreign protocol (Phase 1, desktop and Android) and sets the rules: clean-room,
a foreign peer is always asked about unless pinned, visible only in receive mode, pre-auth parsing in its own
low-privilege process, a fuzzer for every parser, a test against the real app, and the protocol version read
again from the source before writing.

## The protocol, read again

<https://github.com/localsend/protocol>, `README.md` at commit `62bd340` (2026-08-07): **v2.2**, not the v2.1
that ADR-0010 and the backlog name. The only change since 2.1: the receiver answers `422` to an `upload` whose
bytes do not match the `sha256` given in `prepare-upload`. We implement 2.2 and accept any `2.x` peer.

## Decision

1. **`crates/vutric-compat-localsend`**, written from the spec only:
   - `proto`: messages and strict parsers. Bodies are size-checked before parsing (1 MiB, 4 KiB datagrams, 4096
     files), every string is bounded, aliases lose control and bidi-override characters, file names come out of
     `vutric_core::filename::sanitize`, a `sha256` must be 64 hex characters, previews over 64 KiB are dropped.
   - `tls`: a self-signed certificate kept across restarts; its SHA-256 (upper-case hex) is the fingerprint the
     user pins. The client accepts the peer's self-signed certificate but still verifies the handshake
     signature, and reports the fingerprint. The receiver asks for an optional client certificate, so a sender
     that presents one is identified by it rather than by its self-declared `fingerprint` field.
   - `server` (receive): nothing is written before the user accepts; one session at a time (`409`); 10 offers per
     minute per address (`429`); an upload must come from the address that prepared it, with that file's token
     (`403`); never more bytes than announced; `422` on a checksum mismatch; files land as `.vutric-part` in a
     staging folder and are renamed only when complete; `cancel` deletes the parts. At most 64 connections,
     30 s idle per upload.
   - `client` (send): `register`, `prepare-upload` (with `pin`), streamed `upload`, `cancel`; refusals come back
     as typed `Refusal` values.
   - `discovery`: multicast announce and answer on `224.0.0.167:53317` (register over HTTPS first, multicast
     answer as fallback), our own echoes filtered by fingerprint.
2. **Its own process.** The `vutric-compat-localsend` binary runs all of the above; the daemon starts it through
   `host::Adapter` and talks JSON lines over stdin/stdout (`ipc`). The daemon treats the adapter as untrusted
   too: lines are capped at 1 MiB, messages are closed enums, and a received file is moved only if
   `host::checked_path` finds a plain file with a sanitized name directly inside the staging folder.
   Process limits:
   - all: empty environment (Windows keeps `SystemRoot`, without which Winsock does not start), no inherited
     handles besides the pipes, killed with the daemon;
   - Windows: a job object with a 512 MiB memory cap, no child processes, kill on close;
   - Linux: `no_new_privs`, `PDEATHSIG`; Unix: no core dumps.
3. **Settings** (`vutric_core::storage::Compat`, C9): `compat.{localsend, browser, quickshare, kdeconnect,
   warpinator, croc, wormhole}`, each with `enabled` and `pinned`. LocalSend and the browser page are on by
   default, the rest off. Files written before `compat` existed get the defaults; pinned entries that are not
   64-hex fingerprints are dropped. Additive: every existing `Settings` literal uses `..Settings::default()`.
4. **Fuzzing**: `localsend_prepare`, `localsend_announce` (announcements, `register`, query strings) and
   `localsend_ipc` (what the daemon reads from the adapter) join `fuzz/`, the stable smoke run and the CI matrix.

## Why not

- **An existing Rust `localsend` crate:** ADR-0010 allows one only after a licence and code review; the protocol
  is small, and our parsers need limits a general crate does not promise.
- **Running the adapter inside the daemon:** HTTP and JSON parsing happen before any user decision; a bug there
  must not reach the daemon's keys or the user's files outside the staging folder.
- **AppContainer (Windows) or Landlock/seccomp (Linux) now:** the job object and the path check already bound
  the damage; a full OS sandbox is the next step (left open below).

## Measured

`crates/vutric-compat-localsend/tests/roundtrip.rs`, Windows 10, debug build, loopback HTTPS: 3 MB in 81-87 ms
(34-37 MB/s; release numbers belong to N7). The same tests cover accepting one file of two, rejection (`403`), a
wrong PIN (`401`), `register`, and the whole path through the sandboxed process.

## Left open

- **M5, the real app:** send to and receive from LocalSend on Android and Windows and record the app version.
  Until then backlog C1 is `[~]`. Steps: install LocalSend, turn on Vutric receive mode, send one photo each way,
  check the `422` path with a corrupted file.
- The fingerprint's letter case as the official app computes it (we only compare values we computed ourselves,
  so pinning works either way).
- AppContainer / Landlock for the adapter process; the browser page (C2) and the download API (spec §5).
