# ADR-0064: The listener knows how the dialer pairs: a link proof or the six digits

Status: accepted (2026-11-18) · Stream: W3 · Backlog: S8 · Issue: #13 (from W1, ADR-0013 decision 10) ·
Builds on: ADR-0050, ADR-0059, ADR-0061

## Context

`Session::pair_code` runs on both ends at once, and the dialer knows when it needs it: it found the device by
mDNS, a typed address or a short code. The listener does not. A device that scanned the QR and a device that typed
the short code both connect inside the pairing window and look the same. If the listener always runs
`pair_code`, it eats a QR dialer's first message; if it never does, a code dialer times out. W1's phone therefore
never runs it as listener and trusts any stranger admitted in its pairing window as `ticket` (owner question of
2026-11-18). That is the gap ADR-0050 exists to close: a stranger who finds the phone by its beacon during the
window could be trusted without any check.

W1 suggested a flag in `Hello` or a "peek" on the listener. A flag alone is not enough: anyone can set it to "I
scanned the QR". The listener needs proof that the dialer read its screen.

## Decision

1. **A one-time secret in the QR.** While the pairing window is open the listener adds `s=<32 hex>` (16 random
   bytes, `vutric_core::sas::LinkSecret`) to its pairing link: `pairing::with_secret(uri, &secret)`, for a plain
   link or a hotspot link alike. A new secret per window; it never leaves the screen otherwise. Older dialers
   ignore the key (`PairingTicket::parse` skips unknown keys).
2. **The dialer proves it.** `Peer::connect_link(uri)` parses the link, dials, and when the link has `s` sends
   `PairProof { proof }` right after `Hello`, `proof = BLAKE3-derive-key("vutric/1 2026-11-18 pairing link
   proof", x ‖ s)` with `x` the 32-byte TLS exporter of ADR-0050. Hosts that dial differently call
   `Session::prove_link(&secret)`.
3. **The listener asks once.** After admitting an unknown device in its pairing window, the host calls
   `Session::pair_intent(Some(&secret), PAIR_INTENT_WAIT)` (3 s) and gets:

   | `PairIntent` | First message from the dialer | The host |
   |---|---|---|
   | `Link` | a `PairProof` that matches the secret | trusts it as `ticket` |
   | `Code` | `PairCommit` (kept for `pair_code`) | runs `pair_code` + `pair_finish`, trusts as `code` only if both say yes |
   | `WrongLink` | a `PairProof` that does not match (an old QR, a forgery) | does not trust it; may say "scan the QR again" |
   | `Neither` | anything else (kept for the next `recv`), or silence for 3 s | does not trust it: a guest, every offer asked about |

4. **Default: nothing is trusted without a check.** `Neither` is never stored in the trust list. The owner question
   of 2026-11-18 (W1) is answered by this default; trusting such a peer would need a new owner decision.
5. **Session keeps what it read.** `Session` now holds a small queue (32 messages) that `recv` returns first. The
   listener's peek uses it, and so do the transfer loops (ADR-0065). The queue is internal; `recv` keeps its
   meaning for every caller.

## Why it holds

- The proof is bound to this TLS session by `x`, so it cannot be replayed on another connection, and a man in the
  middle who relays it gets a different `x` on each side.
- 128 bits of secret cannot be guessed inside a 10-minute window. Someone who photographs the QR can pair, as
  before: the QR is the authority, which is what `ticket` always meant.
- A code dialer is unchanged: `pair_code` sends `PairCommit` first, and the listener's `pair_code` takes the kept
  commitment.

## S5 row (ADR-0061)

| Route | Before authentication | After | Code |
|---|---|---|---|
| Vutric pairing link with `s` | unchanged: QUIC authenticates both keys first | one extra message, `PairProof`, 32 bytes, same TLS stream | not needed: the proof shows the dialer read the screen; the six digits stay for every other way in |

## Measured

| Check | Result |
|---|---|
| `crates/vutric-net/tests/pair_intent.rs::qr_dialer_proves_the_link` (loopback, two peers) | `Link` |
| `…::an_old_qr_is_not_trusted` (proof made with another secret) | `WrongLink` |
| `…::code_dialer_runs_the_check_and_the_listener_follows` (listener runs `pair_code` after `Code`) | same six digits, both confirmed |
| `…::neither_keeps_the_first_message_and_companion_messages_flow` | `Neither`, the first message still arrives |
| `…::silent_dialer_is_a_guest_after_the_wait` (400 ms wait) | `Neither` in under 2 s |
| `sas.rs` `link_proof_binds_secret_and_session`, `link_secret_hex`; `pairing.rs` `secret_rides_along_and_old_parsers_ignore_it` | pass |
| fuzz `pairing_ticket` also round-trips any secret `secret_in` accepts | smoke run pass |

Not verified: phone to phone on a real LAN (needs W1's change below and two phones).

## What W1 must change (issue #13)

- Listener (`accept_session` in `crates/vutric-mobile/src/api.rs`, the `TEMP until W3 #13` note): make a
  `LinkSecret` from 16 random bytes when the pairing window opens; show `pairing::with_secret(link, &secret)` in the
  QR and in the hotspot QR; after `accept_with` returns `Admission::Unknown`, call `pair_intent`. Store `ticket` on
  `Link`, run the six-digit screen on `Code`, store nothing on `WrongLink` and `Neither`.
- Dialer: dial a scanned or pasted link with `Peer::connect_link(text)` instead of `PairingTicket::parse` +
  `connect_ticket`, so the proof is sent. Nearby, typed and short-code dials stay as they are (they already run
  `pair_code` first).
- The owner question "(W1) 2026-11-18 · A stranger that connects while the phone shows its own QR…" is settled by
  decision 4.

## Consequences

- W2's daemon does the same as listener when it shows its QR.
- A QR printed or shared as a link keeps working only while that window's secret is current; an old QR gives
  `WrongLink`, and the UI asks for the current one.
