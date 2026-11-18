# ADR-0068: captive portals, big files over the relay, cable hints in the QR

Status: accepted (2026-11-18) · Stream: W3 · Backlog: R1a, Rb, R5a, R2 · Builds on: ADR-0052, ADR-0053, ADR-0060

Four leftovers of the routing work that need no extra hardware to build. Each one ends at a real-network step.

## 1. Captive portals (R1a)

**Context.** ADR-0052 explains a failed connection with `lan::diagnose`: local network denied, isolated, other
network, or no network. A hotel or guest Wi-Fi that shows a login page fits none of these. Both the LAN path and
the relay fail, and the useful screen is "sign in to this Wi-Fi first".

**Decision.**

- `vutric_core::captive::classify` reads the answer to `GET /generate_204`:
  - `204`, or `200` with an empty body: the internet is open.
  - a redirect, or any other answer, or bytes that are not HTTP: a portal.
- A redirect's `Location` is passed on as the login link only when it is `http(s)`, printable ASCII, and at most
  512 bytes. At most 8 KiB of the answer is read.
- The parser is pure and fuzzed (`captive_answer`).
- `vutric_net::captive::check(host, wait)` sends the request with a time bound. A silent server or no route gives
  `Offline`, not "portal".
- **The check host is our own relay.** `iroh-relay` already serves `/generate_204` over plain HTTP: port 80 when
  it uses TLS, its own port otherwise. `captive::host_for_relay(url)` gives the host, so no third-party server
  (Google, Apple) is asked. This also closes the owner question the first draft of this ADR would have added.
- `lan::Evidence.captive`, the new verdict `Unreachable::Captive` (key `captive`) and the new route
  `NextRoute::SignIn` (offered first, then hotspot and cable).
- A portal is the verdict only while the LAN probe is unanswered. If the LAN path works, the portal is not why
  the connection failed.

API changes, additive: new variants and one new field. Nothing outside W3 matches on `Unreachable`, `NextRoute` or
builds `Evidence`; checked with a search of `apps/` and `crates/` on 2026-11-18.

## 2. Big files and the relay (Rb)

**Context.** TM §2.6 says never to push 10–100 GB through the relay. The relay is shared and slow, and it costs
the operator.

**Decision.**

- `vutric_core::route::Bulk::of(relayed, total)` returns one of three verdicts:
  - `Go`: direct, or at most 2 GiB;
  - `Ask`: relayed and above 2 GiB. The UI asks, and offers `Bulk::better()` = hotspot or cable.
  - `TooBig`: relayed and above 10 GiB.
- `transfer::send` enforces `TooBig` itself, before anything is offered. A relayed start often turns direct after
  hole punching, so it first waits `DIRECT_WAIT` (10 s) for a direct path. If none comes, it fails with
  `TooBigForRelay`, a typed error next to `Declined` and `NoRoom`.
- The `Ask` verdict is the UI's job, through `Session::bulk(total)`. The new helpers `Session::is_relayed`,
  `wait_direct` and `remote_ip` are additive.

**iroh-blobs: not adopted.** Our transfer already verifies every file (BLAKE3 over the whole file, checked before
the rename) and resumes from the part file (ADR-0006 part 3). iroh-blobs would add verified *streaming* (each
16 KiB chunk proven as it arrives), plus a second wire protocol and a store. The gain is earlier detection of a
corrupt chunk on a hostile relay. QUIC already authenticates every packet end to end, so a relay cannot corrupt
data undetected. Revisit if we add multi-source download (one file from several devices), which is where
iroh-blobs is strong.

## 3. Cable hints in the QR (R5a)

**Context.** Two computers on one Ethernet, Thunderbolt or USB4 cable get self-assigned addresses only:
`169.254/16` and `fe80::`. `lan::badge` already shows such a link as "Kabel", and `lan::expand_link_local` turns
a zone-less `fe80::` hint into one candidate per local interface.

The interop check found that **the QR never carried those addresses.** iroh's interface list (netwatch
`local_addresses`) always leaves out `fe80::`. It also leaves out `169.254` whenever the machine has any other
address. A laptop that is on Wi-Fi as well as the cable therefore put no cable address in its QR.

**Decision.** `lan::self_assigned_addrs(bound, locals)` pairs every self-assigned address of our interfaces with
the port of the socket bound to the unspecified address of the same family. `Peer::ticket` adds these addresses
before ranking, so wired links still come first and `MAX_HINTS` still caps the list. The QR may now be a little
longer; the cap is unchanged.

## 4. The relay page (R2 follow-up)

`docs/relay.md` now names the `/generate_204` request and the 2 GiB / 10 GiB rule, so what the relay operator
can see stays complete.

## Not verified

1. A real captive portal: hotel, airport, or a phone hotspot with a login page. Only local servers that answer
   like one were used.
2. A real relay-only network for the `TooBigForRelay` refusal. On one host iroh goes direct at once, so the
   refusal is tested with a stand-in "is relayed" check (`transfer::tests`). The real session test shows the
   other half: the verdict follows the live path.
3. A real cable between two computers. The `fe80::` test uses this machine's own interfaces: Wi-Fi, Hyper-V and
   WSL. The `169.254` half was not exercised on a live session; this host has no such address.
4. Linux CI runners may have no link-local IPv6. The R5a test then prints `skipped` and passes.

## Measured

| Check | Result |
|---|---|
| `vutric_core::captive` unit test: 204/200-empty open, redirect with login link, page, unsafe or long `Location`, garbage | pass |
| `vutric_net::captive` tests: local server answering 204 and 302, closed port, silent server stopped by the time bound (< 2 s for a 300 ms wait), relay URL to check host | 3 pass |
| `vutric-relay/tests/relay_path.rs::our_relay_answers_the_captive_check`: `check` against a real `vutric-relay` | `Open` |
| `lan::tests::verdicts`: portal verdict, and no portal verdict when the LAN probe answered | pass |
| `route::tests::bulk_limits_only_apply_to_relayed_paths`, `transfer::tests::relay_guard_waits_for_a_direct_path_then_refuses` (refuses after the wait; allows when the path turns direct after 250 ms) | pass |
| `relay_path.rs::bulk_verdict_follows_the_live_path`: ticket with the relay only, the session is direct on loopback and a 30 GiB verdict is `Go` | pass |
| `lan::tests::self_assigned_addresses_join_the_hints` | pass |
| `route_badge.rs::zone_less_link_local_hint_connects`: the ticket keeps only zone-less `fe80::` hints; the dialer connects and the live path's remote address is `fe80::2638:…` (Wi-Fi link) | pass, 3 runs; before the fix, the ticket had no IPv6 hint at all (`[192.168.100.126, 172.24.112.1, 172.26.144.1, 100.77.9.81]`) |
| fuzz smoke `captive_random_and_near_valid` | pass |
