# ADR-0059: Connect by a typed address or a short code

Status: accepted (2026-11-18) · Stream: W3 · Backlog: R11, A2 (W1) · Issue: #9 · Builds on: ADR-0050, ADR-0052

## Context

When the camera cannot scan the QR (broken camera, a desktop pairing with a desktop, a screen across the room),
the user needs something to type. `Peer::connect` needs the endpoint id, a 64-hex string nobody types. W1 asked
for an additive function (issue #9). Two shapes were on the table: a short code that resolves to the id, or an
unauthenticated hello on the listening port that returns the id.

## Decision

Both, on the beacon that already exists (ADR-0052), with no new wire format:

- **Short code** `vutric_core::code::ShortCode`: the first 40 bits of the endpoint id, eight Crockford base32
  characters shown as `ABCD-EFGH`. Parsing forgives case, spaces and dashes, `O` for `0`, `I`/`L` for `1`.
- **Typed address**: an IPv4 address, with or without a port. The beacon answers on its own port, so the typed
  port is ignored and the announced QUIC port is used.
- `vutric_net::find::Target::parse(text)` reads either; `find::resolve(beacon, target, beacon_port, wait, scan)`
  returns `Found { id, addr, phone }` or `NotFound::{Silent, Denied}` with i18n keys `find.silent` /
  `find.denied`. An address gets a unicast query; a code gets a multicast query and, when the "add device" screen
  allows it, one subnet scan. Queries repeat every 700 ms (above the 500 ms answer limit) until `wait` ends.
- The answer is **not authentication**. The caller connects with `Peer::connect(found.id, &[found.addr])`, the
  other side admits it only inside its pairing window, and both run the 6-digit commit-then-reveal comparison
  (`Session::pair_code` / `pair_finish`). Only then is the device trusted, as `PairMethod::Code`.
- IPv6 is not accepted as typed input: the beacon is IPv4, and link-local IPv6 needs a zone the user cannot know.

## Why 40 bits

A stranger on the same LAN who wants to answer for a given code must find an Ed25519 key with that 40-bit
prefix: about 2^39 key generations on average, inside the 10-minute pairing window. Even then the 6-digit
comparison would fail, because the stranger cannot make both screens show the same digits. Eight characters is
what people retype without mistakes; more bits would only matter if the code were the only check.

## Measured

| Check | Result |
|---|---|
| `tests/find_typed.rs::typed_address_finds_the_device`: loopback, id and QUIC address found | pass |
| `tests/find_typed.rs::silence_ends_in_time`: no listener and a hidden device both end in `Silent` within the wait (+ 500 ms) | pass |
| `tests/find_typed.rs::code_only_accepts_its_own_device`: a code never takes another device's answer | pass |
| `code.rs` unit tests: known value `00 44 32 14 c7` = `0123-4567`, forgiving input, wrong lengths and letters refused | pass |
| fuzz `pairing_ticket` also round-trips any text `ShortCode::parse` accepts | smoke run pass |

Not verified: a code found over multicast on a real Wi-Fi (multicast does not loop back on one host).

## Consequences

- W1 and W2 show the address and the code under the QR, and offer one text field that takes a link, an address
  or a code. W1 wraps `find` in `vutric-mobile` (its crate).
- A network that blocks both multicast and unicast between clients finds nothing here either: the screen then
  offers the full link, the relay or a hotspot (ADR-0052 `next_routes`).
