# ADR-0062: KDE Connect v8 share adapter (C4)

Status: accepted (2026-11-18) · Stream: W3 · Backlog: C4 · Builds on: ADR-0010, ADR-0054, ADR-0056, ADR-0061

## Context

KDE Connect is on most Linux desktops and has an Android app (and GSConnect on GNOME). Backlog C4 asks for its
share plugin only: send and receive files, text and links with a paired device. Everything else KDE Connect does
(notifications, clipboard, remote input, SMS) is out of scope. ADR-0010 rules apply: clean-room code, the
foreign peer is always asked about, a sandboxed process does every pre-auth parse, a fuzzer and an interop test.

## Source read (re-read 2026-11-18)

The protocol description in `kdeconnect-meta` (`protocol.md`), version 8. No KDE Connect, GSConnect or Valent
source code was read or copied; the crate is written from that text alone.

## Decision

New crate `crates/vutric-compat-kdeconnect`, hosted inside the existing `vutric-compat` process (ADR-0056), so it
shares that process's sandbox, staging directory and `Decide` flow. Turned on by `Start.kdeconnect`.

| Part | What it does |
|---|---|
| `proto` | Newline-delimited JSON packets. Identity, pair, `share.request` (file, text, url) and `share.request.update`. Only protocol version 8; a v7 identity is ignored. Lines capped: 4 KiB before TLS, 1 MiB after; names cut to 32 characters, capabilities to 128, text to 64 KiB. |
| `tls` | ECDSA P-256 self-signed certificate, CN = device id (UUID without dashes, 32 hex); kept in `kdeconnect.id` next to the `LocalSend` identity. Any certificate is accepted at the TLS layer; trust is decided by `trust`. The verification key is SHA-256 over both SPKIs sorted, then the pairing timestamp in decimal, first 8 hex characters, upper case. |
| `trust` | Paired devices and their pinned certificate in `kdeconnect-trusted.json` (256 at most). A paired id with a different certificate is dropped (`Verdict::Mismatch`). |
| `link` | Discovery: UDP identity on 1716. The side that hears it dials TCP, sends a plaintext identity naming the target device id, then acts as the TLS **server**; the TCP server is the TLS client. Full identity is exchanged again inside TLS. |
| `payload` | File bytes on a separate TLS connection to a port in 1739–1764 announced in `payloadTransferInfo.port`. Files land as `.vutric-part` in staging and are renamed when the size matches. |
| `receiver` | Accept loop (16 connections at most), pairing questions as events with a timeout, `send` to a paired device, `announce`. |

IPC (`vutric-compat-localsend::ipc`, additive): `Start.kdeconnect`, `Via::Kdeconnect`, `Ready.kdeconnect_port` and
`kdeconnect_id`, `Seen.via` and `device_id`, new `PairRequest`, `Paired`, `Unpaired` messages and the `KdeSend`
command. `Via` and `FromAdapter` are now `#[non_exhaustive]`; no code outside W3 matches on them yet.

Rules:

- A pairing request becomes `FromAdapter::PairRequest` with the 8-character key; both screens show it. The daemon
  answers with `Decide`: non-empty `accept` pairs, empty refuses, no answer within `decide_timeout` refuses.
- The pairing timestamp must be within 30 minutes of our clock.
- Shares from a device that is not paired are ignored without reading the payload.
- `KdeSend` refuses a device that is not paired or whose certificate changed, before anything is offered.

## S5 row (ADR-0061)

| Route | Before authentication | After | Code |
|---|---|---|---|
| KDE Connect v8 (C4) | One JSON line before TLS, 4 KiB cap read before parsing, must name our device id; UDP identity datagrams parsed with the same cap. Inside the sandboxed `vutric-compat` process; fuzz `kdeconnect_packet`. | Mutual TLS; the certificate is pinned at pairing and checked on every connection, including payload connections. | 8-hex verification key from both certificates and the timestamp. Not the 6-digit exporter code (not in the protocol); deviation, like C1/C3. |

## Not verified

Written from the protocol text only; none of these has been checked against the real app yet (M5):

1. Capability strings `kdeconnect.share.request` and `kdeconnect.share.request.update` are all we announce; the
   real app may want more to show the share action.
2. Payload TLS roles: we assume the receiver of the payload is the TLS client.
3. Verification key: hex encoding, upper case and the 8-character length.
4. A v7 device (older Android app) is ignored, not downgraded.

## Measured

| Check | Result |
|---|---|
| `vutric-compat-kdeconnect` unit tests (packet parsing, refusals, SPKI walker, key symmetry, trust file) | 8 pass |
| `tests/interop.rs`: pair then file + text; unpaired ignored and "no" means no; changed certificate dropped; first line must name us and stay small; UDP identity makes the receiver dial, then we send a file to the paired phone and a stranger is refused | 5 pass, 3 runs |
| `vutric-compat-localsend/tests/roundtrip.rs::kdeconnect_through_the_sandboxed_process`: pair and receive 250 KB through the real `vutric-compat` binary, pairing file written | pass |
| `fuzz` smoke `kdeconnect_random_and_near_valid` | pass |

## Consequences

- Real-device test with KDE Connect on Linux and Android is the remaining step (M5); the four items above are
  checked then, and this ADR is amended.
- Fixed ports (UDP 1716, TCP 1716–1764, payload 1739–1764) are used only when `Start.port` is fixed; tests use port 0.
