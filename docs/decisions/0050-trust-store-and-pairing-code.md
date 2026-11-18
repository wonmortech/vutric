# ADR-0050: Trust store, a gate before the first byte, and a pairing code that cannot be ground

- Status: accepted, implemented (backlog S1, S2, S3)
- Date: 2026-11-18
- Builds on: [ADR-0005](0005-any-to-any-offline-first.md) part 3, [ADR-0007](0007-every-route-and-storage-settings.md)
  part 2. Closes the "Open, by design for Phase 1" item of the [0.2 report](../reports/phase-0.2-network.md).

## Context

Spike 0.2 accepted any device that reached the listener, and showed a six-digit code (`TLS exporter mod 10^6`)
that nobody confirmed. A man in the middle who answers mDNS can grind that code: he runs a TLS handshake with each
victim and, as the server of one of them, retries his key share until both codes match (about 2^20 tries,
seconds of CPU). QR or link pairing is safe, because the ticket pins the key; pairing from the nearby list is not.

ADR-0007 part 2 also asks that only trusted devices may send by default, and that "everyone" turns itself off
after 10 minutes, as `AirDrop` does.

## Decision

1. **Trust store** (`vutric_core::trust::TrustStore`, file `trusted.json` via `vutric_net::trust`).
   - Keyed by the endpoint key only. A name or an address never makes a device trusted; seeing a device never
     adds it.
   - Each entry keeps the name, platform, how it was paired (`ticket` > `code` > `pinned`), `paired_at`,
     `last_seen`. Re-pairing upgrades the method, never downgrades it.
   - At most 512 devices; one more forgets the one seen longest ago.
   - A damaged file is moved to `.bad` and the store starts empty: pairing again is safe, guessing is not.
     Owner-only file mode on Unix, atomic write.

2. **Visibility and one admission rule** (`vutric_core::trust::{Visibility, admit}`).

   | Visibility | Trusted device | Unknown device |
   |---|---|---|
   | `off` (pause) | refused | refused |
   | `trusted_only` (default) | in | refused |
   | `everyone(until)` | in | in, every offer asked about |
   | `pairing(until)` (QR or code on screen) | in | in, to pair |

   - Both windows last at most 10 minutes (`EVERYONE_FOR`). A window ending further ahead than that counts as
     expired, so a clock set back or an edited settings file cannot keep "everyone" on.
   - `Peer::accept_with(admit)` asks after QUIC has authenticated the key and **before** the control stream is
     read. A refused connection is closed with application code `403`; the dialer gets the typed error
     `NotAdmitted`, so the UI can say "pair first, or ask the other device to open Hamma for 10 minutes".
   - `Peer::accept()` (accept anyone) stays for the spikes; hosts use `accept_with`.

3. **Commit-then-reveal pairing code** (`vutric_core::sas`, `Session::pair_code` and `Session::pair_finish`).
   - `x` = 32 bytes of TLS exported keying material, label `EXPORTER-vutric/1 pairing sas`.
   - The dialer sends `PairCommit { H(x, a) }`. The listener answers `PairNonce { b }`, and only then the dialer
     sends `PairReveal { a }`. The listener checks the commitment; a mismatch or a message out of order is the
     typed error `PairingTampered`.
   - Both screens show `code(x, a, b)`: six digits from BLAKE3 in derive-key mode with separate contexts for the
     commitment and the code.
   - Each user confirms; `PairResult { confirmed }` goes both ways. The host stores the peer with
     `PairMethod::Code` only when both said yes.
   - Why it holds: the attacker is the listener on one connection and the dialer on the other. As listener he
     must pick `b` before he learns `a`; as dialer he is bound to `a` before he learns `b`. So each attempt
     matches with probability 10^-6, and each attempt costs a visible pairing on both screens. This is the scheme
     of Bluetooth Secure Simple Pairing numeric comparison.

4. **Wire.** Four messages appended to `Message` (`PairCommit`, `PairNonce`, `PairReveal`, `PairResult`).
   Postcard indices of the older variants are unchanged, so nothing else on the wire moves. A spike build that
   predates them fails the pairing check with a decode error, never silently.

## Consequences

- W1 and W2 keep the store and the visibility in their hosts and pass `admit` into `accept_with`. Phone and
  desktop must show the same three states (pause, trusted only, everyone with a countdown) and the same pairing
  screen with six digits and Yes / No.
- `Accept::auto(trusted, …)` in the storage settings now has a real source for `trusted`: the store.
  An `Unknown` admission is never auto-accepted (ADR-0010 rule 2 holds for our own protocol too).
- The old `Session::check_code` stays for QR pairing, where the key is already pinned; the UI must not present
  it as a security check for nearby pairing.
- Tests: `crates/vutric-core/src/trust.rs` and `sas.rs` unit tests (expiry at exactly 600 s, clock-skew guard,
  method upgrade, eviction, 96 single-bit changes, digit spread over 10 000 codes);
  `crates/vutric-net/tests/trust_gate.rs` end to end (unknown refused with `NotAdmitted`, paired by code,
  then admitted as trusted; one user saying no stores nothing).
