# Roadmap

Each phase ends with a written report (what works, what does not, measured numbers) and an explicit go/no-go
from the owner. Platform order is fixed by [ADR-0003](decisions/0003-platform-order.md).

## Phase 0 — spikes (prove the two risky parts)

| # | Spike | Exit criteria |
|---|---|---|
| 0.1 | **Overlay on Windows 11**: transparent, click-through, topmost, never-activating wgpu surface; a textured card flies in from the Send Zone edge through the "Halqa" ring. | ≥ 60 FPS (frame-time p99 < 16.7 ms), first frame < 16 ms after trigger from a warm window, zero focus loss (typing in another app is never interrupted), no flicker on show/hide, per-pixel alpha on Intel, AMD and NVIDIA. |
| 0.2 | **Network**: iroh endpoint on the desktop + a minimal Android client; mDNS discovery, QR pairing, 30 KB preview send. | Release → preview received p50 < 40 ms, p95 < 80 ms on a home Wi-Fi; fallback to QR IP:port when multicast is blocked. |
| 0.3 | **Overlay on KDE Wayland** (layer-shell) and **GNOME 50** (helper extension). | Same as 0.1. Done in Phase 0 only as a feasibility check; the product work is Phase 2. |

Status: 0.1 in progress (this repo). 0.2 waits for the Flutter/Android toolchain on the dev machine.

## Phase 1 — Windows + Android (MVP)

Desktop: daemon with tray (Telefonni ulash, Sozlamalar, To'xtatib turish, Chiqish), QR pairing window, overlay with
the Halqa effect, save to `Downloads\Vutric`, Windows toast (Ochish / Papkada ko'rsatish), settings UI (effect,
Send Zone left/right/top, duration, save folder, autostart), MSIX + winget manifest.
Phone: onboarding + permissions, QR scan (manual IP fallback), recent grid with desktop chip, Share-sheet target,
fling screen with velocity threshold + spring back + haptic, history with progress and resend.

## Phase 2 — Ubuntu + Android

`vutric-host-wlr` (KDE Plasma, wlroots, COSMIC), GNOME Shell extension + D-Bus (GNOME 46 and 50), systemd user
unit, first-run checklist that asks once to enable the extension, `.deb` + PPA, extensions.gnome.org listing.

## Phase 3 — Windows + iOS

iOS client (Flutter + same Rust core): `NSLocalNetworkUsageDescription`, `NSBonjourServices`, multicast entitlement
request, PhotoKit, HEIC→JPEG option, Share Extension. TestFlight.

## Phase 4 — Linux + iOS

Cross-check of the Phase 2 Linux daemon against the iOS client; nothing new on the protocol side by design.

## Phase 5 — macOS (last)

`vutric-host-mac`: borderless `NSPanel` at `.screenSaver` level, `ignoresMouseEvents`, non-activating,
`canJoinAllSpaces`, Metal via wgpu. Notarised `.dmg`. Works with both Android and iOS phones.

## Later (any phase, owner decides)

More effects (Kometa, Pufak, Oddiy + new ones), video and multi-file flings, clipboard, multi-monitor targeting,
LocalSend protocol compatibility, Play Store / App Store / Microsoft Store listings.
