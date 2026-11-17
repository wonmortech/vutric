# CLAUDE.md — Vutric

Talk to the owner in **Uzbek (Latin script)**. Write code, comments, commit messages, docs and ADRs in **English**.
UI copy shipped to users is **Uzbek (Latin)** behind i18n keys from day one (English is the fallback locale).

## What Vutric is

Flick a photo, video or file off the edge of your phone and watch it fly onto your desktop screen with a
GPU effect — on **Windows and Linux first**, the platforms the closed-source reference product (Canopy "Portal",
macOS + iPhone only) ignores. Same Wi-Fi, no cables, no cloud, no Bluetooth.

The original planning brief (the owner's ten messages verbatim + the research behind them) is archived in
[`docs/research/original-brief.md`](docs/research/original-brief.md). Read it once. Newer decisions live in
[`docs/decisions/`](docs/decisions/) and override it.

Never copy Canopy's name, branding, UI or exact effects. Our effects are our own (see `docs/effects.md`).

## Platform order (owner decision, ADR-0003 — do not reorder)

| Phase | Desktop | Phone |
|---|---|---|
| 1 | Windows 10/11 | Android |
| 2 | Ubuntu (GNOME 46/50, KDE Wayland) | Android |
| 3 | Windows 10/11 | iOS |
| 4 | Linux | iOS |
| 5 | macOS (last) | Android + iOS |

Phase 0 (spikes) comes before all of them. Details: [`docs/roadmap.md`](docs/roadmap.md).

## Architecture (decided — follow it, push back only with measured evidence and an ADR)

1. **`crates/vutric-core`** — protocol, pairing payload, framing, transfer bookkeeping. Pure Rust, no GPU, no OS.
   Networking is **iroh** (QUIC + TLS 1.3, local discovery via mDNS). Peers are identified by iroh public key;
   pairing is a one-time QR that carries the desktop endpoint id + LAN address hints.
2. **`apps/daemon`** (Rust) — iroh endpoint, transfer manager (`Downloads/Vutric`), drives the overlay, tray icon,
   local IPC for the settings UI, autostart (HKCU Run / systemd user unit / LaunchAgent later).
3. **`crates/vutric-overlay`** — `winit` + `wgpu` renderer (DX12 on Windows, Vulkan on Linux, Metal on macOS later),
   WGSL shaders. A transparent, borderless, always-on-top, click-through, never-activating full-screen surface,
   pre-created and kept warm so the first effect frame lands in < 16 ms. The GPU is idle when nothing animates.
   Effect math (timelines, easing, springs, trajectories) is pure and unit-tested without a GPU.
4. **Host backends** — the only platform-specific code, behind the `OverlayHost` trait:
   - `crates/vutric-host-win`: `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE |
     WS_EX_TOPMOST`, `SW_SHOWNOACTIVATE`, premultiplied per-pixel alpha (DirectComposition when the swapchain
     only offers opaque).
   - `crates/vutric-host-wlr` (phase 2): `zwlr_layer_shell_v1` via `smithay-client-toolkit`, layer=overlay,
     anchored to all edges, keyboard_interactivity=none, empty input region.
   - `gnome-extension/` (phase 2): GNOME has no layer-shell; a GJS Shell extension finds our window by `app_id`
     and makes it above/sticky/unfocusable/hidden from Alt+Tab. D-Bus to the daemon. Plan B: render inside the
     extension with Clutter + GLSL.
5. **`apps/mobile`** — Flutter + the Rust core through `flutter_rust_bridge` v2. Android first.
   Entry A: system Share sheet target "Vutric". Entry B: in-app recent grid. Fling screen: drag up, release above a
   velocity threshold = send, otherwise spring back; haptic on send.
6. **`apps/settings`** — Flutter desktop, talks to the daemon over local IPC.

Protocol: [`docs/protocol.md`](docs/protocol.md). The desktop starts the effect when the ~30 KB preview arrives and
swaps in the full-resolution texture when the BLAKE3-verified file lands. It never waits for the full file.

**Latency budget:** finger release → first effect frame on the desktop < **100 ms** on LAN. ≥ 60 FPS during effects.
No focus stealing, ever.

## Repo layout

```
crates/vutric-core       protocol, pairing, framing (no I/O)
crates/vutric-overlay    effect math + wgpu renderer
crates/vutric-host-win   Win32 window styling for the overlay
apps/overlay-spike       Phase 0 spike binary (overlay on the real desktop)
apps/daemon              (phase 1)
apps/mobile              (phase 1, Flutter)
apps/settings            (phase 1, Flutter desktop)
gnome-extension/         (phase 2)
docs/                    architecture, protocol, roadmap, effects, design, decisions/ (ADRs)
```

## How to work

- Work in phases. At the end of each phase stop and report: what works, what does not, the **measured** numbers.
  Wait for the owner's OK before the next phase.
- Any deviation from this file → an ADR in `docs/decisions/` (`NNNN-kebab-title.md`, status/context/decision/consequences).
- Measure, don't guess: latency probe (timestamps on both ends), FPS + frame-time counter in debug builds.
- Keep platform code behind traits so core and renderer stay testable without a GPU or network.
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` must pass
  before every commit. CI runs the same on Windows and Ubuntu.
- No code comments that narrate the obvious; doc comments on public items only where they add information.
- Commits: DCO sign-off (`git commit -s`), no AI co-author trailers, English, imperative mood.

## GitHub (wonmortech org)

Public repo `wonmortech/vutric`. Squash merges only. Follow the org conventions used by `muhr`, `sanad`, `shaffof`
(CODEOWNERS, SECURITY.md, CONTRIBUTING.md, DCO, Dependabot, OpenSSF Scorecard).
