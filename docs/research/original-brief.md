# Original brief (archived as written)

> This is the planning prompt produced on 2026-11-17 during the mobile research session, kept verbatim as the
> historical source of truth. The working title was "Fling"; the product is now named **Vutric**. Where this
> brief and `CLAUDE.md` / `docs/roadmap.md` disagree, the newer documents win (for example the platform order
> and the visual style were changed afterwards — see `docs/decisions/`).

# Project: "Fling" — flick photos/files from a phone onto a Windows/Ubuntu desktop with a smooth animation

Talk to me in Uzbek (Latin script). Code, comments, commit messages and docs in English.

## My original requests (verbatim, in Uzbek — the source of truth for what I want)

These are the exact messages I wrote while planning this project, in order. Read them first; everything
below is the plan that was built from them.

1. Senga instageam link bersam videoni ko'ra olasanmi
2. (I attached a 17-second Instagram video) Shunday ishni windowsga ham qilsa bo'ladimi ?
3. Iltimos bu bo'yicha deeep izlanish qil butun dunyodan internetdan , githublardan, forumlardan izlan o'rgan har hil usulda qidir analiz qil . Men linux ubuntu va windows 10/11 uchun shunday qilmoqchiman animatsi huddi ahuday bo'lishi kerak barcha fayllar silliq o'tishi kerak eng yaxshi toollar eng zo'r eng tez texnalogiyalarni tanla .NEt C# bo'lishi shart emas eng zo'ri bo'lsin va reja arxetektura lik referense lar anniq teksgirlgan anniq reja tuz
4. Menga taxminan telefon (android) va windowsda skrenshot meyular bilan  eng boshidan tushuntirib bera olasanmi ?

   Misol uchun rejadagidek ishni qildik - men kompyuter va telefonga appni yuklab olishim kerakmi ? Ketin nima qilaman blutuz yoki wifi yoqiq bo'lishi kerakmi ? Men telefonnig o'zini galereyasidan yoki folderidagi fileni ushlab komyuterga silliq o'tkaza olamanmi yoki yangi appga kirib uning ichidan filelarni topib komyuterni tanlaymanmi  iltimos to'lliq ui/ux pipeline  ni taxminiy chizib tushuntirib ber.
5. https://getcanopy.pro/links. videodagi app ni topdim  nomi canopy ekan appstore da ham bor ekan. https://getcanopy.pro
6. Ha davom et men aytganimdek menga pipline ui/ux ni chizib ber qanday bo'lishi applar va jarayon
7. Ha albatta (yes — draw the screens as a clickable prototype)
8. Ha shunday qil (yes — add the Ubuntu screens too)
9. Undi claude codega beriahim uchun promt ber
10. Promt ichida men senga yozgan barcha matnni yozib qo'y to'lliq qilib

### What the video shows (frame analysis)
A MacBook and an iPhone. The user picks a photo on the iPhone and flicks it toward the right edge of the
MacBook screen; the photo flies onto the Mac desktop with an effect. The Mac settings window shows:
a "Send Zone" slider, "Effects" (Rainbow, Portal, Field Goal, Volcano) and a "Show on This Mac" toggle.
Caption: "Holy app".

### What the app in the video is (from getcanopy.pro)
- Canopy is a macOS "Dynamic Island" app for the Mac notch; "Portal" is one of its features.
- Portal sends photos, videos, files and clipboard content from an iPhone to a Mac. You pick the item in the
  free iPhone app and flick it off the screen edge; it lands where you are looking on the Mac. No cables, no AirDrop menu.
- Mac only (macOS 26+). Windows and Linux are not mentioned. Transport (Wi-Fi/Bluetooth) is not disclosed. Closed source.
- Price: €8.99 one-time, lifetime license for up to 3 Macs, 48-hour free trial.
- Effects named on the site: Rainbow, Paper Plane, Volcano, "and more".

My goal: the same experience for **Windows 10/11 and Ubuntu**, with an Android (then iOS) phone.
The animation must feel as smooth as in the video, and files must transfer smoothly.
Do NOT copy Canopy's name, branding, or exact effects — design our own.

## Answers already settled (from my questions in message 4)
- Yes, the user installs two apps: the desktop app (runs in the tray/background) and the phone app. Pairing is one-time via QR.
- Same Wi-Fi is enough. Bluetooth is NOT used (too slow, would delay the animation). Phone hotspot also works.
- Android does not allow intercepting gestures inside the system Gallery app. So there are two entry points:
  - **A. Share sheet:** Gallery / Files / Camera → Share → "Fling" → our full-screen fling screen → flick up.
  - **B. In-app:** our app's recent-photos grid (desktop already selected at the top) → tap → flick up. Multi-select allowed.
- The desktop is selected automatically when only one is paired; otherwise the last used one is remembered, switchable via a chip.

## Target platforms
- Desktop: Windows 10/11, Ubuntu 24.04 (GNOME 46) and Ubuntu 26.04 (GNOME 50, Wayland-only), KDE Plasma (Wayland)
- Phone: Android first, iOS later
- Both devices on the same Wi-Fi. No Bluetooth, no cloud.

## Architecture (already decided — follow it, push back only with evidence)
1. **Shared Rust core** (`crates/core`): protocol, pairing, transfer, built on **iroh 1.0** (QUIC, TLS 1.3,
   mDNS local discovery). Peers are identified by iroh public key. Pairing is a one-time QR code carrying
   the desktop's endpoint ID + address hints.
2. **Desktop daemon** (`apps/daemon`, Rust): iroh endpoint, transfer manager (writes files to `Downloads/Fling`),
   drives the overlay, tray icon, local IPC for the settings UI, autostart (Windows Registry Run / systemd user service).
3. **Overlay renderer** (`crates/overlay`): `winit` + `wgpu` (DX12 on Windows, Vulkan on Linux), WGSL shaders +
   GPU particles. A transparent, borderless, always-on-top, click-through, non-activating full-screen window.
   It stays hidden but pre-created ("warm"), so the first frame shows in < 16 ms. The GPU is idle when nothing animates.
4. **Host backends** (`crates/host-win`, `crates/host-wlr`, `crates/host-gnome`): the only platform-specific code.
   - Windows: `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`, `HWND_TOPMOST`,
     per-pixel alpha (premultiplied; DirectComposition fallback if the adapter only offers Opaque).
   - KDE/wlroots/COSMIC: `zwlr_layer_shell_v1` via `smithay-client-toolkit`, layer=overlay, anchored to all edges,
     keyboard_interactivity=none, empty `wl_surface.set_input_region`.
   - GNOME: GNOME has no reliable layer-shell. The renderer opens a normal xdg-toplevel with a fixed `app_id`
     and an empty input region. A small **GNOME Shell extension** (`gnome-extension/`, GJS) finds that window by
     `app_id` and makes it above, sticky, full-monitor, hidden from Alt+Tab/taskbar, and never focused.
     The daemon talks to the extension over D-Bus. Plan B, if focus stealing or flicker can't be fixed:
     render the effect inside the extension with Clutter + GLSL.
5. **Mobile app** (`apps/mobile`): Flutter + Rust core via `flutter_rust_bridge` v2 (or `iroh_flutter`).
   - Entry A (Android): system Share sheet target "Fling" → fling screen.
   - Entry B: in-app recent-photos grid, tap → fling screen.
   - Fling screen: full-screen preview, drag up; release with upward velocity over a threshold = send, otherwise
     spring back. Haptic on send.
   - Android: NsdManager/mDNS, MulticastLock, MediaStore. iOS later: NSLocalNetworkUsageDescription,
     NSBonjourServices, PhotoKit, HEIC→JPEG option.
6. **Settings UI** (`apps/settings`): Flutter desktop, talks to the daemon over local IPC.
   Settings: effect, Send Zone (left/right/top edge), duration, save folder, autostart.

## Protocol (design it in `crates/core`, document it in `docs/protocol.md`)
- ALPN `fling/1`. Per fling:
  - Stream 1: `FlingStart { id, file_name, mime, size, blake3, velocity, exit_x, angle, effect_id }`
    + a ~30 KB preview (WebP/JPEG, longest side ≤ 512 px).
  - Stream 2: the full file, streamed in parallel, verified with BLAKE3 on completion.
- The desktop starts the animation as soon as the preview arrives; it never waits for the full file.
  When the full file lands, swap preview → full-res texture.
- Latency budget (finger release → first effect frame on the desktop): < 100 ms on LAN.
- Fallbacks: QR with direct IP:port when multicast/mDNS is blocked; a clear error when the device is not reachable.

## Repo layout
```
crates/{core,overlay,host-win,host-wlr,host-gnome}
apps/{daemon,mobile,settings}
gnome-extension/
docs/{architecture.md,protocol.md,decisions/}   # ADRs for every non-obvious choice
```
Cargo workspace, `rust-toolchain.toml`, CI (GitHub Actions) building Windows + Ubuntu + Android, clippy + fmt + tests.

## How to work
- Work in phases. At the end of each phase: stop, summarize what works, what doesn't, the measured numbers,
  and wait for my OK before the next phase.
- Write an ADR in `docs/decisions/` for any deviation from this architecture.
- Measure, don't guess: add a latency probe (timestamps on both sides) and an FPS counter in debug builds.
- Keep platform code behind traits, so the core and renderer stay testable without a GPU or network.

## Phase 0 — spikes (do this first, ~1 week of scope)
1. Overlay spike: a transparent, click-through, always-on-top wgpu window that animates a textured quad flying in
   from the right edge, on (a) Windows 11, (b) KDE Wayland via layer-shell, (c) GNOME 50 via the helper extension.
   Report: alpha mode per GPU vendor, whether focus is ever stolen, any flicker on show/hide.
2. Network spike: iroh endpoint on desktop + an Android test app (Flutter + FRB): discover via mDNS, pair via QR,
   send a 30 KB preview, measure round-trip and one-way latency.
Exit criteria: < 100 ms from release to first frame, ≥ 60 FPS during the animation, no focus loss on any target.

## Phase 1 — MVP: Android → Windows
QR pairing, in-app grid + fling screen + Share target, preview + full file, one effect ("Halqa": a ring that opens
at the Send Zone edge, the photo flies in with a short light trail), save to `Downloads\Fling`, Windows toast with
"Ochish" / "Papkada ko'rsatish", tray menu (Telefonni ulash, Sozlamalar, To'xtatib turish, Chiqish),
history screen on the phone.

## Phase 2 — Ubuntu (GNOME + KDE)
GNOME extension + D-Bus, layer-shell backend, systemd user service, .deb packaging, the first-run screen
that asks the user to enable the GNOME extension once. Test on GNOME 46 and 50.

## Phase 3 — iOS. Phase 4 — more effects (Kometa, Pufak, Oddiy), video, multi-monitor, clipboard.
## Phase 5 — packaging: MSIX + winget, .deb/PPA + extensions.gnome.org, Play Store, App Store.

## UI reference (screens already designed)
- Android: A1 onboarding + permissions (Photos, Nearby Wi-Fi devices, Notifications) → A2 QR scanner
  (with "connect by IP" fallback) → A3 home grid (desktop chip on top, tabs: Rasmlar/Videolar/Fayllar/Clipboard,
  bottom nav: Tanlash/Tarix/Qurilmalar) → A4 Share sheet with "Fling" → A5 fling screen (drag up to send,
  "Yuborildi" state) → A6 history (done / progress %, resend).
- Windows: W1 "Telefonni ulash" QR window + tray icon → W2 transparent overlay, ring at the right edge, photo flies in
  over other windows → W3 toast "Pixel 8 dan rasm keldi" with Ochish / Papkada ko'rsatish → W4 settings
  (effect cards, Send Zone segmented control, duration slider, save folder, autostart).
- Ubuntu (GNOME): U1 first-run checklist (installed ✓, "GNOME kengaytmasini yoqish" button, background service ✓)
  → U2 QR window + top-bar indicator menu → U3 overlay over a terminal window → U4 GNOME notification at top center.
Style: neutral and clean; Inter for body, Space Grotesk for headings, accent #2457D6, touch targets ≥ 44 px,
all UI copy in Uzbek (Latin) with i18n keys from day one.

## Research references
- Ubuntu 26.04 = GNOME 50, Wayland-only: https://linuxiac.com/ubuntu-26-04-lts-resolute-raccoon-released/
- wlr-layer-shell: https://wayland.app/protocols/wlr-layer-shell-unstable-v1
- gtk4-layer-shell (supported compositors; GNOME not supported): https://github.com/wmww/gtk4-layer-shell
- GNOME overlay needs a shell extension: https://discourse.gnome.org/t/display-a-transparent-animation-on-top-of-gnome/8222
- wanderlay (Rust click-through overlay for GNOME Wayland): https://github.com/tilacog/wanderlay
- winit + wgpu transparent example: https://docs.rs/crate/wry/latest/source/examples/wgpu.rs
- wgpu transparent alpha-mode issue: https://github.com/gfx-rs/wgpu/issues/5661
- Zed per-pixel GPU transparency on Windows: https://git.secluded.site/zed/commit/94b75f3ad95eadc29b76218696fbd054642ace45
- Vello: https://docs.rs/crate/vello/0.12.0 · Rive Rust runtime: https://github.com/rive-app/rive-bevy
- iroh FAQ: https://docs.iroh.computer/about/faq · iroh_flutter: https://pub.dev/documentation/iroh_flutter/1.0.1/
- LocalSend protocol (possible later compatibility): https://github.com/localsend/protocol
- flutter_rust_bridge: https://lib.rs/crates/flutter_rust_bridge
- iOS Local Network + multicast entitlement: https://developer.apple.com/forums/thread/683798
- Canopy (reference product): https://getcanopy.pro

Start with Phase 0. First, show me your plan for the spikes and the exact crates and versions you'll use.
