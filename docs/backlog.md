# Backlog: everything we have decided or promised, in one list

Rule: nothing that the owner asked for or a research document proposed may live only in a chat. It goes here
with its source and phase. Status: `[x]` done · `[~]` built, not yet verified on real devices · `[ ]` open.
A line is removed only when a new ADR drops it, and the ADR is then named in the line.

Sources:

- ADRs `0001`–`0010` in [decisions/](decisions/)
- [protocol.md](protocol.md)
- reports [0.1](reports/phase-0.1-windows-overlay.md) and [0.2](reports/phase-0.2-network.md)
- research documents (Uzbek):
  - [transfer-methods.md](research/transfer-methods.md) (`TM §n`)
  - [airdrop-alternativa.html](research/airdrop-alternativa.html) (`AA §n`)
  - [vutric-stack-tavsiya.html](research/vutric-stack-tavsiya.html) (`ST §n`)
  - [vutric-arxitektura.html](research/vutric-arxitektura.html), the state on 2026-11-18
- [original-brief.md](research/original-brief.md)
- How the work is split between parallel sessions: [workstreams.md](workstreams.md)

## 0. Right now (closing spike 0.2)

| # | Item | Source | Status |
|---|---|---|---|
| N1 | Try the gallery-first APK on a real phone: gallery, multi-select, "Ulanib yuborish", Settings, forwarding received files | ADR-0008 | [~] |
| N2 | Preview latency on real Wi-Fi, router WAN unplugged, both directions: p50 < 40 ms, p95 < 80 ms | roadmap 0.2, ADR-0005 | [ ] |
| N3 | Connect by the link's IP:port with multicast blocked | report 0.2 | [ ] |
| N4 | iPhone 16 Pro ⇄ Windows with a fresh IPA: same numbers, watch the iOS "Local Network" prompt | TM §4.5 | [ ] |
| N5 | Phone ⇄ phone (two Android devices) and desktop ⇄ desktop | ADR-0005 | [ ] |
| N6 | 1 GB file over 5 GHz and 2.4 GHz, over LAN and hotspot: MB/s, CPU, battery; target ≥ 30 MB/s on 5 GHz LAN | AA §10 | [ ] |
| N7 | Release-build file speed (today 46.6 MB/s is a debug build on loopback) | ADR-0006 | [ ] |
| N8 | Commit today's work (gallery, settings screen, ADR-0008/0009/0010, backlog, research HTMLs) to PR #6 | session 2026-11-18 | [x] |
| N9 | Finish the 0.2 report with measured numbers → owner go/no-go | roadmap | [ ] |

## 1. Our own routes (ADR-0007, TM §2, §4.2)

| # | Route | Phase | Status |
|---|---|---|---|
| R0 | Same network + internet direct/relay (n0) | 0.2 | [x] |
| R1 | Stronger LAN finding:<br>• every address in the QR: link-local IPv6, 169.254, tethering subnets<br>• unicast probe<br>• opt-in subnet scan<br>• LocalSend-style multicast inside `224.0.0.0/24`<br>• iOS `localNetworkDenied` detection, with a screen that explains it<br>• AP-isolation detection: internet works, no peer answers | 1 | [ ] |
| R1a | Captive-portal and guest-Wi-Fi message: "this network keeps devices apart", then offer R2/R3 | TM §2.5 | [ ] |
| R2 | Our own `iroh-relay`: ACME TLS, rate limit, on a Vutric domain; n0 only as fallback; route badge and live speed; publish what the relay operator can see (Syncthing lesson) | 1 (Phase 1 does not ship on n0) | [ ] |
| R3 | Hotspot:<br>• Android LocalOnlyHotspot; the QR carries SSID, password and ticket<br>• iPhone joins with `NEHotspotConfiguration`<br>• Windows, Linux and macOS join<br>• a Windows host through `NetworkOperatorTetheringManager`<br>• fallback for MIUI/HarmonyOS without LOHS<br>• iOS may fall back to a saved network | 1–3 | [ ] |
| R4 | BLE for finding and handshake only: rotating IDs (never contact hashes); carries the ticket and hotspot keys; wake-up; ≤ 64 KB payloads | 1–3 | [ ] |
| R5 | USB cable: Android ADB reverse / tethering / AOA; iPhone usbmuxd (peertalk), after an App Review check | 1–3 | [ ] |
| R5a | Ethernet / Thunderbolt / USB4 cable between two computers (link-local IPv6 + mDNS) | 1–2 | [ ] |
| R6 | Wi-Fi Aware within one platform: Android 8+; iOS 26+ with a Network.framework adapter (unverified) | after 3 | [ ] |
| R7 | Wi-Fi Direct: Android ⇄ Android, Android ⇄ Windows | after 3 | [ ] |
| R8 | Encrypted cloud handoff (opt-in): S3-compatible storage, key in the link fragment, expiry, size limit; iOS background URLSession | opt-in | [ ] |
| R9 | Animated QR (libcimbar, ≤ 33 MB, ~106 KB/s); TXQR as a lighter option | opt-in | [ ] |
| R9a | Pairing code by sound (ggwave, 8–16 B/s), code only | idea | [ ] |
| R10 | iPhone ⇄ Android Wi-Fi Aware, re-checked every quarter | watched | [ ] |
| Rx | "Avtomatik" order: same network → internet direct → our relay → offer hotspot, Aware or cable; plus a route picker (Avtomatik, Faqat shu tarmoq, Internet orqali, Tarmoqsiz, Bluetooth) | ADR-0006 §2 | [ ] |
| Rb | Big files: consider `iroh-blobs` for verified streaming; never push 10–100 GB through the relay | TM §2.6 | [ ] |
| Rn | Not built, on purpose: Bluetooth Classic, NFC P2P, LoRa, bulk data over ultrasound | ADR-0007 | — |

## 2. Other apps' protocols (ADR-0010)

| # | Adapter | Phase | Status |
|---|---|---|---|
| C1 | LocalSend v2.1, send and receive: desktop, Android; iOS in 3; macOS in 5 | 1 | [ ] |
| C2 | Browser page, no app needed, upload and download (LocalSend "Web share" / qrcp) | 1 | [ ] |
| C3 | Quick Share LAN mode (NearDrop `PROTOCOL.md`): Windows 1, Linux 2, macOS 5; labelled "eksperimental" until it has survived 2 Quick Share releases | 1/2/5 | [ ] |
| C4 | KDE Connect v8, share plugin: desktop, Android | 2 | [ ] |
| C5 | Warpinator (gRPC, group code) | 2 | [ ] |
| C6 | croc (PAKE, relay) | 2 | [ ] |
| C7 | magic-wormhole (mailbox + transit) | 2 | [ ] |
| C8 | Adapter rules:<br>• clean-room code, no GPL copying<br>• foreign peers are always asked about unless pinned<br>• visible only in receive mode; "everyone" ends after 10 min<br>• sandboxed process for pre-auth parsing<br>• fuzzer per adapter<br>• real-app interop test per adapter<br>• protocol version re-read before writing the adapter | 1+ | [ ] |
| C9 | Settings `compat.*`: per-adapter `enabled` (LocalSend and browser on by default) and `pinned` | 1 | [ ] |
| Cn | Not possible: AirDrop, Continuity, Samsung/Huawei/Mi Share, Xender/SHAREit/Zapya/Send Anywhere/Blip/Feem, Phone Link, Nearby Sharing (ADR-0010 §2) | — | — |

## 3. Security (ADR-0005, ADR-0007 §2, report 0.2, TM §3)

| # | Item | Phase | Status |
|---|---|---|---|
| S1 | Trust store of paired keys: an unknown device needs an explicit OK | 1 | [x] ADR-0050, `vutric_core::trust`, `vutric_net::trust`, `tests/trust_gate.rs` |
| S2 | Commit-then-reveal numeric comparison before mDNS pairing is offered (today the 6 digits are shown, not confirmed) | 1 | [x] ADR-0050, `vutric_core::sas`, `Session::pair_code`/`pair_finish` |
| S3 | Default: only trusted devices can send; "everyone" switches off after 10 min | 1 | [x] ADR-0050, `Visibility`, `Peer::accept_with` (UI toggles: W1/W2) |
| S4 | Fuzz the framing and the `Offer` parser in CI (protocol-aware) | 1 | [ ] |
| S5 | Every route: bounded fixed frames before auth, same encryption for every frame type, 6-digit TLS-exporter code | all | [ ] |
| S6 | Privacy line on screen A1: "Fayllar shifrlangan, faqat siz va qabul qiluvchi ko'radi"; the Lan-only setting brings back "Hech narsa internetga chiqmaydi" | 1 | [ ] |
| S7 | The device name is never published to lookup | 0.2 | [x] |

## 4. Storage settings (ADR-0007 §4, ADR-0008)

| # | Item | Status |
|---|---|---|
| T1 | On the phone screen: accept mode, gallery + album + keep copy, split by type, subfolder, keep tree, conflicts | [~] |
| T2 | Save folder of the user's choice: Android SAF (`SafStore`), iPhone Files bookmark, desktop folder dialog; a missing folder falls back to the default and tells the user once | [ ] |
| T3 | Per-device overrides (`devices`) on the device detail screen | [ ] |
| T4 | Free-space guard (refuse when < 1 GB would remain), checking app storage too on phones | [ ] |
| T5 | After receiving: notification / open file / open folder / nothing | [ ] |
| T6 | Unfinished parts kept 7 days (1–30, "delete now"); history 90 days | [ ] |
| T7 | "Ask" option for name conflicts | [ ] |
| T8 | Desktop settings window with the same groups in the same order | [ ] |
| T9 | Versioned schema with migrations from the first release | [~] (`version` field exists) |

## 5. Desktop (roadmap Phase 1–2, ADR-0009 §4, ST §3–5)

| # | Item | Phase | Status |
|---|---|---|---|
| D1 | `vutric-daemon`:<br>• tray menu: Telefonni ulash, Sozlamalar, To'xtatib turish, Chiqish<br>• toast: Ochish / Papkada ko'rsatish<br>• IPC (named pipe)<br>• autostart<br>• ≤ 30 MB idle | 1 | [ ] |
| D2 | Overlay cards ("Qabul qilasizmi?", progress ring, "tushdi") drawn in the overlay, ≤ 16 ms, clickable without focus; fallback is a small Flutter window | 1 | [ ] |
| D3 | QR pairing window | 1 | [ ] |
| D4 | Flutter desktop windows (settings, history, devices, big send window), separate process, 0 MB when closed | 1 | [ ] |
| D5 | Send from the desktop: drag onto the Send Zone; Explorer menu "Vutric bilan yuborish"; Share contract | 1 | [ ] |
| D6 | Settings UI: effect, Send Zone (left/right/top), duration, save folder, autostart | 1 | [ ] |
| D7 | MSIX + winget; installer firewall rule for the Private profile; first-run hint for Public networks | 1 | [ ] |
| D8 | Overlay checks: NVIDIA and AMD dGPU, multi-monitor with mixed DPI, Windows 11; choose the monitor | 1 | [ ] |
| D9 | GPU needing over 2 s to start: the first fling must not be refused | 1 | [ ] |
| D10 | Linux: `vutric-host-wlr` (KDE, wlroots, COSMIC), GNOME 46/50 extension + D-Bus, systemd user unit, one-time extension checklist, Nautilus/Dolphin scripts, StatusNotifier, `.deb` + PPA, extensions.gnome.org | 2 | [ ] |
| D11 | macOS: `NSPanel` overlay, notarised `.dmg` | 5 | [ ] |

## 6. Android (ADR-0009 §2, roadmap Phase 1)

| # | Item | Status |
|---|---|---|
| A1 | Gallery-first home, multi-select, one send button | [~] |
| A2 | Onboarding + permissions; QR scan with manual IP as fallback | [~] |
| A3 | `ShareEntry`: Share target and sharing shortcuts for paired devices | [~] |
| A4 | `TransferJob`: user-initiated data transfer job (14+), `dataSync` foreground service before that | [~] |
| A5 | `VutricTile`: Quick Settings "receive mode" | [~] |
| A6 | `LinkManager`: hotspot (1), Wi-Fi P2P and Aware (later) | [~] |
| A7 | `Nearby`: NSD + MulticastLock (1), BLE (R4) | [~] |
| A8 | `SafStore`: SAF folder and MediaStore move after verification | [~] |
| A9 | Fling screen: velocity threshold, spring back, haptic | [~] |
| A10 | History with progress and resend | [~] |
| A11 | Receive: Halqa in the app when open, otherwise a heads-up notification with a thumbnail; no `SYSTEM_ALERT_WINDOW` | [~] |
| A12 | `Companion` (notification mirroring) and `Clipboard`, opt-in | [ ] |
| A13 | Kotlin `ForeignDiscovery` for LocalSend and KDE Connect | [ ] |
| A14 | Apply the Claude Design UI (`design/claude-design/`) to every phone screen, improved where it falls short | [~] |

## 7. iPhone (ADR-0009 §3, roadmap Phase 3)

| # | Item | Status |
|---|---|---|
| I1 | Share Extension in SwiftUI (no Flutter), streaming through Rust | [ ] |
| I2 | Live Activity + widget ("PC'ga yuborish") | [ ] |
| I3 | App Intents: Siri, Shortcuts, Action button | [ ] |
| I4 | Discovery: `NWBrowser`/`NWListener` `_vutric._udp`, CoreBluetooth; `NSLocalNetworkUsageDescription`, `NSBonjourServices`, multicast entitlement request | [ ] |
| I5 | `NEHotspotConfiguration` join | [ ] |
| I6 | PhotoKit gallery, HEIC → JPEG option | [ ] |
| I7 | Files folder bookmark | [ ] |
| I8 | App Group + Keychain group shared by the app and its extensions | [ ] |
| I9 | Check whether `BGContinuedProcessingTask` (iOS 26) can keep a P2P transfer running | [ ] |
| I10 | TestFlight | [ ] |

## 8. Competitor features in one app (ST §9)

| # | Feature (who has it) | Where it is planned | Status |
|---|---|---|---|
| F1 | Send from the Share menu (AirDrop, Quick Share, LocalSend) | A3, I1, D5 | [~] |
| F2 | Device shortcut in the Share sheet (Quick Share) | A3 | [ ] |
| F3 | Find nearby devices automatically | R1, R4, A7, I4 | [ ] |
| F4 | No internet, no router | R3, R6, R7 | [ ] |
| F5 | Different networks over the internet (PairDrop) | R0, R2 | [x] (n0) |
| F6 | Resume after a drop | protocol | [x] |
| F7 | Background transfer with the app closed (Quick Share) | A4, D1 | [ ] |
| F8 | One-tap tile or button | A5, I2, D1 | [~] |
| F9 | Progress on the lock screen | A4 notification, I2 | [~] |
| F10 | Siri, Shortcuts, automation; desktop CLI | I3, `net-spike` CLI | [ ] |
| F11 | PIN mode and auto-accept (LocalSend, Quick Share) | S1, S3, ADR-0007 | [~] (auto-accept setting) |
| F12 | Clipboard sharing (KDE Connect, Apple) | A12 | [ ] |
| F13 | Notifications mirrored to the PC (KDE Connect, Phone Link) | A12 | [ ] |
| F14 | Ring my phone (KDE Connect) | Kotlin + Swift module, after Phase 1 | [ ] |
| F15 | Phone as a mouse for the PC (KDE Connect) | Flutter + daemon input, later | [ ] |
| F16 | Receive with no app, from a browser link (LocalSend, PairDrop) | C2 | [ ] |
| F17 | LocalSend compatibility | C1 | [ ] |
| F18 | "Wow" throw effect (Canopy Portal, Mac only) | overlay | [x] Windows |
| F19 | Simple first screen with no setup (LocalSend lesson) | A1, A2 | [~] |

## 9. Effects (effects.md)

| # | Item | Status |
|---|---|---|
| E1 | Halqa | [x] |
| E2 | Kometa: a streak with a particle tail | [ ] |
| E3 | Pufak: a refractive bubble that pops on landing | [ ] |
| E4 | Oddiy: a fast, quiet slide, also the reduce-motion choice | [ ] |
| E5 | Many files: the first one is thrown, the rest form a stack with a counter | [ ] |

## 10. Measurements owed (beyond §0)

| # | Measure | Source |
|---|---|---|
| M1 | Desktop UI spike: Flutter, Tauri and Slint, with LocalSend as the reference. Measure RAM (idle, open, after close), cold open ≤ 400 ms and fps | ADR-0009 |
| M2 | Share of direct versus relay connections (opt-in anonymous telemetry) | ADR-0007 §5 |
| M3 | LocalOnlyHotspot on 2.4 GHz versus 5 GHz | ADR-0007 §5 |
| M4 | iPhone ⇄ Windows and Android ⇄ Windows over LAN and hotspot | TM §4.5 |
| M5 | Each adapter against the real app it speaks to (C8) | ADR-0010 |

## 11. Later (owner decides)

- Multi-monitor targeting.
- Store listings: Play Store, App Store, Microsoft Store.
- The `vutric.io` domain: free on 2026-11-17, not registered.
- Dependabot PRs #2 and #3. They are open because of a startup failure; pollster 1.0 is an API break.
- The design brief for the ADR-0007 screens. It is kept with the design references, outside the repo.
