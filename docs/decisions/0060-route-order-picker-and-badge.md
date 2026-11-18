# ADR-0060: Route order, route picker and the route badge in the engine

Status: accepted (2026-11-18) · Stream: W3 · Backlog: Rx · Builds on: ADR-0006 part 2, ADR-0007 parts 1 and 3

## Context

ADR-0007 fixed the "Avtomatik" order (same network, internet direct, our relay, then hotspot, Wi-Fi Aware or
cable offered to the user), a picker that forces one route, and a badge that shows the route type, never a
promise. Each screen (phone, desktop) would otherwise re-derive this. The engine owns it, so both behave alike.

## Decision

`vutric_core::route` (pure) and two additions in `vutric-net`, all additive:

| Piece | What it does |
|---|---|
| `Picker` | `Auto`, `LanOnly`, `Internet`, `Offline`, `Bluetooth`; i18n keys `route.picker.*`; `Settings.route`, default `Auto`. An unknown value written by a newer version reads as `Auto` (`route::lenient`), so the settings file never fails to load over it. |
| `Picker::plan(Situation)` | The steps, best first: `SameNetwork`, `InternetDirect`, `Relay`, then `Offer(Hotspot / WifiAware / Cable)` for what the device can start. `LanOnly` = same network only; `Internet` = direct then relay, no LAN search; `Offline` = offers only. Empty plans are `NoRoute::Unavailable`; `Bluetooth` is `NoRoute::NotBuilt` until R4. |
| `Reach::for_picker` | `Anywhere` only for `Auto` and `Internet`; the others bind with relays and lookup off, so nothing leaves the LAN. |
| `Badge` + `lan::badge` | `DirectLan`, `DirectInternet`, `Relay`, `Hotspot`, `Cable`, `Connecting`; keys `route.badge.*`; `crosses_internet()` drives the privacy line (S6). Relayed path = Relay. A direct path to one of our links is LAN, except a tethering link (Hotspot), a USB tether (Cable: names with `usb`, `ndis`, `iphone`, `apple mobile`), or a wired link where both ends are self-assigned `169.254/16` / `fe80::/10` (two computers on one cable, Cable). Another private address (another VLAN) is LAN; loopback is LAN; CGNAT `100.64/10` and public addresses are internet. |
| `Session::badge` / `badge_with(locals)` | The badge of the selected iroh path right now. `badge_with` takes the address list, so a UI that refreshes every second does not ask the OS every time. |

With `Internet`, the caller dials by key without LAN hints (`connect(id, &[])`); iroh may still upgrade to a LAN
path if one exists, and the badge then says so honestly.

## Measured

| Check | Result |
|---|---|
| `route::tests`: ADR order with everything available; a field with only a hotspot offers only the hotspot; forced choices stay inside their route; empty plans say why; serde names stable; newer value reads as `Auto` | pass |
| `lan::tests::badges_follow_the_link`: Wi-Fi, LOHS (`192.168.43.x`), Windows "Remote NDIS" USB tether, `169.254` cable, other VLAN, CGNAT, public, loopback | pass |
| `tests/route_badge.rs`: two `Reach::Lan` peers on one machine show `DirectLan` on both ends once the path is chosen (3 runs, 0.09 s each) | pass |

Not verified: Hotspot and Cable badges on real devices (the classification is tested on interface data only).

## Consequences

- W1 and W2 build the picker row and the badge from these keys; they never parse addresses themselves.
- A new route (R4 BLE, R6 Aware, R7 Wi-Fi Direct) adds a `Step` / `Offer` / `Badge` variant here, then its host
  code. Variants are added, never renumbered.
