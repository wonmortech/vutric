# ADR-0005: Every device is a peer, and the local network is enough

- Status: accepted (design); spike 0.2 measures it
- Date: 2026-11-17

## Context

The first brief was phone → desktop only. The owner widened it on 2026-11-17:

- transfers go both ways: desktop → phone as well as phone → desktop;
- phone → phone and desktop → desktop work too;
- everything works on a shared network with **no internet**: router unplugged from the WAN, a trip, a classroom.

What others do:

| Product | Discovery | Transport | Without a shared network |
|---|---|---|---|
| LocalSend (open source) | UDP multicast `224.0.0.167:53317`, HTTP subnet scan as a fallback | HTTPS, self-signed | no |
| KDE Connect | UDP broadcast | TLS over TCP | no |
| Quick Share, AirDrop | BLE | Wi-Fi Direct or hotspot, AWDL | yes, but closed and per-vendor |

## Decision

1. **One core on every device, symmetric protocol.**
   - The same Rust core runs on every device. It holds an iroh endpoint, persistent Ed25519 identity and a trust list.
   - "Phone" and "desktop" are no longer protocol roles. Every transfer has a *sender* and a *receiver*.
   - `FlingStart` may come from either side. `EffectStarted` carries `receiver_micros`.
   - Wire format is unchanged; only names and docs change.

2. **Offline first. No server is used, so no internet is needed.**
   - The endpoint is built with the `Minimal` preset, relays disabled, no DNS/pkarr lookup.
   - Peers are found by:
     1. mDNS (`iroh-mdns-address-lookup`);
     2. IP:port hints carried in the pairing ticket;
     3. a short unicast probe of the local /24, only while the "Add device" screen is open (the LocalSend fallback for networks that drop multicast).
   - Reaching a peer over the internet (relay) is a later, opt-in mode. It is never the default.

3. **Pairing works between any two devices.**
   - If one side has a camera, it scans the other's QR (`vutric://pair?...`).
   - If neither does (desktop ↔ desktop), the user picks the device from the nearby list. Both screens then show the same three words, and the user confirms on both, like Bluetooth numeric comparison.
   - The words come from TLS exported keying material of that very connection. A man in the middle would see different words.
   - The pairing word already in screen W1 (`qamish`) becomes this check.

4. **No shared network → one device makes one.**
   - "Hotspot mode": an Android phone opens a `LocalOnlyHotspot`. It has no internet by design. Its SSID, password and the pairing ticket are put in one QR.
   - The other device joins:
     - Android: `WifiNetworkSpecifier`;
     - Windows: the WinRT Wi-Fi API;
     - Linux: NetworkManager.
   - The same iroh stack then runs on top. Above the link layer everything is IP, so there is one code path.
   - Wi-Fi Aware is a later option: support is uneven across Android devices.

5. **What "receive" looks like per platform.**

   | Receiver | Effect |
   |---|---|
   | Desktop | The Halqa overlay (ADR-0004). |
   | Android | Halqa in-app when Vutric is open; otherwise a heads-up notification with the thumbnail. No `SYSTEM_ALERT_WINDOW`: drawing over other apps is a permission we do not ask for. |

   Sending from a desktop: drag a file onto the Send Zone edge, the tray menu, or "Send with Vutric" in the file manager.

## Platform constraints we design for

- **Android** delivers multicast only under a `WifiManager.MulticastLock`. Visibility runs in a foreground service while "Ko'rinadigan" (visible) is on.
- **iOS**
  - Raw multicast needs Apple's `com.apple.developer.networking.multicast` entitlement (approval required).
  - Standard Bonjour browsing of a declared service type (`NSBonjourServices`) does not.
  - So iOS will use `NWBrowser`/`NWListener` for `_vutric._udp` and feed the found addresses into iroh. iOS is platform step 3 (ADR-0003); spike 0.2 does not cover it.
- **Windows** asks for a firewall rule. The installer adds an inbound UDP rule for the Private profile only.
- **Guest and hotel Wi-Fi** often isolate clients. Unicast is blocked too, so no discovery trick helps. The app detects it (ticket addresses unreachable) and offers Hotspot mode.

## Consequences

- Spike 0.2 measures desktop ↔ desktop and phone ↔ desktop in both directions, with the router's WAN unplugged.
- Spike 0.2 exit criteria are unchanged (p50 < 40 ms, p95 < 80 ms) and now also hold in both directions, offline.
- Phone ↔ phone needs two Android devices and is measured when they are available.
- No third-party server is involved in any transfer, which also keeps the privacy promise on screen A1: "Hech narsa internetga chiqmaydi" (nothing goes to the internet).

## Sources

- [LocalSend protocol](https://github.com/localsend/protocol): multicast discovery with HTTP fallback.
- [Apple: multicast entitlement](https://developer.apple.com/tutorials/data/documentation/bundleresources/entitlements/com.apple.developer.networking.multicast.md), [Apple DTS: when Bonjour does not need it](https://developer.apple.com/forums/thread/655920).
- [Android: local-only hotspot](https://developer.android.com/develop/connectivity/wifi/localonlyhotspot), [Wi-Fi network request API](https://developer.android.com/develop/connectivity/wifi/wifi-bootstrap), [Wi-Fi Aware](https://developer.android.com/reference/android/net/wifi/aware/WifiAwareNetworkSpecifier).
