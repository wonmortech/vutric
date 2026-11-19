# Vutric site — capability & case map (content source for the vau rebuild)

**Date:** 2026-11-19 · **Decision:** [ADR-0016](../decisions/0016-halqa-3d-signature.md) · **Direction:** [DESIGN.md](../../DESIGN.md)
**Source of truth:** `docs/**` (roadmap, backlog, protocol, architecture, relay, effects, workstreams, owner-questions),
both phase reports, ADRs 0001–0015 + 0050–0068, `design/claude-design/README.md`.

This is the content the site must present so a visitor can see **every capability and every case** — built or not.
It is a living document: it cites ADR / backlog ids so drift from the code is visible. The site build (a later
surface) consumes this; no claim appears on the site that is not here with a source.

## Status chips (the only statuses the site uses)

| Chip | Meaning | backlog marker |
|---|---|---|
| **Built** | implemented, tests pass now | `[x]` |
| **Built · verifying on devices** | code/engine done + CI-tested, never run on a real phone / relay / hotel-Wi-Fi / cable | `[~]` |
| **In progress** | being built/measured now (spike 0.2 closeout) | §0 |
| **Planned P1…P5** | on the roadmap for a named phase | `[ ]` + phase |
| **Opt-in** / **Watched** / **Idea** | ships off / re-checked quarterly / noted, unscheduled | roadmap |
| **Not possible** | closed protocol (ADR-0010 §2) | `—` |

## ⚠️ Honesty banner (non-negotiable — ADR-0007 §3)

- **No real-device Wi-Fi throughput number exists yet.** Every transfer-speed figure in the docs is loopback and/or
  a debug build. The site may **not** present any of them as real-world speed.
- The **only** production-representative live measurement is the **overlay FPS/latency** (Phase 0.1). Those, and the
  loopback latency figures *clearly labelled as loopback*, are the only numbers the site may quote — see §B.
- **"Built · verifying on devices" reads as "built, verifying", never "works".** No "faster than AirDrop", no
  "always P2P", no "100% offline" unless the UI is in a state where it is literally true.

## Proposed site section order (vau → completeness → reference)

1. **Hero** — the 3D Halqa throw (the signature). One line: *flick a photo off your phone, it lands on your computer.*
2. **Differentiators** (shown first): offline / no account · the throw gesture · any-to-any, any file.
3. **How it connects** — the route tour (§3) with the Avtomatik fallback chain as a live diagram.
4. **The gesture & effects** (§1), **platforms & roadmap** (§2), **discovery & pairing** (§4), **security & trust** (§5),
   **storage & settings** (§6), **the phone app, every state** (§7), **the desktop** (§8), **works with other apps** (§9),
   **files & behaviour** (§10), **companion extras** (§11), **themes & design system** (§12).
5. **When things go wrong** — the failure/edge-case story (§A).
6. **Design-system reference** — the current colour/type/components/screens page, kept beneath.

---

## 1. The gesture & effects

The gesture *is* the product (ADR-0005; ADR-0011 #6). "Fling" names the gesture, not the brand (ADR-0002).

**Throw (phone):** flick-to-send; threshold `dy < −150 px` OR `vy < −0.5 px/ms` (500 px/s); out 280 ms
`cubic(.3,.7,.4,1)`, spring-back 560 ms `cubic(.34,1.56,.64,1)`; live tilt `dx·0.05° − up·0.02°`, shrink ≤14%,
portal glow `min(up/170,1)`; light haptic + result pill 220 ms/+8 px near the thumb — all **Built · verifying on
devices** (ADR-0011 #6; design README).

**Effects (receiver side):**

| Effect | One-line | Status | Source |
|---|---|---|---|
| **Halqa** (default) | luminous portal opens, item flies out and lands | **Built** (Windows overlay) | effects.md; E1; report 0.1 |
| Halqa phases | Open 0–24% → Fly 10–62% (+4 contour echoes) → Land (spring 3.2 Hz ζ 0.42) → Close → Hold 1.4 s → Dock | **Built** | effects.md |
| Oddiy | fast quiet slide-in; also the reduced-motion fallback | Planned P1 (W2) | E4; effects.md |
| Kometa | streak with a particle tail along the throw direction | Planned (later) | E2; effects.md |
| Pufak | arrives in a refractive bubble that pops on landing | Planned (later) | E3; effects.md |
| Many-file stack | first file thrown, the rest stack with a counter | Planned P1 (W2) | E5; ADR-0006 §3 |
| Reduced motion | every throw/scan/pulse → 200 ms fade / still | Built (phone, verifying) · Planned P1 (desktop) | effects.md; ADR-0011 #11 |
| Send Zone position | portal anchored left / right / top edge | Planned P1 (W2) | D6 |
| Choose monitor | pick which monitor plays the effect | Planned P1 (D8); multi-monitor broader = Later | D8 |

**Preview-first:** the effect starts on a ~30 KB preview while the full file streams, so a 40 MB video feels as
instant as a photo (architecture.md; protocol.md).

## 2. Platforms × phase (order fixed by ADR-0003)

| Platform | Phase | Receive tech | Status |
|---|---|---|---|
| **Windows 10/11** | P1 | layered click-through + DirectComposition/wgpu, never-activating | overlay **Built** (0.1 passed); daemon Planned P1 |
| **Android** | P1 | in-app Halqa; else heads-up notification w/ thumbnail | Built · verifying on devices |
| **Ubuntu — KDE/wlroots/COSMIC** | P2 | `vutric-host-wlr`, layer-shell | feasibility checked (0.3); product Planned P2 |
| **Ubuntu — GNOME 46/50** | P2 | window + Shell extension over D-Bus | Planned P2 |
| **iOS** | P3 | receives app-open; Share Ext, Live Activity, widget, App Intents, PhotoKit | modules CI-built · verifying; TestFlight Planned P3 |
| **Linux + iOS cross-check** | P4 | no new protocol | Planned P4 |
| **macOS** (last) | P5 | borderless `NSPanel` `.screenSaver`, Metal via wgpu | Planned P5 |

Overlay refusal rule: if an adapter offers no transparent composite, the overlay **refuses to start** rather than
black out the desktop (ADR-0004).

## 3. How it connects — routes R0–R10 (+ extras)

Every route runs the same protocol; a route only changes how the QUIC connection is reached (ADR-0007 §1).
**Avtomatik order (ADR-0060):** same network → internet direct → our relay → offer hotspot / Wi-Fi Aware / cable.

| # | Route | Covers | Status | Source |
|---|---|---|---|---|
| R0 | same network + internet (direct or n0 relay) | same Wi-Fi; different networks | **Built** | R0; report 0.2 |
| R1 | stronger LAN finding (all addresses in QR, probe, opt-in scan, multicast, AP-isolation + iOS-denied detect) | multicast blocked, AP isolation | Built · verifying | R1; ADR-0052 |
| R1a | captive-portal / guest-Wi-Fi message + login URL, then offer R2/R3 | hotel / guest Wi-Fi walls | Built · verifying | R1a; ADR-0068 §1 |
| R2 | our own `iroh-relay` (ACME TLS, rate-limit, route badge + live speed) | strict/carrier NAT | Built · verifying; **prod domain = owner Q** | R2; ADR-0053; relay.md |
| R3 | hotspot (Android LOHS, iPhone `NEHotspotConfiguration`, Windows host) | no shared network | core Built · verifying | R3; ADR-0058 |
| R4 | BLE for finding + handshake only (rotating IDs, ≤64 KB) | no network, no QR scan | Planned P1+ | R4 |
| R5 | USB cable (Android ADB/tether/AOA; iPhone usbmuxd) | no radio, very large files | Planned | R5 |
| R5a | Ethernet/Thunderbolt/USB4 computer↔computer (link-local IPv6 + mDNS), "Kabel" badge | tetherless PC↔PC | Built · verifying | R5a; ADR-0068 §3 |
| R6 | Wi-Fi Aware within one platform (Android 8+, iOS 26+) | near, no network | Planned (after P3) | R6 |
| R7 | Wi-Fi Direct (Android↔Android, Android↔Windows) | near, no network | Planned (after P3) | R7 |
| R8 | encrypted cloud handoff (opt-in; key in link fragment, expiry) | receiver offline now | Opt-in / Planned | R8 |
| R9 | animated QR (libcimbar ≤33 MB; TXQR lighter) | air-gapped rooms | Opt-in / Planned | R9 |
| R9a | pairing code by sound (ggwave) | air-gap, code only | Idea | R9a |
| R10 | iPhone↔Android Wi-Fi Aware | near, cross-platform | **Watched** (blocked today) | R10 |
| Rx | Avtomatik order + route picker (Avtomatik / Faqat shu tarmoq / Internet / Tarmoqsiz / Bluetooth) | route selection | Built (engine); Bluetooth greyed until R4 | Rx; ADR-0060 |
| Rb | big files: ask > 2 GiB on relay, refuse > 10 GiB, wait 10 s for a direct path | 10–100 GB | Built · verifying | Rb; ADR-0068 §2 |
| Rn | deliberately NOT built: Bluetooth Classic, NFC P2P, LoRa, bulk-over-ultrasound | — | **Won't build** | Rn |

**Fallback chain** (ADR-0005 §2 + ADR-0052 + ADR-0060): mDNS → ranked IP:port hints in the ticket (wired → Wi-Fi →
tethering → other → virtual last) → zone-less `fe80::` per interface → unicast probe → opt-in `/24` scan (≤1024
hosts, 2 ms paced) → multicast `224.0.0.167:53319` → relay → then *offer* hotspot / Wi-Fi Aware / cable.

## 4. Discovery & pairing

mDNS/NSD discovery · QR pairing link `vutric://pair?v=2&id&n&a&r` (≤8 ranked hints) · six-digit commit-then-reveal
check (ADR-0050) · short code `ABCD-EFGH` (ADR-0059) · typed IPv4 · manual link paste · 41-byte LAN beacon
(ADR-0052) · LAN diagnosis verdicts (`LocalNetworkDenied`/`Isolated`/`OtherNetwork`/`NoNetwork`/`Captive`) ·
AP-isolation + iOS-denied detection · hotspot-in-the-link (ADR-0058) · standard `WIFI:` camera QR · desktop↔desktop
numeric-comparison pairing · companion/compat pairing via `PairProof`/`PairCommit`/guest (ADR-0064) · one-time QR
secret `s=` · trust store (remembered devices). **Built**; phone/LAN paths **verifying on devices**.

## 5. Security & trust

Trust store (keys only, ≤512, damaged→`.bad`) · admission gate before the first byte (refused = QUIC 403) ·
commit-then-reveal SAS (10⁻⁶/attempt) · "everyone for 10 min" auto-off · visibility modes
`off`/`trusted_only`(default)/`everyone(≤10m)`/`pairing(≤10m)` · pre-auth: only bounded fixed frames, 128 KiB cap ·
honest privacy line (LAN-only adds "nothing goes to the internet" **only when true**) · device name never published ·
proof bound to the TLS session (no replay) · **17 cargo-fuzz targets** (found 3 real bugs) · relay transparency
(operator sees keys/IPs/bytes, never names/files). All **Built** (ADR-0050, 0051, 0053, 0061, 0064).
*Known deviation:* LocalSend / Quick Share / KDE Connect / wormhole have no 6-digit exporter code by design — they
rely on pinned fingerprints / their own PIN + always-ask (ADR-0061/0062/0067).

## 6. Storage & settings (one owner: Rust `vutric-core`, ADR-0007 §4)

Save folder (per-OS default + any picked folder) · split by type · subfolder (none/device/date/device+date) ·
gallery album · folder tree kept/flattened · name-conflict keep-both/replace/skip/**ask** · accepting
ask/auto-trusted/auto-≤N MB · free-space guard (<1 GB, both volumes) · after-receiving notify/open-file/open-folder ·
unfinished parts kept 7 d · history 90 d + resend · route picker + live route badge · per-device overrides ·
Android SAF tree · **versioned schema + migrations** · desktop settings window (Planned P1) · compat settings
(`compat.*`, LocalSend+browser on by default). Engine **Built**; platform pickers **verifying on devices**.

## 7. The phone app — every state (Flutter, W1)

Shell: three tabs **Otish · Qabul · Tarix** + round side button (QR idle / peer connected), gallery-first (ADR-0008),
9 themes × 320 dp × 2× text tested (ADR-0011 #14/#15). All **Built · verifying on devices**.

- **S1 Home (3):** ready · preparing (QR sheet) · failed.
- **S2 Scanner:** corner brackets, animated scan line, `vutric --listen` hint, manual-link entry.
- **S3 Send (all 9):** ulanmagan · ulanmoqda · ulangan rasm yoʻq · rasm tayyor · sudrash paytida · uchib ketdi ·
  yetib bordi ("ekranda N ms") · rad etildi · javob yoʻq.
- **S4 Receive (all 5):** tanlov · kutish (my QR) · ulangan kutilmoqda · rasm keldi (flies in carrying its tilt) ·
  aloqa uzildi (Qayta ulanish).
- **Extras:** onboarding/permissions · Share-sheet target · history+resend (last 200) · editable device-name pill ·
  9 themes + Avto · Quick-Settings "Vutric: qabul" tile · background receive (heads-up notification).

## 8. The desktop (W2 — Planned P1 unless noted)

`vutric-daemon` (≤30 MB idle) · tray (Telefonni ulash / Sozlamalar / Toʻxtatib turish / Chiqish) · toast
(Ochish / Papkada koʻrsatish) · named-pipe IPC · autostart · **overlay cards that never take focus** + progress ring
(focus-less click proven in principle, ADR-0009) · QR pairing window · Flutter desktop windows (0 MB when closed) ·
drag onto the Send Zone · Explorer "Vutric bilan yuborish" · settings UI · MSIX + winget · installer firewall rule ·
GPU/multi-monitor checks + slow-GPU first-fling guard.

## 9. Works with other apps (interop, ADR-0010)

A file from any adapter gets the same Halqa, folder rules, gallery copy and history. Each adapter is clean-room,
asks the foreign peer unless pinned, is visible only in receive mode, sandbox-parses before auth, has a fuzzer and a
real-app test. **Real-app verification (M5) pending for all.**

| Adapter | Direction | Phase | Status |
|---|---|---|---|
| **LocalSend** v2.2 | send + receive | desktop+Android P1; iOS P3; macOS P5 | Built · real-app pending |
| **Browser page** (no app) | send + receive | desktop P1 | Built (real Chromium) · phones pending |
| **Quick Share** (LAN) | receive→send | Windows P1, Linux P2, macOS P5 | Built · "eksperimental" until 2 releases survive |
| **KDE Connect** v8 | send + receive | desktop+Android P2 | Built · real-app pending |
| **Warpinator** | send + receive | desktop P2 | **Planned — not started; owner Q** (GPL-only source) |
| **croc** | send + receive | P2 | **Parked** (no spec; owner Q) |
| **magic-wormhole** | send + receive | desktop+CLI P2 | Built (vs real 0.24.0) · phone + public-servers pending |
| Compat on the phone (`:compat`) | send + receive | Android P1 | Built · device + real-app pending |

**Not possible (ADR-0010 §2) — shown honestly, never hidden:** AirDrop (AWDL closed → use the browser page) ·
Apple Continuity/Universal Clipboard · Samsung/Huawei/Mi Share · Xender/SHAREit/Zapya/Send Anywhere · Phone Link /
Windows Nearby Sharing. Each row says *why not* and *what Vutric offers instead*.

## 10. Files & transfer behaviour

Any file type (both directions) · clipboard text (1 B–64 KiB) · many files at once (≤256/offer, asked once) ·
≤4 parallel QUIC streams · resume after a drop (missing ranges only, `.vutric-part`) · **BLAKE3 hash check** before
atomic rename · 30 KB preview both directions (≤96 KiB, ≤512 px) · preview hardening (JPEG-only, ≤1024 px, 16 MB) ·
HEIC→JPEG (off by default) · filename safety (device names, reserved chars, ≤32 levels — fuzzed) · big-file guard.
Engine **Built**; previews measured loopback, real Wi-Fi pending. Clipboard *images* deferred (ADR-0065 §5).

## 11. Companion / opt-in extras

Notification mirroring (opt-in, per-app allowlist, trusted only, **off by default / empty allowlist**) · clipboard
sync both ways (trusted session) · companion wiring caps/trust/rate-limit (**Built**). "Ring my phone" (F14) and
"Phone as a mouse" (F15) are **Planned — no ADR, no phase** → show as **Soon** (owner Q).

## 12. Themes & design system

9 app themes (tokens generated from the handoff, never hand-typed) · 5 site themes (Daylight/Graphite/Ocean/Sand +
brand) · **material morph** glass/outline/brutal via one `VSurface` · brand mark single "V" (Vutric Blue `#2969FF`;
iOS-native uses Apple `#007AFF` on purpose) · tokens (radius 8/14/22/999; Geist + Geist Mono) · **WCAG AA** in all 9
themes (CI-tested) · 48×48 touch + text scaling · reduced motion. The 9 theme **names are not pinned** (owner Q).

---

## A. When things go wrong — failure / edge-case story

Every case has a defined behaviour or a dedicated screen. The site tells this story (it is a differentiator).

| Case | What the user hits | Vutric's answer |
|---|---|---|
| Connection lost | peer disappears | S4 "aloqa uzildi" + Qayta ulanish |
| Rejected | receiver says no | S3 "rad etildi"; 403 → "pair first, or ask them to open Hamma 10 min" |
| Timeout | silent peer | S3 "javob yoʻq"; 8 s handshake / 5 s fling-answer |
| AP isolation | internet works, no peer | `Isolated` → offer relay/hotspot |
| Captive Wi-Fi | hotel login wall | message + login URL → SignIn / hotspot / cable |
| iOS Local Network denied | iOS blocks LAN | dedicated explainer screen |
| No multicast | router drops 5353 | fall back to QR IP:port hints / unicast probe |
| Windows "Public" Wi-Fi | inbound UDP blocked | installer firewall rule + first-run hint |
| Free space low | <1 GB would remain | refuse `NoRoom` (both volumes) |
| Name conflict | file exists | keep both / replace / skip / **ask** |
| Big file on relay | >2 GiB / >10 GiB | ask (offer hotspot/cable) / refuse after 10 s |
| Missing save folder | folder unplugged | falls back to default, tells once |
| Hash mismatch | corrupt transfer | delete part, list `failed`, retry from 0 |
| Hostile preview | malicious/huge image | decode capped, refused `FlingFailed` |
| Old QR screenshot | stale QR reused | `WrongLink` → "scan the QR again" |
| Stranger in pairing window | beacon stranger during your QR | `Neither` → guest, every offer asked |
| Slow GPU (>2 s) | dGPU wakes slowly | first fling not refused (daemon guard) |
| MIUI/HarmonyOS no LOHS | OEM lacks LocalOnlyHotspot | fall back to system hotspot, type SSID/pwd |

## B. The only numbers the site may quote (with their caveats)

**Overlay (production-representative) — Phase 0.1:**
- 60.3 FPS during Halqa (177 frames / 2.93 s, 5/5 runs); pacing p50 16.6–16.8 ms, p99 19.1–20.5 ms, 0 missed vblanks.
- First frame after trigger (warm): 0.56–10.67 ms. Focus taken: 0 in 15 runs. (Intel UHD 730, 1440p@60, Win10.)

**Network (loopback — label as loopback, NOT Wi-Fi) — Phase 0.2:**
- mDNS discovery 138 ms; QUIC connect + Hello both ways 6 ms; 30 KB preview RTT p50 0.2 ms / p95 0.4 ms (n=100 each
  way); pairing link 179 chars / 4 addresses; stalled-client fix 7.8 s → <0.4 s.

**File throughput (loopback, debug/release as marked):** 203 MB/2 files 4.4 s = 46.6 MB/s (debug); LocalSend 34–37
MB/s (debug); Quick Share 14 MB/s debug / 70–110 MB/s release; relay meet 33 ms then direct 3.9 ms RTT; interfaces
9 addresses in 2.7 ms. **Design target (not a live number):** finger release → first effect frame < 100 ms.

**Incidental:** release APK 86.7 MB (with compat adapters).

## C. Site baseline (before the vau rebuild) — measured 2026-11-19

Desktop 1440×900, live `vutric.wonmor.io`: FCP 168 ms · load 147 ms · **71 KB** (JS 7 KB, fonts 52 KB, img 1 KB) ·
7 requests · hero **60 FPS** · console clean · Lighthouse A11y 96 / BP 100 / SEO 100 / Agentic 50 · page height
7915 px · sections `top, colour, type, foundations, components, effect, screens, desktop`. Screenshots in the session
scratchpad (`before-01..04`). The rebuild must not regress this leanness (ADR-0016 decision 2).
