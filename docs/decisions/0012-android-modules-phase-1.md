# ADR-0012: Android modules, phase 1, and the first-start screens

- Status: accepted, implemented in `apps/mobile-spike` (stream W1). It compiles (`flutter build apk --release`,
  77.0 MB) and `flutter analyze` is clean; nothing has been tried on a real phone yet.
- Date: 2026-11-18
- Implements: [ADR-0009](0009-native-modules-kotlin-swift.md) part 2, phase 1. Builds on [ADR-0011](0011-phone-ui-from-claude-design.md).
- Backlog: A2, A3, A4, A5, A6, A7, A8, A11.

## Context

ADR-0009 lists the Kotlin modules that open Android's doors (Share menu, background transfers, Quick Settings,
any folder, a hotspot). It also says Kotlin keeps no state, all logic stays in Rust, and Kotlin reaches Rust
through UniFFI. UniFFI is not wired yet. Waiting for it would hold back every module. The modules in phase 1
need no Rust call of their own, because each one only opens an OS door and hands the result on.

## Decision

| # | Decision | Why | Where |
|---|---|---|---|
| 1 | **Phase 1 bridge = one MethodChannel `vutric/android`.** Kotlin returns OS results and UI events to Dart (shared files, tile tap, picked folder, hotspot credentials); Dart calls Rust as before. UniFFI comes when a Kotlin module must call Rust without Flutter running (the tile receiving in the background, `Companion`) | ADR-0009 rule 3 allows a channel for UI events; no data channel is added, and Kotlin stays stateless | `MainActivity.kt`, `lib/android.dart` |
| 2 | **ShareEntry (A3):** `SEND` and `SEND_MULTIPLE` for `*/*`. Each stream is copied once into `cache/shared/<n>/<name>` (the name made safe), off the UI thread; the cache is cleared at the next share. The files land in the Otish selection, ready for "Yuborish". Cold start works: the start intent waits until Dart asks for it. Sharing shortcuts per paired device wait for W3's trust list | A `content://` item has no path the Rust sender can open | `ShareEntry.kt`, `HomeShell._onAndroid`, `SendTabState.addFiles` |
| 3 | **TransferJob (A4):** a `dataSync` foreground service, with one progress notification, on every Android version. The Android 14 user-initiated data transfer job is **not** used yet: it needs a `JobService` in which the work itself runs, while our bytes move in Rust in the app process. The service only keeps that process at foreground priority. Only the first call starts the service, made while the app is in front; updates (at most 2 a second) go straight to the notification manager, because Android 12+ refuses a background `startForegroundService`. `onTimeout` (Android 15's dataSync cap) stops cleanly | Big videos keep going with the screen off; no crash when the user leaves the app | `TransferService.kt`, `_TransferSheet` in `transfer_ui.dart` |
| 4 | **VutricTile (A5):** a non-toggle tile, "Vutric: qabul". A tap opens the app on Qabul with this phone's QR (after an unlock if the phone is locked). It has no on/off state, because the session lives in the app | One tap to receive, never a tile that lies about state | `VutricTile.kt`, `ACTION_RECEIVE` |
| 5 | **SafStore (A8):** pick any folder (`ACTION_OPEN_DOCUMENT_TREE`, persisted grant). Rust still writes into the app folder and checks every file. Only then does Kotlin copy each finished file into the tree and delete the local copy after a size check. A file that fails stays where it was. The tree URI lives in `ui.json` **temporarily** (W3 #8: `vutric-core` settings drop unknown keys) | A half-written file never appears in the user's folder; reaches SD cards and cloud folders that a plain path cannot | `SafStore.kt`, `_offer` in `receive_tab.dart`, Sozlamalar → Papkalar |
| 6 | **LinkManager (A6), hotspot part:** `LocalOnlyHotspot` behind "Routersiz: telefon hotspoti" in the connect sheet. Step 1 shows a `WIFI:` QR (phone cameras join it directly) plus the name and password with copy buttons for a computer. Step 2 is the normal pairing QR, refreshed every 4 s so it carries the hotspot address. The hotspot stays up while the session rides on it and closes when the session ends. The permission (`NEARBY_WIFI_DEVICES` with `neverForLocation` on 13+, fine location only on 8–12) is asked at the tap. Wi-Fi Direct and Aware come later (R6, R7) | Full Wi-Fi speed with no router and no internet | `LinkManager.kt`, `lib/screens/hotspot_page.dart` |
| 7 | **Nearby (A7):** phase 1 is the multicast lock (held only while the app is in front) plus the Rust mDNS discovery already in `vutric-net`. Android `NsdManager` is **not** added yet. It would announce the same service a second time, in a format W3 owns. It is worth adding only if real phones show OEMs that block raw multicast, which the device run will tell. BLE stays with R4 | No duplicate discovery path until a measurement asks for one | `MainActivity.kt` (`vutric/multicast`) |
| 8 | **First start (A2):** two screens, shown once (`onboarded` in `ui.json`). (1) What Vutric does, with a still picture of the throw. (2) This phone's name plus the permissions. The gallery is asked here; the camera and local network are only explained here, because the OS asks for them when they are first used, with the reason strings from the manifest and Info.plist. Nothing blocks: every permission can be given later in place | A prompt with context gets accepted; a wall of prompts at launch does not | `lib/screens/onboarding.dart` |
| 9 | **Manual fallback for the scanner (A2):** pasting the pairing link stays the fallback. A typed IP alone cannot connect, because the endpoint id is required; W3 #9 asks for a short code or an address-only hello | Honest fallback today, better one tracked | `scan_page.dart`, W3 #9, `docs/owner-questions.md` |
| 10 | App label "Vutric" (was "Vutric spike") | It is what the user sees in the Share sheet and launcher | `AndroidManifest.xml` |
| 11 | **Receive in the background (A11):** a photo or file offer arriving while Vutric is not on screen becomes a heads-up notification (high-importance channel "Kelgan rasm va fayllar"; a photo carries its thumbnail as a big picture). A tap opens Qabul. No frame is awaited in the background (none would be drawn, and the thrower would hang); the notification is what "shown" reports. It works while the process lives (open session, or a transfer service running); receiving with the app fully closed needs UniFFI (decision 1) | Never draws over other apps (no `SYSTEM_ALERT_WINDOW`), never leaves the thrower waiting | `IncomingNotice.kt`, `handleIncoming` / `_offer` in `receive_tab.dart` |

## Verified

- `flutter analyze`: no issues.
- `flutter build apk --release --target-platform android-arm64,android-x64`: passed, Kotlin compiled, 77.0 MB.

## Device checklist (adds to ADR-0011's)

1. Gallery → share 3 photos → Vutric: the app opens on Otish with them selected; also from a cold start.
2. Share a PDF from a file manager: it is copied and sent with its real name.
3. Start a 1 GB video, go home, turn the screen off: the notification shows progress and the transfer finishes.
4. Android 13+: the notification permission prompt appears once, at the first transfer.
5. Quick Settings → add "Vutric: qabul" → tap on a locked phone: unlock, then Qabul with the QR.
6. Sozlamalar → Papkalar → pick an SD card folder → receive files: they end up there, and the snackbar says so.
7. Revoke the folder grant in system settings: files stay in the Vutric folder and nothing crashes.
8. Wi-Fi off everywhere → Routersiz → second phone scans the Wi-Fi QR → pairing QR → a throw lands.
9. Close the session: the hotspot goes away.
10. App in the background, desktop throws a photo: a heads-up with the thumbnail appears at once, and the desktop shows the result (no timeout); a tap opens Qabul.

## Consequences

- When UniFFI arrives, the same modules call Rust directly; the Dart-side calls in `lib/android.dart` then
  shrink to UI events only.
- Two W3 asks are open (#8 settings field, #9 typed address). Two `TEMP` notes mark the code to change.
