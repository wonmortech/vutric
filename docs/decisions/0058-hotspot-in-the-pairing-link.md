# ADR-0058: The hotspot travels in the pairing link (R3 core)

Status: accepted (2026-11-18) · Stream: W3 · Backlog: R3 · Builds on: ADR-0007 (route R3), ADR-0052

## Context

With no shared network (no router, a guest Wi-Fi that isolates clients, a field with no Wi-Fi at all), one device
opens a hotspot: an Android phone through `LocalOnlyHotspot`, a Windows PC through its mobile hotspot
(`NetworkOperatorTetheringManager`). The other device must join that network and then reach the Vutric endpoint
on it. Typing a random SSID and password is the step people get wrong, so ADR-0007 puts both into the same QR as
the pairing ticket. Opening and joining hotspots is platform code (W1 `LinkManager`, iOS
`NEHotspotConfiguration`, W2 on Windows). This ADR fixes only the shared payload, owned by `vutric-core`.

## Decision

- `vutric_core::hotspot` adds four optional keys to the existing link, so one QR carries both:
  `vutric://pair?v=2&id=…&n=…&a=…&ws=<ssid>&wk=<wpa2|wpa3|open>&wp=<password>&wh=1`.
  - `ws` SSID and `wp` passphrase are percent-encoded like `n`, so the link stays ASCII.
  - `wk` is required with `ws`; an unknown value (`wep`, typos) is an error, not a guess.
  - `wh=1` marks a hidden network.
- Old apps already ignore unknown keys, so a hotspot link still pairs on them; the user just joins the network by
  hand. Nothing in `PairingTicket` changes, and the protocol version stays 2. The change is additive.
- `Wifi::new` admits only networks a phone can join: SSID of 1–32 bytes with no control characters; WPA2/WPA3
  with an 8–63 character printable-ASCII passphrase or a 64-hex raw key; open networks without a password.
- `Wifi::in_link` returns `None` when the link has no hotspot, and an error when it has a half one (an SSID
  without `wk`, a password without an SSID). A broken QR is refused rather than sending the user to a network
  that cannot work.
- `Wifi::to_wifi_qr` gives the standard `WIFI:T:WPA;S:…;P:…;H:true;;` payload with `\ ; , : "` escaped. A host
  can show it as a second QR, so a phone without Vutric joins with its camera app first.
- `Wifi`'s `Debug` prints `<hidden>` instead of the password, so a logged ticket does not leak it.
- The address hints (`a`) are the hotspot host's own addresses, already ranked by ADR-0052 (tethering subnets
  such as `192.168.43.x` and `192.168.137.x` included), so the joining device dials at once without mDNS.

## Security

The password is as secret as the QR on the screen, which is also true of the ticket itself. `LocalOnlyHotspot`
generates a new password every time it starts, so a photographed QR is useless after the session. The link is
never published to lookup or relay. The connection on the hotspot is the same iroh QUIC/TLS 1.3 session as on any
LAN, with the same trust gate (ADR-0050); joining the hotspot grants nothing by itself.

## Measured

| Check | Result |
|---|---|
| `hotspot::tests::round_trip_and_old_parsers_still_pair`: a WPA2 hotspot with `&`, `=`, `;` and space in the password round-trips; `PairingTicket::parse` of the same link equals the plain ticket | pass |
| `hotspot::tests::unicode_hidden_and_open`: Unicode SSID, hidden, open network | pass |
| `hotspot::tests::refuses_networks_a_phone_cannot_join`: empty, 33-byte and control-character SSIDs; short and 64 non-hex passphrases; open with a password; WPA without; five half/broken links | pass |
| `hotspot::tests::standard_wifi_qr_escapes`, `debug_hides_the_password` | pass |
| fuzz `pairing_ticket` (S4) now also checks the hotspot part: limits hold, own link round-trips, the old parser still pairs | smoke run pass; libFuzzer runs in CI |

## Consequences

- W1 builds the Android side: start `LocalOnlyHotspot`, put its `SoftApConfiguration` into a `Wifi`, show the
  link; on scan, join with `WifiNetworkSpecifier` (Android) or `NEHotspotConfiguration` (iOS, I5). Joining from a
  Windows, Linux or macOS desktop and hosting from Windows are W2.
- Left for real devices: LOHS on MIUI/HarmonyOS (some builds do not offer it; fallback is the system hotspot with
  the user typing its name and password into the same `Wifi`), iOS falling back to a saved network, and the
  2.4 versus 5 GHz speed (M3).
