# Phase 0.2 report — any-to-any LAN transport

Status: **in progress.** The loopback numbers below prove the code path; the exit criteria need two devices on a
real Wi-Fi and are still open. Design: [ADR-0005](../decisions/0005-any-to-any-offline-first.md).

Date: 2026-11-17 · Code: `crates/vutric-net`, desktop CLI `apps/net-spike`, phone app `apps/mobile-spike`
(Android APK and an unsigned iPhone IPA from the `iOS spike` workflow).

## Exit criteria

| Criterion | Target | Result |
|---|---|---|
| Preview one-way, phone → desktop, home Wi-Fi, internet unplugged | p50 < 40 ms, p95 < 80 ms | open |
| Preview one-way, desktop → phone, same network | p50 < 40 ms, p95 < 80 ms | open |
| Pairing by link when multicast is blocked | connects with the link's IP:port | open on a device; passes on loopback |
| Phone ↔ phone, desktop ↔ desktop | connects, both directions | open |
| Both screens show the same six-digit check | equal | passes (integration test and loopback CLI) |

## Measured so far: one Windows machine, two processes (not a Wi-Fi number)

| Step | Result |
|---|---|
| mDNS discovery (`send --discover`) | 138 ms |
| QUIC connect + `Hello` both ways | 6 ms |
| 30 KB preview, RTT, n=100 each way | p50 0.2 ms, p95 0.4 ms |
| Pairing link | 179 characters, 4 LAN addresses (virtual adapters included) |

## How to reproduce

```powershell
cargo build -p vutric-net-spike --release
.\target\release\vutric-net-spike.exe listen --name "Studio PC"      # prints a QR and the pairing link
# second machine, or a second terminal:
.\target\release\vutric-net-spike.exe send --discover --name "Laptop"
```

Phone side: [apps/mobile-spike/README.md](../../apps/mobile-spike/README.md). Each run sends 3 warm-up and 100
measured probes one way, then 3 + 100 the other way, on one session.

## Real photos, every direction (built, waiting for the device run)

| Path | Thrower | Catcher | Number reported |
|---|---|---|---|
| phone → desktop | phone **Rasmni kompyuterga otish** | `vutric-overlay-spike --listen` (Halqa) | release → desktop first frame, there and back |
| desktop → desktop | `vutric-net-spike fling --ticket LINK --photo PATH` | `vutric-overlay-spike --listen` | same |
| desktop → phone | `vutric-net-spike fling --photo PATH` (shows a QR) | phone **Rasm qabul qilish** | arrival → photo on the phone screen |
| phone → phone | phone A **Rasmni kompyuterga otish** | phone B **Rasm qabul qilish** (its QR) | both of the above |

The catcher answers `EffectStarted` only after the frame that shows the photo was presented, so the thrower's
round trip is an upper bound of "release → first frame". Latency probes still work against every catcher.
The phone catch API is covered by `crates/vutric-mobile/tests/catch.rs` (two real peers in one process).

## Review fixes (before the device run)

An independent review of the 0.2 code found, and this branch fixed:

| Area | Was | Now |
|---|---|---|
| Phone API sessions | a blocking call took the session out of its slot; leaving a screen could not cancel it, and a stale call later replaced a newer session | sessions stay in the slot behind an async lock; an epoch cancels every wait at once (`disconnect`, `start`, a new session); stale calls never write; covered by `crates/vutric-mobile/tests/catch.rs` |
| Timeouts | none: a silent peer hung a throw, a handshake or the listener forever | handshake 8 s (a bad client is skipped, the listener keeps going), answer to a fling 5 s |
| Hostile previews | decoded with no size limit, on the network threads | `vutric_net::preview::decode`: JPEG only, at most 1024 px a side, 16 MB allocation cap, announced size must match, decoded off the network threads; refused with `FlingFailed` |
| Oversized previews | a noisy photo could exceed 96 KB and kill the session on the receiver | `preview::encode` steps quality down to fit 64 KB; `Session::send` validates before sending |
| Photo orientation | EXIF orientation ignored (portrait photos sideways) | applied when the preview is made |
| Close | `close()` right after a last send could drop it | `Session::finish` waits for the peer to take it (2 s at most) |
| Catch timing | `shown` was sent before the photo was decoded and drawn | the photo is decoded first, `shown` follows the frame that draws it |
| LAN only | global IPv6 addresses went into the QR | only private/unique-local IPv4/IPv6 (no link-local with a scope) |
| Identity file | any read error silently replaced the identity | only a missing file creates one; owner-only, atomic write |

A second, verifying review confirmed these and found a narrow race (a call could read the epoch after a
disconnect and keep a retired session): epochs now move and are read under the session-slot lock. It also
led to typed refusals (`probe::Refused`, not text matching), error messages without peer payloads, bidi
controls stripped from device names, and local validation before a throw so a bad name never drops a session.

Known small gaps, left for the real daemon: handshakes are accepted one at a time (a client that stalls for
8 s delays the next one); a desktop GPU that needs over 2 s to start refuses the first fling it then plays;
leaving a phone screen and tapping connect within a few milliseconds can cancel that connect (tap again).

**Open, by design for Phase 1:** a receiver accepts any device that reaches it, and the six-digit check is shown
but not confirmed. QR/link pairing pins the peer's key, so it is safe against an impostor; the mDNS path is not
(an active attacker can grind a 20-bit code). Phase 1 adds a trust store of paired keys (unknown devices need an
explicit OK) and commit-then-reveal numeric comparison before mDNS pairing is offered to users.

## Found on the way

- **Windows marks many Wi-Fi networks as Public.** Inbound UDP to the listener is then blocked until the user
  allows the app in the Windows Firewall prompt (or marks the network Private). The real daemon needs a firewall
  rule from its installer for the Private profile, and a clear first-run hint for Public networks.
- **iOS refuses multicast** without Apple's entitlement, which a free Apple ID cannot get. `vutric-net` now keeps
  running without mDNS and pairs by QR link; discovery on iOS moves to `NWBrowser` in the real client.
- **Virtual adapters** (Hyper-V vEthernet, VPNs) land in the pairing link next to the real Wi-Fi/Ethernet address.
  Harmless (iroh tries all of them) but they make the QR denser; rank physical interfaces first later.
