# ADR-0133: "Ring my phone" on Android

- Status: accepted, implemented in Kotlin (stream W6). Compiles and passes the Robolectric tests below; nothing has
  been tried on a real phone yet.
- Date: 2026-11-18
- Builds on: W3's [ADR-0069](0069-ring-my-phone-and-the-command-line.md) (`Message::Ring`, on `w3/net-3`), issue #18,
  [ADR-0130](0130-w6-share-shortcuts-and-transfer-job.md).
- Backlog: F14 (Android side).

## Context

W3 put `Message::Ring(Start { secs } | Stop)` on the wire with its rules (`ring::accepts` = trusted only,
`RingLimit`). Issue #18 asks the phone for a loud sound and a vibration even in silent mode, a notification with
"Topdim", and a stop on `Stop`, on timeout or on the user's tap. W6 builds the Android part; W1 wires the Dart
handler and the bridge (`Incoming.ring`, `ring` / `stop_ring`).

## Decision

| # | Decision | Why | Where |
|---|---|---|---|
| 1 | Kotlin rings only what Dart hands it, after the engine admitted it. It applies no rule of its own except the bounds 1..60 s (default 30), the same as `MAX_RING_SECS` / `DEFAULT_RING_SECS` | Rules stay in Rust (ADR-0009 rule 1) | `RingPlan.of` |
| 2 | Sound on the **alarm stream**: `MediaPlayer` with `USAGE_ALARM` looping the default alarm sound (then ringtone, then notification sound; a generated `ToneGenerator` tone if the phone has none). It sounds in silent and vibrate mode. Do Not Disturb still wins where the user's DND policy blocks alarms; we do not ask for `ACCESS_NOTIFICATION_POLICY` | Loud where the platform allows, never past the user's DND choice | `Ringer.playSound` |
| 3 | The alarm volume is raised to its maximum for the ring and put back when it stops; transient audio focus with alarm attributes | The phone is lost; a ring at volume 1 finds nothing | `Ringer.playSound`, `stopSound` |
| 4 | Vibration with alarm attributes (`VibrationAttributes.USAGE_ALARM` on 13+), a repeating pattern of long buzzes | Felt in a pocket or under a cushion | `Ringer.vibrate`, `RingPlan` |
| 5 | Notification channel `w6_ring` (high importance, silent channel because the sound is ours), category `ALARM`, public on the lock screen, one action **"Topdim"**; swiping it away counts as found | One tap stops it from anywhere | `Ringer.notify` |
| 6 | **Full-screen intent rules on Android 14+:** `USE_FULL_SCREEN_INTENT` is declared, but the system grants it by default only to calling and alarm apps, and the user can revoke it. The full-screen `RingActivity` is attached only when `canUseFullScreenIntent()` is true; otherwise the ring is a heads-up notification. Android 13 and older get the full-screen page | A full-screen intent without the grant silently degrades anyway; asking the system keeps the behaviour honest. Play review of the permission is an owner question | `RingPlan.fullScreen`, `RingActivity` |
| 7 | `RingActivity` is native and tiny (shows over the lock screen, turns the screen on, one big "Topdim" button, Back = found) | It must appear at once, even with no Flutter engine running | `RingActivity.kt` |
| 8 | Ends: **found** (button, action, swipe, Back) → event `w6.ring` `found`, so Dart sends `Session::stop_ring()` and the desktop shows "topildi"; **timeout** → event `timeout`; the sender's **Stop** (`w6.ringStop`) → no event | The desktop learns only what the phone's user did | `Ringer.finish`, `RingPlan.End` |
| 9 | No foreground service of its own. A ring arrives while the process is alive (open session, or a transfer holding it, ADR-0012 decision 11); a background `startForegroundService` would be refused on Android 12+ anyway | Same limit as the background receive; UniFFI removes it | — |

## What W1 wires

1. In the incoming handler: `Incoming.ring` `Start { secs }` → `w6.ringStart` with `secs` and the device's name;
   `Stop` → `w6.ringStop`.
2. On `w6.ring` `found` → `stop_ring()` on that session.
3. Device page: "Kompyuterni jiringlatish" → `ring(DEFAULT_RING_SECS)` for desktops.
4. `w6.permissions` `ring` once (notifications on 13+), at the first ring setting or pairing.

## Verified

- `RingPlanTest` (4): bounds, pattern, full-screen rule, which ends tell Dart.
- `RingerTest` (4, Robolectric SDK 34): the notification carries "Topdim"; found clears it and emits `found`; the
  sender's stop emits nothing; it stops by itself after `secs` with `timeout`.

## Device checklist (owner)

- [~] 1. Phone on silent, screen off, desktop `vutric-net-spike ring --ticket LINK --seconds 10`: loud alarm sound + vibration; the command exits 3 after 10 s.
- [~] 2. Same, tap "Topdim" on the lock screen: sound stops at once; the command exits 0 ("found").
- [~] 3. `--stop` from the desktop: the phone stops, no "found" on the desktop.
- [~] 4. Android 14+, a fresh install: the ring is a heads-up (no full-screen grant); after granting "full screen notifications" in system settings: the full-screen page over the lock screen.
- [~] 5. Do Not Disturb with alarms blocked: no sound (expected), the notification still shows.
- [~] 6. Alarm volume set to 1 before the ring: loud during the ring, back to 1 afterwards.

## Consequences

- iOS keeps the ringer volume (no Critical Alerts entitlement); W1's issue #18 covers it.
- If Play refuses `USE_FULL_SCREEN_INTENT` for Vutric, removing the one manifest line leaves the heads-up path,
  which already works without it.
