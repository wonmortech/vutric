# ADR-0013: The phone on the W3 API: trust, connect without a QR, routes, hotspot join, companion

- Status: accepted, implemented in `apps/mobile-spike` and `crates/vutric-mobile` (stream W1). `flutter analyze`
  is clean, `flutter test` and `cargo test -p vutric-mobile` pass, the release APK builds. Nothing has been tried
  on a real phone yet.
- Date: 2026-11-18
- Builds on: [ADR-0011](0011-phone-ui-from-claude-design.md), [ADR-0012](0012-android-modules-phase-1.md).
  Uses W3's [ADR-0050](0050-trust-store-and-pairing-code.md), [0052](0052-lan-finding-beacon-and-diagnosis.md),
  [0057](0057-storage-engine-rules.md), [0058](0058-hotspot-in-the-pairing-link.md),
  [0059](0059-connect-by-typed-address-or-code.md), [0060](0060-route-order-picker-and-badge.md),
  [0061](0061-preauth-rules-and-privacy-line.md), [0063](0063-settings-schema-versions.md).
- Backlog: T1, T2, T3, A2, A6, A8, A12, A13, A14, N1; the phone parts of R3, R11, Rx, S6; F12, F13.

## Context

W3 merged the trust store and pairing code, the beacon with short codes, the typed connect, hotspot tickets,
the route picker and badge, the privacy line, the SAF tree in settings and per-device rules. The phone still
spoke the 0.2 API: one `lan_only` flag, every peer trusted after a scan, the SAF tree in `ui.json`. This ADR
records how the phone uses the new API and what is still missing from W3.

## Decision

| # | Decision | Why | Where |
|---|---|---|---|
| 1 | **Bridge.** `start(name, identity)` takes the reach from `Settings.route` (`Reach::for_picker`) and starts the `BeaconService` (phone = true); a beacon that cannot bind is logged, never fatal. New calls: `my_code`, `set_visibility` / `visibility` (with the countdown), `devices`, `forget_device`, `device_rules` / `put_device_rules` (JSON in, checked by `set_device_rules`), `typed_kind`, `connect_typed`, `pair_start` / `pair_confirm`, `route_choices`, `route_plan`, `privacy_lines`, `current_badge`, `hotspot_link`, `wifi_qr`, `hotspot_of_link`. Errors with a meaning carry an i18n key (`find.silent`, `find.denied`, `find.unreadable`, `find.no_beacon`) | The UI decides nothing the engine already decides; it only words it | `crates/vutric-mobile/src/api.rs`, bindings regenerated (FRB 2.13) and `cargo fmt -p vutric-mobile` run after codegen |
| 2 | **Trust (ADR-0050).** The trust store is `trusted.json` beside `settings.json`. A QR or link dial trusts the peer as `ticket` (the key came from the QR). A nearby, typed or short-code dial to an unknown device runs the six-digit check: both screens show the digits, "Bir xil" on both trusts it as `code`, "Farq qiladi" on either closes the session. As listener the phone admits with `trust::admit`: visibility `trusted_only` by default, `pairing` while its own QR is on screen (Mening QR kodim, hotspot page), `everyone` for 10 minutes from Sozlamalar | Matches ADR-0050; the listener limit is decision 10 | `connect.dart` (`ConnectingPage._verify`, `PairDigits`), `settings_page.dart` ("Kim yubora oladi") |
| 3 | **Connect without a QR (R11, ADR-0059).** A fourth row "Kod yoki manzil" opens `TypedPage`: one field takes a pairing link, an IPv4 address (with or without port) or the 8-character short code; a live hint says which it is, and the clipboard is offered when it holds one. A short code can also be looked for by scan, not only by beacon. Our own short code is printed under our QR | Replaces the paste-the-link fallback of ADR-0012 decision 9 (W3 #9 closed) | `connect.dart` (`TypedPage`), `MyQrPage` |
| 4 | **Routes (Rx, ADR-0060).** The picker (Avtomatik, Faqat shu tarmoq, Internet orqali, Tarmoqsiz, Bluetooth greyed until R4) sits in Sozlamalar and as a chip at the top of the connect sheet. The sheet's rows come from `route_plan`, so "Internet orqali" hides the LAN search and the typed address, and "Tarmoqsiz" leads with the hotspot. A choice that changes the reach restarts the node; with an open session the user is asked first. The badge on the open session (peer sheet) is `Session::badge`, refreshed every 2 s | One place decides the order; the screen never promises a route that is not used | `route_picker.dart`, `waysFor` in `connect.dart`, `RouteBadge` in `throw_page.dart` |
| 5 | **Privacy line (S6, ADR-0061).** Under the hero on the home screen, in the connect sheet and under the route picker: exactly the lines `privacy_lines(route)` returns ("Fayllar shifrlangan, faqat siz va qabul qiluvchi koʻradi", plus "Hech narsa internetga chiqmaydi" for LAN-only and offline) | Says only what is true for the chosen route | `PrivacyLine` |
| 6 | **Hotspot both ways (R3, ADR-0058).** Host: LOHS starts, the camera QR is `wifi_qr` from Rust, and the Vutric QR is `hotspot_link`, so the pairing link itself carries SSID, password and security; a segmented control switches between the two. Joining: a scanned or typed link with a hotspot asks once, then `WifiNetworkSpecifier` + `requestNetwork` (30 s) + `bindProcessToNetwork`, then the node restarts on the joined network and dials. A failed dial leaves the network; closing the session leaves it too. Android 10+ only (the specifier API) | No router needed on either side, and one QR does both steps | `LinkManager.kt` (`join`, `leave`), `hotspot_page.dart`, `connectWithLink` |
| 7 | **SAF tree in settings (T2, #8).** The picked folder is `save.android_tree {uri, label}` in Rust settings; an old `ui.json` folder is moved over once. Receiving still moves finished files with `SafStore` as in ADR-0012 | One source of truth for receiving rules | `session.dart` (`loadTransferSettings`, `setSaveFolder`) |
| 8 | **Device screen (T3, ADR-0063).** Sozlamalar → Qurilmalar lists paired devices (name, method, last seen, "own rules"). A device opens on its own receiving rules: accept, gallery, split by type, subfolder, each with "Umumiy" (follow the general setting); all "Umumiy" removes the entry. "Unutish" asks, then forgets the device and its rules. The peer sheet links to it for a trusted peer | Per-device behaviour without a second settings screen | `device_page.dart` |
| 9 | **Foreign apps (A13, ADR-0010).** While the connect sheet is open, Kotlin `ForeignDiscovery` lists KDE Connect devices (`NsdManager`, `_kdeconnect._udp`, four TXT values capped at 64 chars) and LocalSend devices (passive listen on `224.0.0.167:53317`). LocalSend datagrams (4 KiB cap) are parsed only in `ForeignParser`, an `isolatedProcess` service with no permissions and no network (the ADR-0010 rule for pre-auth bytes). Nothing is announced, nothing is sent: the rows say sending opens later. TEMP until **W3 #11** (a phone host for the compat adapters) | Discovery is cheap and safe now; sending needs W3's adapters, which run only as a desktop process today | `ForeignDiscovery.kt`, `ForeignParser.kt`, `ForeignList` |
| 10 | **Listener-side pairing limit.** The listener cannot tell whether a dialer will run `pair_code`, so the phone as listener never runs it and trusts a stranger admitted during its own pairing window as `ticket`. Phone-to-phone by short code therefore times out until **W3 #13** gives the listener a way to know | Running it blindly would eat a QR dialer's first message | `accept_session` in `api.rs` (TEMP note), owner question |
| 11 | **Companion (A12, F12, F13), opt-in, off by default.** Sozlamalar → "Kompyuter bilan bogʻlash" (Android). *Clipboard:* the switch adds "Buferni yuborish" to the peer sheet; the text (64 KiB cap) travels as `Bufer <date>.txt` over the file transfer, TEMP until **W3 #12** (`Message::Clipboard`). *Notifications:* opt-in twice (the switch, then Android's notification access page); `CompanionListener` skips ongoing notices, group summaries and Vutric's own, cuts title to 80 and text to 400 chars, and an empty allowlist means all apps. Nothing leaves the phone until `Message::Notice` exists (W3 #12); notices wait in an in-memory queue of 50 | Useful now for the clipboard, honest about notifications | `companion.dart`, `Companion.kt`, manifest |
| 12 | **i18n.** All new copy is behind keys in `lib/i18n.dart` (Uzbek Latin first; English where written; a missing English line falls back to Uzbek, never empty). Rust hands keys (`route.*`, `privacy.*`, `find.*`); a test checks every key has an Uzbek line | One step towards the full i18n of ADR-0011 | `i18n.dart`, `test/pure_test.dart` |

## Verified

- `flutter analyze`: no issues. `flutter test`: 53 tests pass, among them `test/screens_test.dart`: every new
  screen in all 9 themes at 320 dp with text 2x (no overflow), 48 dp labelled tap targets, the device page writing
  whole rules and removing an override, Bluetooth not selectable, the pairing digits read one by one.
- `cargo clippy -p vutric-mobile --all-targets -- -D warnings`: clean. `cargo test -p vutric-mobile`: unit tests
  (routes, privacy, hotspot round trip, typed kind, visibility countdown) and `tests/catch.rs` (two nodes on
  loopback: trust by ticket, `trusted_only` refuses a stranger, device rules set, read, cleared, bad id refused).
- `flutter build apk --release --target-platform android-arm64,android-x64`: passed, Kotlin compiled, 78.2 MB
  (81 973 932 bytes); `aapt2 dump` shows `CompanionListener` and `ForeignParser` (`isolatedProcess=true`).

## Not verified (needs a real phone)

Hotspot join on any OEM, the beacon and short codes on a real LAN, the six-digit flow against the W2 daemon,
the badge changing from relay to direct, notification access on MIUI/One UI, KDE Connect and LocalSend devices
appearing, and the isolated parser process starting on Android 8.

## Device checklist (adds to ADR-0012's)

15. Sozlamalar → Yoʻl → "Faqat shu tarmoq": the home screen adds "Hech narsa internetga chiqmaydi"; back to Avtomatik removes it.
16. Connect sheet with "Internet orqali": no "Tarmoqdan topish" and no "Kod yoki manzil"; a QR still connects.
17. Desktop shows its short code; phone → "Kod yoki manzil" → type it: the six digits appear on both, "Bir xil" on both → connected; Sozlamalar → Qurilmalar lists it as "Raqam bilan".
18. Repeat 17 and press "Farq qiladi" on one side: both sessions close, nothing is trusted.
19. Type the desktop's IPv4 address instead: same as 17.
20. Phone A Routersiz → "Vutric bilan" QR; phone B scans it with Vutric: B asks to join, joins, connects, a throw lands; close the session on B: B leaves the hotspot.
21. Sozlamalar → Qurilmalar → the desktop → "Galereyaga: Yoʻq" → desktop throws a photo: it is not in the gallery; set "Umumiy": it is again.
22. Unutish the desktop: it disappears; its next connect needs the code again.
23. Kim yubora oladi → "Hamma, 10 daqiqa": the countdown shows; after 10 minutes it is back to "Faqat ishonchli qurilmalar".
24. Peer sheet on a connected session over the relay, then on the same Wi-Fi: the badge reads "Relay orqali", later "Toʻgʻridan-toʻgʻri · LAN".
25. Kompyuter bilan bogʻlash → "Buferni yuborish" on → copy text → peer sheet → "Buferni yuborish": a `Bufer ….txt` arrives on the desktop with the text.
26. Kompyuter bilan bogʻlash → "Bildirishnomalarni kompyuterga koʻrsatish" on: Android's access page opens; grant it; a chat notification arrives: nothing crashes, nothing is sent.
27. LocalSend open on a laptop and KDE Connect on another phone, both on this Wi-Fi: the connect sheet lists them under "Yaqindagi boshqa ilovalar"; closing the sheet stops the listening.

## Consequences

- Three W3 asks are open: #11 compat host on the phone, #12 `Notice` / `Clipboard` messages, #13 listener-side
  pairing. Each has one `TEMP until W3 #n` note in W1 code.
- The 0.2 desktop `net-spike` has no beacon and no pairing: typed codes and the six-digit check can only be tried
  against the W2 daemon or another phone (with decision 10's limit).
