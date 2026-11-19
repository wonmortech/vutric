# ADR-0130: W6 on the Android channel, sharing shortcuts per paired device, and the Android 14 transfer job

- Status: accepted, implemented in `apps/mobile-spike` (stream W6, package `io.wonmor.vutric_spike.w6`). Compiles
  and passes the unit tests listed below; nothing has been tried on a real phone yet.
- Date: 2026-11-18
- Builds on: [ADR-0009](0009-native-modules-kotlin-swift.md) part 2, [ADR-0012](0012-android-modules-phase-1.md)
  decisions 1-3, [ADR-0013](0013-phone-on-the-w3-api.md), [ADR-0014](0014-phone-pairing-companion-compat.md).
- Backlog: F2, F7 (Android side). Related W6 ADRs: 0131 (Wi-Fi Aware and Direct), 0132 (BLE), 0133 (ring), 0134 (M3).

## Context

W1 owns the phone app and is busy with iOS. Several Android doors from ADR-0009 were still open: direct Share
targets per paired device (F2), a transfer that survives the app being closed on Android 14+ (F7), the Wi-Fi Aware
and Wi-Fi Direct links (R6, R7), the BLE plumbing (R4), the ringer (F14) and the M3 hotspot probe. W6 builds them in
new files so W1 only wires them, and never rewrites a W1 file.

## Decision

### 1. One door, on W1's channel

| # | Decision | Why |
|---|---|---|
| 1 | All W6 Kotlin lives in package `io.wonmor.vutric_spike.w6`; resources are prefixed `w6_`; tests in `app/src/test/**/w6/` | No file of W1's is shared, so the two streams never conflict |
| 2 | **No second channel.** `MainActivity` forwards every method it does not know to `W6.handle` (`w6.*` methods) and every permission result to `W6.onPermissionsResult`; events go out on the same `event` method as `{"kind": "w6.<module>", ...}` | ADR-0012 decision 1: one `vutric/android` MethodChannel, UI events only; Kotlin keeps no app state |
| 3 | Runtime permissions are asked per feature at the tap (`w6.permissions` with `feature`, `ask`), never at launch. One W6 prompt at a time | Same rule as the hotspot prompt (ADR-0012 decision 6) |

The method table is in the doc comment of `W6.kt`.

**Permissions added to the manifest (W6 total):**

| Permission | For | Notes |
|---|---|---|
| `RUN_USER_INITIATED_JOBS` | F7 | normal, install-time |
| `VIBRATE` | F14 | normal |
| `USE_FULL_SCREEN_INTENT` | F14 | Android 14+ grants it by default only to calling/alarm apps; checked with `canUseFullScreenIntent()`, never assumed (ADR-0133) |
| `BLUETOOTH_SCAN` (`neverForLocation`), `BLUETOOTH_ADVERTISE`, `BLUETOOTH_CONNECT` | R4 | Android 12+; the scan is never used to derive location |
| `BLUETOOTH`, `BLUETOOTH_ADMIN` (`maxSdkVersion` 30) | R4 | Android 7-11 |
| (already there) `NEARBY_WIFI_DEVICES` `neverForLocation`, fine location `maxSdkVersion` 32 | R6, R7, M3 | the hotspot's permissions cover Aware and Direct too |

`uses-feature` `bluetooth_le`, `wifi.aware`, `wifi.direct`, all `required="false"`: phones without them still install.

### 2. F2: sharing shortcuts per paired device

| # | Decision | Why | Where |
|---|---|---|---|
| 4 | Dynamic **sharing shortcuts** (`ShortcutManagerCompat`, long-lived, category `io.wonmor.vutric.category.DEVICE_SHARE_TARGET`, `res/xml/w6_share_targets.xml` on `MainActivity`). Android 10+ reads them natively; Android 7-9 through androidx.sharetarget's `ChooserTargetServiceCompat`, the replacement for the removed `ChooserTargetService` | Quick Share's "device in the Share sheet", on every Android version we support | `ShareShortcuts.kt` |
| 5 | The list comes **from the trust list**: Dart passes the full current `devices()` list (most recent first) to `w6.shortcutsPublish` whenever it changes. Stale shortcuts are removed with `removeLongLivedShortcuts`; `addDynamicShortcuts` (not `set`) keeps any other dynamic shortcut. At most the launcher's per-activity limit | Kotlin keeps no state; a forgotten device disappears with the next call | `ShareShortcuts.publish` |
| 6 | A share through a device shortcut arrives as the normal `ACTION_SEND` with `EXTRA_SHORTCUT_ID`; W1's share event gets one more key, `device` (endpoint id, or empty) | W1's `ShareEntry` path stays the only copy path; Dart only preselects the device | `MainActivity.route`, `ShareShortcuts.deviceOf` |
| 7 | `w6.shortcutUsed` after a send ranks that device higher | The Share sheet orders by use | `ShortcutManagerCompat.reportShortcutUsed` |

### 3. F7: transfer with the app closed

| # | Decision | Why | Where |
|---|---|---|---|
| 8 | On Android 14+ a transfer is held by a **user-initiated data transfer job** (`setUserInitiated(true)`, network `ANY`) when the active network is validated. W1's `dataSync` service stays the path on Android 7-13 and in every other case | The job has no fixed time cap (Android 15 caps `dataSync` at 6 h a day) and shows in the system's task manager | `UserTransferJob.kt` |
| 9 | Hotspot, Wi-Fi Aware, Wi-Fi Direct and a router without internet stay on the service: the job's network constraint would never be met there | A job that never starts holds nothing | `KeepAlive.choose` |
| 10 | A scheduled job that has not started within 5 s gives the transfer to the service; a job the system ends (network, quota) falls back to the service; the **user's Stop** in the task manager is final (event `w6.transfer` `stopped_by_user`, no service behind their back) | Never two keep-alives, never one that ignores the user | `KeepAlive.choose`, `UserTransferJob.onStopJob` |
| 11 | W1's `TransferService` is extended by three registration lines only: `update` asks the job first, `stop` stops it, `isRunning` counts it. The bytes still move in Rust in the app process; the job, like the service, only keeps the process at transfer priority and carries the progress notification (same channel `transfers`) | ADR-0012 decision 3 stays true; one notification switch for the user | `TransferService.kt` |

## W1 files touched (registration lines only)

| File | Lines |
|---|---|
| `AndroidManifest.xml` | permissions and features above; two `meta-data` on `MainActivity`; `UserTransferJob`, `RingActivity`, `RingStopReceiver` |
| `src/debug/AndroidManifest.xml` | `M3ProbeActivity` (debug builds only) |
| `MainActivity.kt` | `W6.sink`, the `else -> W6.handle` dispatch, `W6.onPermissionsResult`, `W6.onDestroy`, the `device` key in the share event |
| `TransferService.kt` | three lines (decision 11) |
| `app/build.gradle.kts` | `androidx.core:core-ktx`, `androidx.sharetarget:sharetarget`, JUnit 4 + Robolectric for tests, `unitTests.isIncludeAndroidResources` |

## What W1 wires (no W6 Dart)

1. After every change of `devices()`: `w6.shortcutsPublish` with `[{id, name, kind}]`; after a send: `w6.shortcutUsed`.
2. In the share handler: if `device` is not empty, preselect that device and go straight to sending.
3. On `w6.transfer` `stopped_by_user`: cancel the running transfer.

## Verified

- `./gradlew :app:testDebugUnitTest`: `KeepAliveTest` (7), `ShareShortcutsTest` (4, Robolectric SDK 34) pass.
- Lint and the release APK: see the W6 PR.

## Device checklist (owner)

- [~] 1. Pair a PC, open the Gallery, Share a photo: "Ofis kompyuteri" is a direct target in the Share sheet (Android 10+; on Android 9 through the chooser bridge).
- [~] 2. Pick it: Vutric opens with the photo and the PC already chosen.
- [~] 3. Forget the PC in Vutric: its target disappears from the Share sheet.
- [~] 4. Android 14+, on a router with internet: send a 2 GB video, swipe Vutric away from recents: the transfer finishes; the system task manager lists it.
- [~] 5. Same, press Stop in the task manager: the transfer stops and does not restart by itself.
- [~] 6. Android 14+ on the phone's own hotspot: the transfer is held by the old notification service (no job).
- [~] 7. Android 13: behaviour unchanged from ADR-0012.

## Consequences

- When UniFFI arrives (ADR-0012 decision 1), the job and the shortcuts can talk to Rust without Flutter; the channel
  methods then shrink to UI events.
- androidx.core and androidx.sharetarget are new app dependencies (both already pulled in transitively by Flutter
  plugins in part; measured APK size in the W6 PR).
