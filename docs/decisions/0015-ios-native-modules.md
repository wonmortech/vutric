# ADR-0015: iPhone native modules: App Group, Share Extension on Rust, Live Activity, App Intents

- Status: accepted, implemented in `apps/mobile-spike/ios`, `apps/mobile-spike/lib` and `crates/vutric-mobile`
  (stream W1). `cargo clippy`/`cargo test -p vutric-mobile`, `flutter analyze --fatal-infos` and `flutter test`
  pass locally; the iOS targets, the VutricKit XCTests and both IPAs are built only in CI
  ([ios-spike.yml](../../.github/workflows/ios-spike.yml)). Nothing has run on a real iPhone yet.
- Date: 2026-11-18
- Builds on: [ADR-0009](0009-native-modules-kotlin-swift.md) (Flutter UI, native Swift/Kotlin modules, one Rust library carrying FRB
  and UniFFI, bindings generated, platform channels only for UI events), [ADR-0012](0012-android-modules-phase-1.md)
  (the Android modules and their one `MethodChannel`), [ADR-0013](0013-phone-on-the-w3-api.md).
- Backlog: I1–I8, the iOS halves of F2 and F10; F8 and F9 through I2.

## Context

The iPhone build was the Flutter app alone: no way to send from another app's Share sheet, nothing on the lock
screen while a transfer runs, nothing for Siri or Shortcuts, and no hotspot join, Files folder or HEIC choice.
Android got its native modules in ADR-0012. iOS extensions cannot run a Flutter engine cheaply (a Share Extension
has roughly 120 MB of memory and must start fast), and they live in separate processes that see none of the app's
files unless both sides share an App Group.

## Decision

| # | Decision | Why | Where |
|---|---|---|---|
| 1 | **One App Group, also the Keychain group (I8).** Every target signs `com.apple.security.application-groups = $(VUTRIC_APP_GROUP)` (`group.io.wonmor.vutricSpike`, one build setting). Rust's `settings.json` and `trusted.json` move from the app's own folder into `<group>/vutric` once (`moveIntoGroup`, never over a newer file), and `set_settings_dir` points there. The identity key moves from `identity.key` into one Keychain item (`io.wonmor.vutric.identity`, `AfterFirstUnlockThisDeviceOnly`) whose access group is the App Group id, so no Team ID prefix has to be known at build time. When the Keychain refuses (no entitlement in the signature) the file copy stays and only the extensions lose sending | The extensions must see the same pairing and identity as the app, or they would be strangers to the PC | `SharedStore.swift`, `Keychain.swift`, `*.entitlements`, `main.dart` (`settingsDir`), `session.dart` (`loadIdentity`) |
| 2 | **The Share Extension calls Rust in its own process, through UniFFI (I1).** `crates/vutric-mobile/src/share.rs` exports `share_open`, `share_devices`, `share_send` (with a `ShareListener` progress callback) and `share_cancel` with `uniffi` proc-macros in the same library as the FRB bridge (ADR-0009 rule 2). `share_send` binds its own short-lived iroh peer (the route setting decides relay or not), dials the trusted device by id with any `NWBrowser` hints, runs `transfer::send` and closes, so it never touches the app's session slot. The Swift bindings are generated in CI (`tool/ios_bindings.sh`, `vutric-uniffi-bindgen` in library mode on the `.a`), never committed. The appex links `libvutric_mobile.a` **without** `-force_load` and with dead stripping, so only the share path and what it reaches stays in it | The alternatives were worse: handing files to the app (`openURL` from an extension is not allowed, and the app may not be running) or a background URLSession (no P2P). The size is in the CI job summary; the job fails above 100 MB | `share.rs`, `uniffi-bindgen/`, `RustShare.swift`, `VutricShare/`, `ios_bindings.sh`, ios-spike "Sizes" |
| 3 | **The Share Extension is SwiftUI only.** It lists paired devices from the snapshot, copies attachments into `<group>/outbox/<uuid>` off the main actor (files, images, movies, text and a URL as `.txt`; HEIC to JPEG when the setting is on), shows one progress line, and maps every `ShareError` to a key (`share.error.*`, `bulk.too_big`). Rows are 44 pt; the progress animation is off under Reduce Motion. When the sheet closes its outbox folder is removed and any folder older than a day is swept | The sheet must open instantly and never hold a Flutter engine | `ShareView.swift`, `ShareModel.swift`, `Info.plist` (activation rule) |
| 4 | **Device suggestions in the Share sheet (F2, iOS half).** After every start, pairing and forget, the app donates one `INSendMessageIntent` per paired device (newest 8; `conversationIdentifier` = device id) after deleting the old ones. The Share Extension declares `IntentsSupported = INSendMessageIntent`; when the sheet opens from a suggestion, `extensionContext.intent` names the device and the send starts without a pick | This is how iOS shows "people" at the top of the Share sheet; no private API | `VutricIosPlugin.syncShared`, `ShareModel` (`extensionContext.intent`) |
| 5 | **Live Activity and the "PC'ga yuborish" widget (I2, F8, F9).** A WidgetKit extension (iOS 16.2+) holds `SendWidget` (small, circular and rectangular lock-screen families; a tap opens `vutric://send[?device=]`) and `TransferLiveActivity` (lock screen and Dynamic Island: device, direction, percent, bytes, files). The app drives it from the transfer sheet through `transferUpdate` / `transferStop`; Swift throttles updates to twice a second (`UpdateGate`, phase changes always pass) and dismisses four seconds after the end. Numbers are formatted like Dart's `sizeText` | Progress where the user looks; ActivityKit has no push token here (no server) | `VutricWidgets.swift`, `TransferActivity.swift`, `TransferState.swift`, `TransferActivityController`, `transfer_ui.dart` |
| 6 | **App Intents (I3, F10 iOS half).** `SendFilesIntent` ("Qurilmaga yuborish": files and a device entity from the snapshot) copies the files to the outbox, browses Bonjour for two seconds and sends through the same `RustShare.send` in the app process, with no screen. `OpenSendIntent` opens the app on the Otish tab (for the Action button). `VutricShortcuts` publishes both with Uzbek and English phrases | Siri, Shortcuts, automations and the Action button without opening the app | `Intents.swift` |
| 7 | **Bonjour and the Local Network prompt (I4).** Runner and the Share Extension declare `NSLocalNetworkUsageDescription` and `NSBonjourServices = _vutric._udp`. iroh's mDNS (swarm-discovery) publishes plain DNS-SD under that type, the instance being the endpoint id in base32, so `NWBrowser` (no multicast entitlement needed) sees desktops; `NearbyBrowser` resolves each to IP:port and Rust turns the instance name back into an endpoint id (`endpoint_of_instance`). Rust's own mDNS on iOS still needs `com.apple.developer.networking.multicast`, which is requested from Apple (owner question) | Browsing works now with no entitlement; advertising from the phone waits for Apple | `Nearby.swift`, `share.rs`, `Info.plist` files |
| 8 | **Hotspot join (I5).** `NEHotspotConfiguration` (WPA/WPA2/WPA3 or open, hidden, `joinOnce`), then a `NEHotspotNetwork.fetchCurrent` check of the SSID; errors map to Android's codes (`bad_network`, `permission`, `unavailable`, `unsupported`), so `hotspot_page.dart` keeps one set of messages. The entitlement `com.apple.developer.networking.HotspotConfiguration` is in Runner only | Same flow as A6 | `Hotspot.swift`, `hotspot_page.dart`, `connect.dart` |
| 9 | **Photos and HEIC (I6).** The gallery stays `photo_manager` (PhotoKit underneath); the native part is "HEIC rasmlarni JPEG qilib yuborish" (off by default, quality 0.9, ImageIO, metadata kept). The Share Extension follows the same setting from the snapshot | Windows has no HEIC codec by default; the original stays untouched on the phone | `Heic.swift`, `send_tab.dart`, `settings_page.dart` |
| 10 | **Files folder (I7).** A folder picked in Files is kept as a security-scoped bookmark; received files move there after verification with `NSFileCoordinator` (copy, then delete; free names as "a (2).jpg"); a stale bookmark is refreshed. The bookmark sits in `ui.json` as `// TEMP until W3 #26`, because W3's `save.android_tree` accepts only `content://` URIs | Same as A8 on Android | `FolderBookmark.swift`, `receive_tab.dart`, `session.dart`, issue #26 |
| 11 | **One `MethodChannel` `vutric/ios`, no Pigeon.** Same shape as `vutric/android` (ADR-0012): Dart calls `groupDir`, `identity`, `storeIdentity`, `syncShared`, `takeStartEvents`, `transferUpdate`, `transferStop`, `hotspotJoin`, `hotspotLeave`, `jpegFromHeic`, `pickFolder`, `folderUsable`, `moveToFolder`; Swift sends `event` (`kind: send`, optional `device`, validated as 64 hex in both languages). Registration goes through the implicit engine's registrar and `addSceneDelegate` for URL and activation events | A dozen small calls do not pay for a code generator; transfers never cross it | `ios.dart`, `VutricIosPlugin.swift`, `AppDelegate.swift` |
| 12 | **Shared Swift in a local package `VutricKit`.** Store, keychain, transfer state, Bonjour, HEIC, folder, hotspot and strings live in one SwiftPM package that all three targets link and that has its own XCTests, run on a simulator in CI. Strings are a String Catalog (`uz` source, `en`) with the same keys as `lib/i18n.dart`; a Dart test fails when a catalog line and the Dart line differ | Testable without a phone; one copy of the strings | `ios/VutricKit/`, `test/ios_test.dart` |
| 13 | **Two IPAs.** `vutric-spike-unsigned.ipa` as before, and `vutric-spike-adhoc.ipa`: the same bundle ad-hoc signed with each target's entitlements expanded, so a sideloading tool that keeps bundle entitlements can carry the App Group | Whether a free Apple ID may hold App Groups and the hotspot entitlement is not something CI can tell | ios-spike "IPAs" |

## Verified

- `cargo clippy -p vutric-mobile -p vutric-uniffi-bindgen --all-targets -- -D warnings` clean; `cargo test -p
  vutric-mobile` passes, including the new `share` tests (base32 instance → endpoint id, other services ignored,
  hint filtering, not set up, bad identity).
- `uniffi-bindgen` library mode on the static library (checked on Windows: the `.lib` and the `.dll` give the same
  Swift).
- `flutter analyze --fatal-infos` clean, `flutter test` passes (83), with `test/ios_test.dart`: events, snapshot
  shape, HEIC names, the App Group move, both String Catalogs against `lib/i18n.dart`, and the Xcode project
  (App Group in every target, Bonjour where discovery runs, extensions embedded before Thin Binary, the
  extension not force-loading Rust).
- In CI: VutricKit XCTests on an iPhone simulator, `flutter build ios --release --no-codesign` with both
  extensions embedded, `nm` finds `uniffi_vutric_mobile_fn_func_share_send` in Runner and in the Share Extension,
  sizes in the job summary, and the ad-hoc IPA's three signatures carry the App Group (run 44450699151, all
  green).
- Measured sizes (arm64 release, before App Store thinning, run 44450699151): Share Extension binary
  **11.2 MB** (11 824 848 bytes; whole `.appex` 11 564 KiB), Runner 22.6 MB, `App.framework` 6.4 MB, widget
  extension 0.5 MB; IPAs 21.6 MB (unsigned) and 21.9 MB (ad hoc). Linking Rust without `-force_load` and with dead
  stripping keeps the extension at about half of Runner.

## Not verified (needs a real iPhone)

- That Sideloadly with a free Apple ID keeps the App Group, the second and third bundle ids, and the hotspot
  entitlement; whether the ad-hoc IPA is needed at all.
- The Share Extension's memory peak during a large send (the 120 MB limit), and that iroh binds and dials from an
  extension process.
- Live Activity on the lock screen and in the Dynamic Island; widget placement; Siri phrases in Uzbek.
- Share sheet suggestions appearing after donation.
- `NWBrowser` finding a desktop and the Local Network prompt in the extension.
- Hotspot join answers on a device; the Files folder move into iCloud Drive and "On My iPhone".

## Device checklist (adds to ADR-0014's)

1. Install `vutric-spike-adhoc.ipa` with Sideloadly; if the extensions are missing, install the unsigned one and
   note which capabilities Sideloadly reports.
2. Open Vutric, pair a PC (QR), allow Local Network.
3. Photos → Share → "Vutric": the PC is listed; send 3 photos; the PC receives them. Note the time to open.
4. Photos → Share: the PC appears in the top row of suggestions; tapping it sends without a pick.
5. Send a 2 GB video from Files through the Share sheet; watch for the sheet being killed (memory).
6. Start a send in the app, lock the phone: the Live Activity shows the percent; Dynamic Island on a Pro model.
7. Add the "PC'ga yuborish" widget (home and lock screen); tap it: Vutric opens on Otish.
8. Shortcuts → "Qurilmaga yuborish": pick a file and the PC; run it from Shortcuts and from Siri.
9. Settings → turn on HEIC → JPEG; send a HEIC photo; the PC gets `.jpg`.
10. Settings → pick a Files folder; receive a file from the PC; it lands in that folder.
11. Desktop shows the hotspot QR; scan it on the iPhone: it joins and pairs; leaving the session forgets the network.

## Consequences

- One Rust library, two binding layers (FRB for Dart, UniFFI for Swift); the share API is small and separate from
  `api.rs`, so W3 changes to the session do not touch the extension.
- The extension runs its own peer: a send from the Share sheet while the app holds a session to the same PC opens
  a second connection, which the PC accepts as another session.
- Three bundle ids per install; a free Apple ID has a small App ID allowance per week, so testers may hit it.
- The Files folder lives in `ui.json` until W3 #26.
- `BGContinuedProcessingTask` (I9) and TestFlight (I10) stay open.
