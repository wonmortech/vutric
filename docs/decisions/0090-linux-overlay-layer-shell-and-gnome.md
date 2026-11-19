# ADR-0090: The Linux overlay: layer-shell where it exists, a GNOME Shell extension where it does not, and the GPU in its own process

- Status: accepted for Phase 2. Spike 0.3 is built and runs in CI under headless sway and headless GNOME Shell 46
  (with the limits in "CI limits"); nothing is verified on a real Linux desktop yet (checklist: [`../checklists/linux-desktop.md`](../checklists/linux-desktop.md)).
- Date: 2026-11-18
- Stream: W4 · Linux desktop. Backlog: D10, spike 0.3.
- Builds on: [ADR-0004](0004-windows-overlay-composition.md) (what the overlay must do),
  [ADR-0009](0009-native-modules-kotlin-swift.md) §4 (one always-on daemon), [ADR-0031](0031-desktop-daemon.md)
  (no GPU at rest, warm on connect, 30 s linger), [ADR-0032](0032-overlay-cards-without-focus.md) (cards are CPU
  rasters in their own focus-free surface).
- Changes: CLAUDE.md §4 listed the GNOME extension's "Plan B" (render inside the extension with Clutter + GLSL).
  Plan B is dropped for effects; cards on GNOME are drawn by the extension (§2).

## Context

The overlay has the same five duties on every desktop (ADR-0004): transparent per-pixel alpha, every input falls
through, never takes keyboard focus, sits above normal windows, and is warm before the first fling lands. Wayland
gives an ordinary client none of these. What exists:

| Desktop (Phase 2 targets) | Mechanism a client can use | Notes |
|---|---|---|
| KDE Plasma 6 (Kubuntu, Fedora KDE), COSMIC, sway, Hyprland, niri, wayfire | `zwlr_layer_shell_v1`: layer `overlay`, anchor all edges, `keyboard_interactivity = none`, exclusive zone -1, plus an empty `wl_surface` input region | One protocol for all of them. Plasma has supported it since 5.20, COSMIC uses it for its own panels. |
| GNOME 46 (Ubuntu 24.04 LTS), 47–48, 49–50 (Fedora 43+) | No layer-shell, by Mutter's design. A Shell extension (GJS) can retype a window, keep it above, hide it from Alt+Tab, and draw its own actors. | Mutter API moved in 49 (below). |
| X11 sessions | override-redirect + XShape would work, but GNOME 50 has no X11 session and Plasma 6 defaults to Wayland | Out of scope for Phase 2; owner question. |

GNOME's window APIs, read from Mutter's headers and the GNOME 49 porting guide:

- **46–48:** `Meta.WaylandClient.new(context, launcher)` + `spawnv(display, argv)` launches a *trusted* client.
  Only for that client: `owns_window`, `hide_from_window_list`, `make_dock`, `make_desktop`
  (`src/meta/meta-wayland-client.h`, branch `gnome-46`).
- **49+:** `Meta.WaylandClient.new_subprocess(context, launcher, argv)` replaces the constructor;
  `Meta.Window.set_type`, `Meta.Window.hide_from_window_list` and `show_in_window_list` move onto the window
  (gjs.guide, "Port Extensions to GNOME Shell 49").

So on every GNOME version we target, a window can be made a dock that is hidden from the window list **only if
the extension launched the process that owns it**.

## Decision

### 1. Backend choice

`vutric_host_wlr::session` decides, and the compositor has the last word:

| Session | Backend |
|---|---|
| Wayland, `zwlr_layer_shell_v1` advertised (any desktop name) | `LayerShell` |
| Wayland, GNOME, no layer-shell | `GnomeShell` |
| Wayland, other compositor without layer-shell | none: files are received, a notification announces them |
| X11 | none (same) |
| no display | none |

`XDG_CURRENT_DESKTOP` is only used to tell GNOME from "some other compositor without layer-shell"; the registry
global decides layer-shell. Unit-tested on every OS (`session.rs`, 5 tests).

### 2. GNOME: a small extension, and the effect window comes from a process the extension launched

Extension `vutric@wonmor.io` (`apps/gnome-extension/`, GNOME 46–50, ESM):

- Exports `io.wonmor.Vutric.Shell1` at `/io/wonmor/Vutric/Shell` on the Shell's session-bus connection
  (`org.gnome.Shell`). The XML in the extension is the contract; `vutric-host-linux::gnome` tests that the Rust
  side names the same methods and signals.
- `SpawnSurface(socket)` launches **one fixed binary**, `vutric-surface`, looked up in `/usr/libexec/vutric`,
  `/usr/lib/vutric`, `~/.local/libexec/vutric` (never a caller-supplied path), with `--backend gnome --socket
  <socket>`. The socket must be a `*.sock` file directly inside `$XDG_RUNTIME_DIR/vutric/`. 46–48 use
  `Meta.WaylandClient.new` + `spawnv`, 49+ `new_subprocess`; the code feature-detects, not version-sniffs.
- On `window-created`, a window passes only if `client.owns_window(win)`. It is then made a dock
  (`client.make_dock` / `win.set_type(DOCK)`), hidden from the window list, kept above, made sticky, and moved and
  resized over the primary monitor. Docks are not given focus on map by Mutter; the surface also sets an empty
  input region, so pointer input falls through to the windows below.
- **Cards** (`ShowCard`, `HideCard`, signal `CardClicked`) are drawn by the extension: the daemon's CPU raster
  (ADR-0032) becomes an `St.ImageContent` on an actor in the Shell's top chrome, with hit rectangles. Shell chrome
  takes clicks without moving keyboard focus away from the app, which is exactly the card's contract. No second
  Wayland window, no focus risk.
- **Panel menu.** GNOME has no StatusNotifierItem host unless Ubuntu's AppIndicator extension is on. The extension
  shows the Vutric menu itself (`SetStatus`, signal `MenuActivated`) and hides it when the calling daemon leaves the
  bus.

The extension never runs daemon logic. With it disabled, files still arrive; the daemon shows a one-time
notification ("Effektlar uchun GNOME kengaytmasini yoqing", `linux.gnome.extension.*` keys) and the first-run
checklist asks once.

Rejected:

| Option | Why not |
|---|---|
| Plan B: Clutter + GLSL inside the extension for effects | a second implementation of every effect, in a language and shader dialect we cannot unit-test with the rest; any regression would be GNOME-only |
| Retype the daemon's own winit window from the extension (no spawn) | impossible on 46–48 (the APIs need the launching `Meta.WaylandClient`), and Ubuntu 24.04 LTS is GNOME 46 |
| The extension launches the daemon itself | the daemon would die or double-start with the extension's enable/disable cycle (lock screen, updates); systemd keeps it instead (ADR-0091) |
| Fullscreen xdg toplevel without an extension | takes focus on map, shows in Alt+Tab, sits under the top bar: breaks three of the five duties |

### 3. The GPU lives in `vutric-surface`, a separate process, on every Linux desktop

`crates/vutric-host-linux/src/bin/vutric-surface/` owns the Wayland connection, the effect surface, wgpu and the
renderer. The daemon talks to it over `vutric_host_linux::proto` (u32 length + postcard over a Unix socket in
`$XDG_RUNTIME_DIR/vutric/`, 5 unit tests): `Warm`, `Play {zone, gesture, image}`, `SetImage`, `Card`, `HideCard`,
`Release`, `Quit`; replies `Ready`, `Warmed`, `FirstFrame`, `Played {stats}`, `Clicked`, `Error`. The timeline runs
next to the GPU; frames never cross the socket.

- **Layer-shell:** the daemon spawns `vutric-surface` itself. Cards go to the same process (layer surface at the
  bottom-right, `keyboard_interactivity = none`, pixels through `wl_shm` ARGB8888, which is the premultiplied BGRA
  the card rasteriser already produces).
- **GNOME:** the daemon asks the extension (`SpawnSurface`); cards go to the extension directly (§2).
- ADR-0031's lifecycle applies unchanged: warm on phone connect, cold fallback on a fling, `Release` 30 s after the
  last effect. On Linux "release" means the process drops its device, and `Quit` ends the process when no phone is
  connected, so an idle desktop gives back the GPU memory **and** the driver libraries, which Windows keeps mapped
  (the 8.5 MB residue in ADR-0031).
- The daemon itself never links wgpu or Vulkan on Linux: a driver crash takes down only the surface, which the daemon
  restarts with the next fling.
- Cost: one more process and one socket hop (sub-millisecond on a local socket; to be measured on a real machine
  against the 100 ms release→pixel budget).

### 4. Crates (licences checked: MIT, Apache-2.0 or BSD only)

| Crate | Version | Licence | Why |
|---|---|---|---|
| `smithay-client-toolkit` | 0.19.2 | MIT | layer-shell, xdg-shell, seat, shm; already in the lockfile through winit, so no second copy |
| `wayland-client` / `wayland-backend` | 0.31.15 / 0.3.17 | MIT | `client_system` + `dlopen`: libwayland is loaded at run time, so no `-dev` package at build time and one binary for every distribution |
| `raw-window-handle` | 0.6.2 | MIT/Apache-2.0/Zlib | hands wgpu the `wl_display` + `wl_surface` |
| `zbus` | 5.19.0 | MIT | StatusNotifierItem, dbusmenu, notifications, the extension's API; pure Rust, tokio backend |
| `libc` | 0.2.191 | MIT/Apache-2.0 | `poll` on the Wayland socket |

Rejected: `ksni` (Unlicense, not on the allowed list), `notify-rust` (pulls a second D-Bus stack for one method
call), `gtk`/`libappindicator` (C dependencies and a GTK main loop in the daemon). `zbus` 6.x exists; 5.19 is used
because its API is the one the code and its CI test are written against; moving to 6 is a small follow-up.

### 5. What `vutric-host-wlr` gives the daemon (the same three things `vutric-host-win` does)

| Need | Windows (`vutric-host-win`) | Linux (`vutric-host-wlr`) |
|---|---|---|
| Effect surface that never takes input or focus | `make_overlay` + `show_without_activating` on a winit window | `Host::overlay_layer` / `Host::overlay_toplevel` → `Surface` (implements the raw-window-handle traits, so `wgpu::Instance::create_surface(Arc<Surface>)` works as with winit) |
| Focus-free clickable card | `card::CardWindow::show(w, h, bgra, at)` | `Host::show_card(bgra, w, h, scale, output)`, `hide_card`, `card_scale` |
| Pointer events on the card | `card::Mouse` callback on the UI thread | the same `Mouse` enum, callback on the Wayland thread (must be `Send`) |

There is no shared `OverlayHost` trait yet; W2's daemon calls `vutric-host-win` functions directly. The trait
and the Linux branch of the daemon are W2 files, filed as a `needs-w2` issue (see the PR) with the exact API.

## Spike 0.3 measurements

Only CI numbers exist so far: GitHub's `ubuntu-24.04` runner, headless sway (wlroots, pixman renderer) and headless
GNOME Shell 46.0 (Mutter 46.2, virtual monitor), Mesa llvmpipe (software Vulkan). They prove the protocol paths but
say nothing about real GPU frame pacing. Numbers: [`../reports/phase-0.3-linux-overlay.md`](../reports/phase-0.3-linux-overlay.md)
and the `linux-overlay-sway` / `linux-overlay-gnome` workflow artifacts.

### CI limits (what CI cannot verify)

| Path | Verified in CI | Not verifiable in CI |
|---|---|---|
| layer-shell (sway headless) | layer surface configured, transparent wgpu surface, 5 + 3 effects at ~59 fps with 0 hitches, 50 cards, the daemon protocol | focus, click-through, transparency as seen, real GPUs |
| GNOME (Shell 46 headless) | extension loads; `Ping`, `ShowCard`, `HideCard`, `SetStatus`; the surface plays effects as a plain Mutter client; the extension launches it as a trusted client, `Ready` + `Warmed` at the full 1920×1080 monitor; the window is adopted (`make_dock` + `hide_from_window_list` on 46) | **effects after the dock retype**: headless Mutter (surfaceless renderer, no GPU) sends the dock window no frame callbacks, so the first `Play` times out after 30 s. Whether a real GNOME 46/50 session paces a dock window normally is checklist rows 1, 5 and 9; if it does not, the fallback is to keep the window `NORMAL` + above + skip-taskbar and only hide it from the window list. GNOME 47–50 APIs (`new_subprocess`, `Window.set_type`) are not on the runner at all |

The GNOME job gates on the verified column and records the post-retype play count without gating on it.

## Consequences

- Two code paths on Linux instead of one, but both share the renderer, the timeline and the surface process; only
  surface creation and card display differ.
- The extension must pass extensions.gnome.org review (spawning a binary is allowed when it is part of the
  extension's stated purpose and the path is fixed; reviewed per upload). Checklist in
  [`../../packaging/linux/README.md`](../../packaging/linux/README.md).
- Fractional scaling: the surface uses the integer `wl_output` scale. On a 125 % Plasma output it renders at 2× and
  the compositor scales down. `wp_fractional_scale_v1` + `wp_viewporter` is a follow-up once measured on a device.
- Fullscreen apps: the layer-shell overlay sits above them (layer `overlay`); the GNOME dock does not cover a
  fullscreen window. Whether the effect should yield to fullscreen games and video is the open design question
  from the Phase 0.1 report.

## Still unverified (real machines)

Everything in the checklist: focus never leaves the app on Plasma 6, GNOME 46 and GNOME 50; pointer clicks fall
through the effect surface; the card takes a click without focus on both; `make_dock` really suppresses focus on
map in Mutter 46; St.ImageContent `set_bytes` signature on 46 vs 47+; frame pacing on Intel, AMD and NVIDIA.
