# ADR-0009: One Rust core, Flutter for the screens, Kotlin and Swift for the OS doors

- Status: accepted by the owner (2026-11-18) for the layering and the Kotlin/Swift modules. The desktop UI
  toolkit (Flutter desktop) stays provisional until the spike in "Verification" has measured it.
- Date: 2026-11-18
- Builds on: [ADR-0003](0003-platform-order.md), [ADR-0004](0004-windows-overlay-composition.md),
  [ADR-0007](0007-every-route-and-storage-settings.md), [ADR-0008](0008-gallery-first-phone-app.md).
- Research: [vutric-stack-tavsiya.html](../research/vutric-stack-tavsiya.html) and
  [airdrop-alternativa.html](../research/airdrop-alternativa.html) (Uzbek), both summarised here.
- Supersedes: nothing. It makes explicit the Flutter desktop choice that `docs/research/original-brief.md` made
  without a reason.

## Context

The owner's criteria (2026-11-18):

- The project is long-term, and rewriting screens is acceptable.
- A large download is acceptable.
- What matters most:
  - **low RAM while running**;
  - **smooth and fast in real use**;
  - **a "wow" effect**;
  - **every feature the competitors have**, in one app.

After reading the stack proposal, the owner chose the variant with **Kotlin and Swift modules**.

Many competitor features live inside the operating system, not inside an app window:

- Quick Settings tile
- Share-sheet device shortcuts
- background transfer
- hotspot
- BLE
- Live Activity
- Siri/Shortcuts

Flutter cannot reach these properly, and an iOS Share Extension cannot host the Flutter engine within its memory
limit.

## Decision

### 1. Layers

| Layer | Language | Runs | Owns |
|---|---|---|---|
| Core | Rust (`vutric-core`, `vutric-net`, future `vutric-compat-*`) | on every device, one copy of the code | protocol, pairing, transfer, resume, trust list, settings schema, file-name safety, route choice |
| Desktop daemon | Rust (`tray-icon`, `windows-rs`, `zbus`) | always (autostart) | network, transfers, tray, toast, IPC server, overlay |
| Overlay | Rust `wgpu` (ADR-0004) | window always present, draws only during effects | Halqa effect **and** the instant cards ("Qabul qilasizmi?", progress, "tushdi") |
| Big screens | Flutter (phone; desktop provisional) | only while open; desktop process exits on close | gallery, settings, history, devices, send sheet |
| OS doors, Android | Kotlin | as Android starts them | the modules in part 2 |
| OS doors, iOS | Swift / SwiftUI | as iOS starts them (own processes for extensions) | the modules in part 3 |

**Rules:**

1. **Business logic only in Rust.** Kotlin, Swift and Dart call OS APIs and hand the result to the core. They
   keep no state of their own, so the Flutter screen and a Kotlin tile can never disagree.
2. **One native library per platform.** Android gets one `libvutric.so` and iOS gets one `xcframework`, carrying
   both the flutter_rust_bridge exports (Dart) and the **UniFFI** exports (Kotlin, Swift). The core is never loaded twice.
3. **Bindings are generated, never hand-written.** A new capability is added to Rust first. FRB and UniFFI then
   expose it. Pigeon is used only for UI events between Flutter and Kotlin/Swift, for example "tile tapped, open
   this screen". Data never travels over Pigeon.
4. **Settings have one owner**, Rust (ADR-0008 decision 12).

### 2. Kotlin modules (Android, from Phase 1)

| Module | Android API | What the user gets | Learned from | When |
|---|---|---|---|---|
| `TransferJob` | user-initiated data transfer job (Android 14+), `dataSync` foreground service before that | a big video keeps going with the app closed or the screen off | Quick Share | Phase 1 |
| `ShareEntry` | `ACTION_SEND`, `SEND_MULTIPLE`, sharing shortcuts | Vutric in every Share menu; paired devices ("Ofis kompyuteri") as direct targets | AirDrop, Quick Share | Phase 1 |
| `VutricTile` | `TileService` | one tap in Quick Settings turns on "receive mode" | Quick Share | Phase 1 |
| `LinkManager` | `LocalOnlyHotspot`, `WifiP2pManager`, `WifiAwareManager` | full Wi-Fi speed with no router and no internet (R3, R6, R7) | Quick Share | Phase 1 (hotspot), later P2P/Aware |
| `Nearby` | `NsdManager`, `MulticastLock`, `BluetoothLeAdvertiser/Scanner` | devices appear by themselves, no QR (R1, R4) | AirDrop, Quick Share | Phase 1 (mDNS), BLE with R4 |
| `SafStore` | `ACTION_OPEN_DOCUMENT_TREE`, MediaStore | save to any folder; fixes the folder-path limit of ADR-0008 | ADR-0007 part 4 | Phase 1 |
| `Companion` | `CompanionDeviceManager`, `NotificationListenerService` | background rights for the paired PC; notifications mirrored to the PC | KDE Connect, Phone Link | Phase 1+, opt-in |
| `Clipboard` | `ClipboardManager` (foreground only) | send the copied text to the PC in one tap | KDE Connect | later |

### 3. Swift modules and extensions (iOS, from Phase 3)

| Part | iOS API | What the user gets | When |
|---|---|---|---|
| Share Extension (SwiftUI, **no Flutter**) | `NSExtension` share | send from Photos, Files and Safari in one tap; Rust streams the file and never loads it whole | Phase 3 |
| Live Activity | ActivityKit | progress in the Dynamic Island and on the lock screen | Phase 3 |
| App Intents | AppIntents | Siri and Shortcuts ("send the last photo to the office PC"), the Action button | Phase 3 |
| Discovery | `NWBrowser`/`NWListener` (Bonjour `_vutric._udp`), CoreBluetooth | the PC appears on the same Wi-Fi; BLE finds a device next to you | Phase 3 |
| Hotspot join | `NEHotspotConfiguration` | joins the hotspot an Android phone or PC opened, from the QR | Phase 3 |
| Photos | PhotoKit, HEIC → JPEG option | the in-app gallery (ADR-0008) with formats Windows can open | Phase 3 |
| Files folder | security-scoped bookmark | save into any Files folder, iCloud Drive included | Phase 3 |
| Wi-Fi Aware | iOS 26 Wi-Fi Aware | iPhone ⇄ iPhone with no router (R6) | after Phase 3 |

The app, the Share Extension and the Live Activity share one App Group folder and one Keychain group. A file sent
from the extension is therefore encrypted with the same paired key.

**Honest iOS limits:**

- An iPhone cannot receive over QUIC in the background for long, so it receives with the app open.
- An app cannot read other apps' notifications.
- The clipboard is read only in the foreground, after the user's "Paste".

### 4. Desktop: what is always on, what opens on demand

| Process | When | RAM target |
|---|---|---|
| `vutric-daemon` (Rust) | always | ≤ 30 MB idle |
| overlay (inside the daemon) | window always present | +5–15 MB idle |
| `vutric-app` (Flutter desktop, provisional) | while the user has it open | ≤ 120 MB open, 0 when closed |

Rejected for the desktop big screens:

- **Tauri 2.** WebView2 opens several processes, which gives the highest RAM. WebKitGTK on Ubuntu is weaker for
  animation and blur.
- **Slint.** Rich shader effects are hard to build.
- **Our own wgpu UI.** We would have to write text input, IME, accessibility and scrolling ourselves: about a year
  of work.
- **Tauri in the daemon process.** The webview would either stay loaded or be rebuilt on every open. Both break
  "always light".

## Consequences

- There are three UI languages (Dart, Kotlin, Swift). They stay thin: each module has one OS API and one test.
- The build gets harder: one crate exports both FRB and UniFFI. CI checks both symbol sets with `nm`, as the iOS
  workflow already does for FRB.
- Android 14+ background limits are met with the user-initiated data transfer job. Older versions use a
  `dataSync` foreground service.
- The overlay cards need clicks without taking focus (`WS_EX_NOACTIVATE`, hit-testing only on the card). If that
  fails in the spike, the card opens in a small Flutter window instead.

## Verification (spike before the desktop toolkit is final)

The same settings screen and the Halqa are built in Flutter desktop, Tauri and Slint. LocalSend itself is measured
on the same machine as a real Flutter-desktop reference. The machines are Intel UHD 730 at 2560×1440 and Ubuntu.

| Measure | Target |
|---|---|
| Daemon idle RAM (Private Bytes / PSS, 10 min average) | ≤ 30 MB |
| Desktop UI open | ≤ 120 MB and less than LocalSend |
| After the UI closes | back to daemon level |
| Overlay card appears (offer → first present) | ≤ 16 ms |
| UI cold open (tray click → first frame) | ≤ 400 ms |
| Animation | 60 fps, p99 < 16.7 ms (ADR-0004 method) |

The result table is added here, and the owner confirms the desktop toolkit.
