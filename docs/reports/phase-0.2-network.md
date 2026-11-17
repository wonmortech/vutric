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

## Found on the way

- **Windows marks many Wi-Fi networks as Public.** Inbound UDP to the listener is then blocked until the user
  allows the app in the Windows Firewall prompt (or marks the network Private). The real daemon needs a firewall
  rule from its installer for the Private profile, and a clear first-run hint for Public networks.
- **iOS refuses multicast** without Apple's entitlement, which a free Apple ID cannot get. `vutric-net` now keeps
  running without mDNS and pairs by QR link; discovery on iOS moves to `NWBrowser` in the real client.
- **Virtual adapters** (Hyper-V vEthernet, VPNs) land in the pairing link next to the real Wi-Fi/Ethernet address.
  Harmless (iroh tries all of them) but they make the QR denser; rank physical interfaces first later.
