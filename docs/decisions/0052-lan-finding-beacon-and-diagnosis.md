# ADR-0052: LAN finding: ranked QR addresses, a beacon, and a verdict when nothing answers

Status: accepted (2026-11-18) · Stream: W3 · Backlog: R1, R1a · Builds on: ADR-0005, ADR-0007

## Context

mDNS alone misses real networks: guest, hotel and office Wi-Fi often filter 5353 or isolate clients, and
Windows machines carry Hyper-V, WSL, Docker and VPN adapters whose addresses a phone can never reach. Until now the
QR took the first 8 LAN addresses sorted by IP, so `172.x` virtual switches came before the `192.168.x` Wi-Fi
address, and link-local IPv6 was dropped entirely (its zone index means nothing on the other machine). When a
connection failed, the app had no way to tell the user why.

## Decision

1. **Ranked addresses** (`vutric_net::lan`). Every up, non-loopback interface address in LAN scope (private and
   link-local IPv4, carrier-grade NAT `100.64/10`, unique-local and link-local IPv6) is classified by interface:
   wired, Wi-Fi, tethering (`172.20.10/24`, `192.168.42-43/24`, `192.168.137/24`, rndis/usb/ap names), other,
   virtual (vEthernet, Hyper-V, WSL, Docker, VMware, VirtualBox, Tailscale, ZeroTier, WireGuard, tun/tap, VPN)
   last. The QR keeps the best 8.
2. **Link-local IPv6 without a zone.** The ticket carries `fe80::...` bare; the dialer turns each such hint into
   one candidate per local interface that has a link-local address (at most 32 candidates). This makes two
   computers on one cable (R5a) or one switch without DHCP connect.
3. **A beacon** (`vutric_core::beacon`, `vutric_net::beacon`): a fixed 41-byte UDP datagram on
   `224.0.0.167:53319` (link-local multicast, which access points forward without IGMP snooping; the group is
   `LocalSend`'s, the port is ours). Announce and query, multicast on every IPv4 interface, unicast for the probe
   and the opt-in `/24` scan (paced at 2 ms, at most 1024 hosts). It carries only the endpoint key, the QUIC port
   and a phone flag. Anything that is not exactly 41 bytes of our version is dropped unread; answers are limited
   to one per source per 500 ms; a device that is not visible (`Visibility::Off`) does not answer.
4. **A verdict** (`lan::diagnose`): `LocalNetworkDenied` (iOS permission, or a "permission denied" send),
   `Isolated { internet }` (the peer's hint is on our subnet but the probe got no answer), `OtherNetwork {
   internet }`, `NoNetwork`. Each has a stable i18n key (`lan.unreachable.<key>`) and an ordered list of next
   routes (`FixPermission`, `Relay`, `Hotspot`, `Cable`) that the UI offers. This is R1a's "this network keeps
   devices apart".

## Why not

- **A name or device type in the beacon:** more to parse before authentication, and it leaks the name to
  everyone on the network. The name arrives in `Hello`, after the key is checked.
- **Our own multicast group outside `224.0.0.0/24`:** routers with IGMP snooping drop it unless something joins
  through IGMP first. Sharing the group with `LocalSend` costs nothing; the port and the magic keep them apart.
- **Scanning by default:** a `/24` sweep looks like a port scan to corporate IDS. It runs only while the "add
  device" screen is open and the user asked for it.
- **TCP or HTTP probes (`LocalSend`'s fallback):** one UDP datagram each way is enough and opens no listener.

## Measured (Windows 10 laptop, Wi-Fi + Hyper-V + WSL + Tailscale)

| | before | after |
|---|---|---|
| First QR hint | a vEthernet `172.x` address | the Wi-Fi address |
| Interface enumeration | n/a | 9 addresses in 2.7 ms |
| Beacon multicast | n/a | sent on every IPv4 interface, no denied sends |

`cargo run -p vutric-net-spike -- lan --lan-only` prints the same table on any machine.

## Left open

- iOS: the host still has to report `NWBrowser` "policy denied" into `Evidence::local_network_denied` (W1).
- A real AP-isolated network and a captive portal: field check with the spike's `lan` command (backlog R1 stays
  `[~]` until then).
