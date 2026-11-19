# Roadmap

Each phase ends with a written report (what works, what does not, measured numbers) and an explicit go/no-go
from the owner. Platform order is fixed by [ADR-0003](decisions/0003-platform-order.md).

## Phase 0 — spikes (prove the two risky parts)

| # | Spike | Exit criteria |
|---|---|---|
| 0.1 | **Overlay on Windows 11**: transparent, click-through, topmost, never-activating wgpu surface; a textured card flies in from the Send Zone edge through the "Halqa" ring. | ≥ 60 FPS (frame-time p99 < 16.7 ms), first frame < 16 ms after trigger from a warm window, zero focus loss (typing in another app is never interrupted), no flicker on show/hide, per-pixel alpha on Intel, AMD and NVIDIA. |
| 0.2 | **Network, any to any, offline** ([ADR-0005](decisions/0005-any-to-any-offline-first.md)): one LAN-only iroh peer (`crates/vutric-net`) on desktop and Android; mDNS discovery, QR pairing link, six-digit pairing check, 30 KB preview **in both directions**. | Preview one-way p50 < 40 ms, p95 < 80 ms on a home Wi-Fi with the router's internet unplugged, desktop → phone and phone → desktop; fallback to the link's IP:port when multicast is blocked. Phone ↔ phone and desktop ↔ desktop when two such devices are at hand. |
| 0.3 | **Overlay on KDE Wayland** (layer-shell) and **GNOME 50** (helper extension). | Same as 0.1. Done in Phase 0 only as a feasibility check; the product work is Phase 2. |

Status: 0.1 passed on Windows ([report](reports/phase-0.1-windows-overlay.md)). 0.2 in progress ([report](reports/phase-0.2-network.md)):
`vutric-net`, the desktop CLI `apps/net-spike` and the phone app `apps/mobile-spike` (Android, plus an unsigned
iPhone build for sideloading, pulled ahead of Phase 3 only to measure on the owner's iPhone); device runs on real
Wi-Fi are next.

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

## Routes and storage settings (across phases, [ADR-0007](decisions/0007-every-route-and-storage-settings.md))

| When | Work |
|---|---|
| Spike 0.2 (done) | R0 same network + internet; any files, many at once, resume, hash check (ADR-0006) |
| Spike 0.2 (done, not yet on a real phone) | Phone opens on its own gallery + files, multi-select, one send button; receiving settings screen ([ADR-0008](decisions/0008-gallery-first-phone-app.md)) |
| Phase 1 | R1 stronger LAN finding + failure screens · R2 our own relay + route badge · storage settings on desktop and Android (save folder, split by type, subfolders, gallery, conflicts, auto-accept, free-space guard, history) · R3 hotspot (Android host) · R4 BLE finding · R5 USB (Android) |
| Phase 3 | storage settings on iPhone (Files folder bookmark, PhotoKit) · R3/R4 on iPhone · R5 USB (iPhone, after App Review check) · R6 Wi-Fi Aware within one platform |
| After Phase 3 | R7 Wi-Fi Direct · R8 encrypted cloud handoff (opt-in) · R9 animated QR |
| Watched | R10 iPhone ⇄ Android Wi-Fi Aware, re-checked every quarter |

## OS modules and UI layers (across phases, [ADR-0009](decisions/0009-native-modules-kotlin-swift.md))

| When | Work |
|---|---|
| Spike 0.3 (before Phase 1 UI) | desktop UI spike: Flutter desktop vs Tauri vs Slint, LocalSend as reference; overlay card without focus; RAM / cold open / fps table → owner confirms the desktop toolkit |
| Phase 1 | Rust daemon (tray, toast, IPC, autostart) + overlay cards · Flutter desktop windows · FRB + UniFFI in one `libvutric.so` · Kotlin: `TransferJob`, `ShareEntry`, `VutricTile`, `LinkManager` (hotspot), `Nearby` (mDNS), `SafStore` · Explorer menu, MSIX/winget |
| Phase 2 | Flutter Linux · Nautilus/Dolphin scripts, StatusNotifier · Kotlin: BLE (`Nearby`), Wi-Fi P2P (`LinkManager`) |
| Phase 3–4 | Swift: Share Extension (no Flutter), Live Activity + widget, App Intents, Bonjour/CoreBluetooth discovery, `NEHotspotConfiguration`, PhotoKit + HEIC→JPEG, Files bookmark · App Group + Keychain group · TestFlight |
| Phase 5 | macOS `NSPanel` overlay · Flutter macOS · Swift share extension |
| Opt-in, Phase 1+ | Kotlin `Companion` (notification mirroring), `Clipboard` |

## Other apps' protocols (across phases, [ADR-0010](decisions/0010-interop-every-app.md))

| When | Work |
|---|---|
| Phase 1 | LocalSend v2.1 adapter (desktop + Android) · browser send/receive page · Quick Share LAN adapter on Windows (eksperimental) · sandboxed adapter processes + fuzzers + real-app tests |
| Phase 2 | Quick Share on Linux · KDE Connect (desktop + Android) · Warpinator · croc · magic-wormhole · LocalSend on Linux |
| Phase 3 | LocalSend on iPhone (foreground) |
| Phase 5 | LocalSend and Quick Share (QR, NearDrop style) on macOS |
| Not possible | AirDrop, Apple Continuity, Samsung/Huawei/Mi Share, Xender/SHAREit/Zapya protocols, Phone Link: closed (ADR-0010 part 2) |

## Everything in one list

[backlog.md](backlog.md) lists every proposal, feature, adapter and owed measurement from the ADRs and the research
documents, each with its source and phase, so nothing is forgotten.

## Later (any phase, owner decides)

More effects (Kometa, Pufak, Oddiy + new ones), clipboard, multi-monitor targeting,
Play Store / App Store / Microsoft Store listings.
