# ADR-0061: Pre-authentication rules on every route (S5) and the privacy line (S6)

Status: accepted (2026-11-18) · Stream: W3 · Backlog: S5, S6 · Builds on: ADR-0005, ADR-0007 part 2, ADR-0050

## Context

ADR-0007 part 2 sets three rules for every route: before authentication only bounded, fixed frames are read and
nothing rich is parsed; every frame type goes through the same encryption (the Samsung Quick Share "V5" lesson);
every route shows the 6-digit code from the TLS exporter. Backlog S6 adds an honest privacy line under the send
button. This ADR checks each route we have against the rules, closes the one gap found, and gives S6 its engine.

## S5: route by route

| Route | Before authentication | After | 6-digit code |
|---|---|---|---|
| Vutric (iroh QUIC) | TLS 1.3 with raw public keys authenticates both keys inside the QUIC handshake; `Peer::accept_with` checks the key before reading the first application byte (`tests/trust_gate.rs`). | One control stream: 4-byte length + postcard, 128 KiB cap checked before buffering (`frame::Decoder`, fuzz `frame_stream`); every message type rides the same TLS stream. | `Session::check_code` and the commit-then-reveal `pair_code`, both from the TLS exporter (ADR-0050). |
| LAN beacon (R1) | Exactly 41 bytes or dropped unread; carries a key and a port only; answers rate-limited (`tests/beacon_unicast.rs`, fuzz `beacon`). | Nothing: it only leads to a QUIC connection, which then authenticates the key. | Not applicable. |
| Typed address / code (ADR-0059) | The beacon, as above. | QUIC, as above. | Required: `pair_code` before the device is trusted. |
| LocalSend v2.2 (C1) | The protocol has no authentication before `prepare-upload`, so JSON is parsed pre-auth. Mitigation as ADR-0010 requires: a separate low-privilege process, bodies capped before parsing (1 MiB, 4 KiB datagrams, 4096 files), every parser fuzzed (`localsend_*`). | HTTPS with the peer's self-signed certificate; the user pins its fingerprint. | Not in the protocol. The pinned fingerprint and the receive-mode prompt stand in for it; listed as a deviation. |
| Browser page (C2) | HTTPS; the link token is checked before any upload is parsed. | Same TLS connection. | Not applicable (no app on the other side). |
| Quick Share (C3) | UKEY2 messages and the connection request are protobuf, parsed pre-auth by the protocol's design, inside the sandboxed `vutric-compat` process with fuzzers (`quickshare_frames`, `quickshare_secure`). **Gap closed here:** frames were capped at 1 MiB from the first byte; now at **16 KiB** until the UKEY2 key exchange finishes (`conn::gated_reader`, `MAX_PREAUTH_FRAME`), on the receiver and the sender. | Every frame goes through the AES-CBC + HMAC channel with sequence numbers (`Channel::open`; replay and forged frames refused in `ukey2` tests); no plaintext frame is accepted after the handshake. | The 4-digit PIN from the UKEY2 auth string, which is what Android shows; a 6-digit exporter code is not possible in that protocol. Deviation. |

## S6: the privacy line

`Picker::privacy_lines()` returns the i18n keys to show under the send button, in order:

- `privacy.encrypted`: "Fayllar shifrlangan, faqat siz va qabul qiluvchi ko'radi". Always: every route is end-to-end
  encrypted, the relay sees only encrypted packets (ADR-0053).
- `privacy.nothing_leaves`: "Hech narsa internetga chiqmaydi". Only for "Faqat shu tarmoq", "Tarmoqsiz" and
  "Bluetooth", the choices that bind the endpoint with relays and lookup off (`Reach::for_picker`). With
  "Avtomatik" it would not be true, so it is not shown.

A live session can also use `Badge::crosses_internet()` to say which route it took.

## Measured

| Check | Result |
|---|---|
| `conn::tests::preauth_frames_are_small_until_the_gate_opens`: a 16 KiB + 1 frame closes the reader before the gate opens; after `Gate::authenticated` it arrives | pass |
| `tests/transfer.rs` (Quick Share receive and send, hostile senders) with the gate in place, 3 runs | pass |
| `route::tests::privacy_line_only_promises_what_is_true` | pass |

## Consequences

- A new route or adapter adds a row to the table above in its own ADR, before it ships.
- The two deviations (no 6-digit code in LocalSend and Quick Share) are inherent to those protocols; their
  adapters stay behind receive mode and the always-ask rule of ADR-0010.
