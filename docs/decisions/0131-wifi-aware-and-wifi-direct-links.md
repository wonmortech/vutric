# ADR-0131: Wi-Fi Aware and Wi-Fi Direct links on Android

- Status: accepted, implemented in Kotlin (stream W6). The engine side is open: W3 #23. Compiles; nothing has been
  tried on a real phone yet.
- Date: 2026-11-18
- Builds on: [ADR-0007](0007-every-route-and-storage-settings.md), [ADR-0009](0009-native-modules-kotlin-swift.md)
  part 2 (`LinkManager`), [ADR-0058](0058-hotspot-in-the-pairing-link.md), [ADR-0130](0130-w6-share-shortcuts-and-transfer-job.md).
- Backlog: R6 (Android 8+ part), R7 (Android ⇄ Android, Android ⇄ PC as a legacy client), F4.

## Context

The hotspot (R3) already gives "no router, no internet". Two more Android radios do the same without turning the
phone into an access point: Wi-Fi Aware (NAN) and Wi-Fi Direct (P2P). Both end with a socket on a network that
iroh's own UDP socket does not reach by itself (Aware's link-local IPv6 lives only on the Aware `Network`).

`vutric-net` cannot run a session over a socket the host opened (checked on `spike/0.2-network` dfd1627 and
`w3/net-3` d4ab2e0). W3 #23 asks for `Peer::attach_stream(TcpStream, LinkKind, LinkRole, expect)` and the badges
`Aware` / `Direct`.

## Decision

| # | Decision | Why | Where |
|---|---|---|---|
| 1 | Both links end in a **connected TCP stream**, detached from Java (`ParcelFileDescriptor.fromSocket(...).detachFd()`) and handed to `LinkSockets.sink`, which owns the descriptor from then on | The same hand-over as the `:compat` socket pair (ADR-0066); Rust gets one int | `LinkSockets.kt` |
| 2 | **TEMP until W3 #23:** the default sink closes the socket and sends `w6.link` `needs_w3`. `w6.linkSink` `dart` switches to passing the fd in the event, for W1's `attach_link_fd` bridge once #23 lands | No descriptor leaks while the engine cannot use it; the UI can say "not ready yet" | `LinkSockets.closeAndReport`, `toDart` |
| 3 | **Wi-Fi Aware:** the listener publishes service `vutric` with up to 255 bytes of service info chosen by the caller (W3's codec), waits for a hello, opens a `ServerSocket(0)`, and answers the data path with `setPort`. The dialer subscribes, says hello, requests the data path, reads the peer IPv6 and port from `WifiAwareNetworkInfo` and connects through `network.socketFactory` | The documented NAN socket flow; the link-local address is only reachable through that `Network` | `AwareLink.kt` |
| 4 | Aware is offered on **Android 10+** (`WifiAwareNetworkInfo` is API 29). Discovery alone would work from 8, but without a socket it gives the user nothing | Honest support line; R6's "Android 8+" becomes "8+ hardware, 10+ to use" | `AwareLink.supported` |
| 5 | The Aware data path takes a PSK (8-63 printable ASCII) when the caller gives one, for example derived from the pairing secret; otherwise it is open. The Vutric handshake on top authenticates and encrypts either way | A PSK sent in a discovery message would be readable on the air, so Kotlin never invents one | `LinkRules.psk` |
| 6 | **Wi-Fi Direct:** the host creates a group with **our own name and passphrase** (`WifiP2pConfig.Builder`, Android 10+), so neither phone shows the system "accept connection" dialog. The answer has W1's hotspot shape `{ssid, password, security: wpa2}` plus `port` | The pairing link and the `WIFI:` QR of ADR-0058 carry it unchanged; a **Windows or Linux PC joins it as an ordinary Wi-Fi network** (legacy client), which covers "Android ⇄ Windows" without a Windows P2P stack | `DirectLink.host` |
| 7 | The group owner listens on port **47613** (a free port if taken, returned as `port`); the joiner connects to the group owner address with up to 10 short retries | The group owner's socket may come up a moment after the group; a fixed default keeps old links working | `LinkRules.DIRECT_PORT`, `DirectLink.connect` |
| 8 | Group name `DIRECT-xy-Vutric`, 16-character passphrase from `SecureRandom`; both checked against the P2P builder rules before the OS sees them | `WifiP2pConfig.Builder` throws on a bad name, and the OS answers that late | `LinkRules.directCredentials`, `validDirect` |
| 9 | The group owner address is on Android's local-network route, so iroh's UDP socket may reach it **without** the stream. W3 #23 lets W3 choose; the real-phone run (checklist 4) decides | Fewer moving parts if the plain path works | — |
| 10 | Permissions: the hotspot's (`NEARBY_WIFI_DEVICES` `neverForLocation` on 13+, fine location on 10-12). No new ones | ADR-0012 decision 6 | `W6Permissions` |

## What W1 wires

1. Route picker "Tarmoqsiz" offers Wi-Fi Aware when `w6.awareAvailable` is true, and Wi-Fi Direct when the phone has
   it, next to the hotspot.
2. Direct host: put `w6.directHost`'s answer into `hotspot_link` (same fields) and show the QR; joiner: `w6.directJoin`
   with the scanned fields.
3. After W3 #23: `w6.linkSink` `dart`, then on `w6.link` `connected` call `attach_link_fd(fd, link, role, peer)`.

## Verified

- `LinkRulesTest` (3): PSK rules, 200 generated Direct credentials pass the builder rules, bad names are refused.
- Not verified: everything that needs two phones (below).

## Device checklist (owner, two Android 10+ phones)

- [~] 1. Phone A Aware publish, phone B subscribe: both log `w6.link` `needs_w3` (today) within 10 s, with Wi-Fi on and no router.
- [~] 2. Same with a PSK on both: still connects; with different PSKs: `failed` `data_path` on B.
- [~] 3. Phone A Direct host: no system dialog on A or B; B joins from the QR; `needs_w3` on both.
- [~] 4. A PC joins phone A's Direct group from the `WIFI:` QR as a normal Wi-Fi network and gets a `192.168.49.x` address; `vutric-net-spike` on the PC reaches the phone through the existing iroh path (tells W3 whether decision 9 holds).
- [~] 5. Phone A as hotspot (R3) and Aware at once: Aware reports `unavailable` (expected on most chips).
- [~] 6. After W3 #23: a throw lands over Aware and over Direct; measure MB/s for W3's M4 row.

## Consequences

- R6 and R7 have their Android halves; the backlog rows stay `[~]` until #23 and the device run.
- iPhone Aware (iOS 26) and Windows' own Wi-Fi Direct API stay open (R6 iOS part, R7 Windows host).
