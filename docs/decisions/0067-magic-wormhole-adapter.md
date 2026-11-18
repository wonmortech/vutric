# ADR-0067: magic-wormhole adapter (C7); croc parked (C6)

Status: accepted (2026-11-18) · Stream: W3 · Backlog: C7, C6 · Builds on: ADR-0010, ADR-0056, ADR-0061, ADR-0062

## Context

magic-wormhole is how many developers and Linux users send a file "to anyone, by a short code". The two sides
meet on a public mailbox server and run SPAKE2 on the code. Then they open an encrypted pipe, either directly or
through a transit relay. Backlog C7 asks for send and receive. Backlog C6 asks for croc, a similar tool. ADR-0010
rules apply: clean-room code, the foreign peer is always asked about, the sandboxed process does every parse, plus
a fuzzer and a real-app interop test.

## Source read (2026-11-18)

The protocol documents in the magic-wormhole repository, `docs/` at commit `566ee9e` (2026-08-31):

- `server-protocol.md`
- `client-protocol.md`
- `file-transfer-protocol.md`
- `transit.md`

No magic-wormhole source code (Python, Rust `magic-wormhole.rs` or Go `wormhole-william`) was read or copied. The
crate is written from those four texts alone. The reference tool was only *run*, as the other end of the interop
tests.

## Decision

New crate `crates/vutric-compat-wormhole`. It is hosted inside the existing `vutric-compat` process (ADR-0056), so
every byte from the mailbox server, the peer and the relay is parsed in that sandbox.

| Part | What it does |
|---|---|
| `code` | `<nameplate>-<words>`: lower-cased, spaces become `-`, at most 128 characters, a numeric nameplate of at most 40 digits. The codes we make are `N-dddd-dddd` (digits instead of the PGP word list: easy to read out in Uzbek; the reference tool accepts them, measured below). |
| `mailbox` | WebSocket JSON client. Steps: `welcome`, then `bind`, `allocate`/`claim`, `open`, `add`, `release` and `close` with a mood. Frames are capped at 1 MiB, message bodies at 256 KiB and ids at 128 characters. A server that asks for proof of work is refused. |
| `crypto` | Symmetric SPAKE2 (Ed25519 group, password = code, identity = app id), then HKDF-SHA256 subkeys. A phase key is `wormhole:phase:` ‖ SHA-256(side) ‖ SHA-256(phase). Mailbox phases and transit records use NaCl secretbox (XSalsa20-Poly1305). |
| `wormhole` | `create` (allocate a code) and `join` (by code). Then `pake`, `version`, and numbered phases delivered in order. A phase that does not decrypt means a wrong code: we close with mood `scary` and fail. |
| `transit` | Direct TCP and relay (`please relay <token> for side <16 hex>`) are raced; the sender picks one with `go`. Records are a 4-byte big-endian length, then `nonce ‖ secretbox`, with a counting nonce. A record above 4 MiB is refused. At most 16 direct and 4 relay hints are read. |
| `transfer` | App id `lothar.com/wormhole/text-or-file-xfer`. Handles text and one file. A directory offer is received as a `.zip` and never unpacked. Offered names go through `filename::sanitize`. The receiver puts the SHA-256 of what it got in its `ack`; as sender we compare it with ours. |

IPC (`vutric-compat-localsend::ipc`, additive only):

- Commands `ToAdapter::WormholeSend { request_id, paths, text, servers }` and `WormholeReceive { request_id, code,
  servers }`. `WormholeServers { mailbox, relay }` defaults to the reference tool's public servers.
- Messages `FromAdapter::WormholeCode` (the code to show) and `WormholeFailed`, plus `Via::Wormhole`.
- A received file goes through the usual `Offer` → `Decide` → `Received` → `Finished` flow, on session
  `wh-<request_id>`. A text arrives as `Text` (kind 1).
- `Adapter::wormhole(cmd)` sends either command.

Rules:

- Every receive is a user act: the code is typed, then the file offer is asked about. With no answer within
  `decide_timeout`, the offer is refused (`{"error": "transfer rejected"}`).
- The code is the password. It goes only to the screen (`WormholeCode`) and is never logged.
- One file or one text per code, like the reference tool's single-offer exchange. Several files mean several codes
  (the reference tool zips a folder; we do not, on send).
- Sending uses the internet servers unless `servers` names others. That is the "goes over the internet" case of
  S6: the UI must say so (W1/W2), and Lan-only mode must hide the action.

## S5 row (ADR-0061)

| Route | Before authentication | After | Code |
|---|---|---|---|
| magic-wormhole (C7) | Mailbox JSON frames capped at 1 MiB; SPAKE2 message only (`pake_v1`, hex, at most 128 bytes); everything else is ignored until the key exists. Inside the sandboxed `vutric-compat` process; fuzz `wormhole_messages`. | Every phase and every transit record is secretbox-encrypted with a key from the code; a failed decryption ends the session. | The code itself (PAKE): a wrong code fails on the first phase. Not the 6-digit exporter code (not in the protocol); deviation, like C1/C3/C4. The `verifier` is available for a later "compare" screen. |

## C6 croc: parked

croc has no written protocol description. Its wire format (PAKE over a relay room, then its own encrypted
messages, with the details varying between releases) exists only as its Go source. Writing an adapter would mean
reading that source, which breaks the clean-room rule of ADR-0010 (C8). Unlike magic-wormhole, there is no protocol
text to work from. C6 stays `[ ]` with this reason, and an owner question is added: build it by reading croc's
MIT-licensed source (allowed by the licence, but not clean-room), or drop it.

## Not verified

1. The relay path against the *real* tool. The reference tool always offers direct hints and reaches us directly
   on one machine. The relay line format and the record layer through the real relay are checked between two of
   our own transits (`transit_through_the_real_relay`). The record layer against the real tool is checked on the
   direct path.
2. The public servers (`relay.magic-wormhole.io`, `transit.magic-wormhole.io`): every test runs against local
   copies of the same server software (mailbox 0.8.0, transit relay 0.5.0).
3. Other implementations: `wormhole-william` (Go, used by some Android apps), Warp/`magic-wormhole.rs` (GNOME) and
   Destiny (Android).
4. Directory send: we do not send folders. We receive a folder as the `.zip`, but no test covers it.
5. Dilation (the newer "connection dilation" protocol) is not implemented. The reference tool falls back to classic
   transit, as the tests show.
6. Phones: the adapter compiles into the same `vutric-compat` library that ADR-0066 runs on Android, but it has not
   run on a phone.

## Measured

Local reference tool: magic-wormhole 0.24.0 on Python 3.12, with `twisted==24.11.0` (26.10 crashes on Windows),
mailbox server 0.8.0 and transit relay 0.5.0, all on 127.0.0.1.

| Check | Result |
|---|---|
| `vutric-compat-wormhole` unit tests: codes, PAKE agreement and disagreement, secretbox tamper, phase keys, mailbox parsing and refusals, hints round-trip and bounds, strict offers, direct pipe both ways and stranger refused | 12 pass |
| `tests/interop.rs`, against the real tool: receive its text; receive its 1.5 MB file; it receives our text (our numeric code); it receives our 900 KB file; a wrong code does not open; two transits through the real relay | 6 pass; the first 5 pass in 4 runs, the relay test once it was fixed (below) |
| `vutric-compat-localsend/tests/wormhole_process.rs`: through the real `vutric-compat` binary, the tool sends a 300 KB file (Offer → Decide → staged, checked by `received_file`), then we send a text and the tool prints it | pass, 3 runs |
| `fuzz` smoke `wormhole_random_and_near_valid` | pass |
| CI job `Interop / magic-wormhole 0.24.0` (`.github/workflows/interop.yml`) runs both test files on Ubuntu with the pinned versions | see PR |

The interop tests found one bug before commit. We sent `please relay <token> for <side>` with the 5-byte mailbox
side. The relay answers `bad handshake` unless the line reads `for side <16 hex digits>`. Transit now has its own
8-byte side (`transit::transit_side`).

## Consequences

- C7 is `[~]`: the engine and the sandboxed hosting are done, while the UI (code field, code display, internet
  notice) is W1/W2. Real-device checks: a phone and the public servers.
- Adding Dilation later is additive: the `version` message would announce it, and older peers ignore it.
