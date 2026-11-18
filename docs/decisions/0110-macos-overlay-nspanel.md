# ADR-0110: macOS overlay = non-activating `NSPanel` with a wgpu Metal layer, at the status-bar level

- Status: accepted for implementation (`crates/vutric-host-mac`); measured only on CI virtual machines so far,
  real-Mac numbers are owed (checklist at the end).
- Date: 2026-11-18
- Stream: W5 (macOS). Builds on [ADR-0004](0004-windows-overlay-composition.md) (the Windows overlay),
  [ADR-0031](0031-desktop-daemon.md) (no GPU at rest, warm on connect) and the roadmap's Phase 5 line.
- Changes: the roadmap said `.screenSaver` level. This ADR picks the status-bar level and keeps `.screenSaver` as a
  measured option only (reasons below).
- Backlog: D11.

## Context

The overlay must draw anti-aliased, partly transparent pixels over everything the user looks at, let every click
fall through, never take keyboard focus, appear on every Space including another app's full-screen Space, and hold
60 fps (120 fps where the display is ProMotion). On Windows this is a layered, no-activate topmost window with a
DirectComposition swapchain (ADR-0004).

On macOS, three AppKit facts decide the shape:

1. **Activation.** Ordering a normal `NSWindow` front with `makeKeyAndOrderFront:` (what winit's `set_visible(true)`
   does) activates the app and takes focus. Only an `NSPanel` with `NSWindowStyleMaskNonactivatingPanel` can be
   shown and even clicked without activating its app. `orderFrontRegardless` shows any window without activating.
2. **Spaces and full screen.** A window is on one Space unless its collection behaviour says otherwise. Another
   app's full-screen window has its own Space; to be drawn there the window needs `fullScreenAuxiliary`, and since
   macOS 13 `canJoinAllApplications`.
3. **Panels hide on deactivate.** `NSPanel.hidesOnDeactivate` is `YES` by default. Our app is never active, so the
   panel would never be visible unless this is turned off.

## Decision

### Window

`OverlayPanel::new` (`crates/vutric-host-mac/src/imp.rs`) creates, on the main thread:

| Setting | Value | Why |
|---|---|---|
| Class, style | `NSPanel`, `Borderless \| NonactivatingPanel` | never activates the app (fact 1) |
| Frame | `NSScreen.frame` of the target screen, not `visibleFrame` | the throw lands over the menu bar and the Dock too |
| Level | `CGWindowLevelForKey(kCGStatusWindowLevelKey)` (25) by default | see "Level" |
| Collection behaviour | `canJoinAllSpaces \| stationary \| ignoresCycle \| fullScreenAuxiliary \| canJoinAllApplications` | every Space, over full-screen apps, not moved by Mission Control, not in Cmd-backtick cycling (fact 2) |
| Input | `ignoresMouseEvents = YES`, `becomesKeyOnlyIfNeeded`, `floatingPanel` | clicks fall through; never key |
| Transparency | `opaque = NO`, `backgroundColor = clearColor`, `hasShadow = NO` | only our pixels show |
| Visibility | `hidesOnDeactivate = NO`, `animationBehavior = None`, `excludedFromWindowsMenu` | visible while we are inactive (fact 3); no fade on show |
| Show | `orderFrontRegardless` | no activation |
| Lifetime | `releasedWhenClosed = NO`, owned by Rust `Retained` | no double release |

The panel stays ordered in and transparent between effects, as on Windows (no show/hide, so no flicker); ADR-0031
decides when it exists at all (created on phone connect, dropped 30 s after the last effect).

`make_overlay(ns_view, level)` and `show_without_activating(ns_view)` apply the same settings to a window someone
else created, such as winit's `NSWindow` in the daemon. They mirror `vutric_host_win::make_overlay` and
`show_without_activating`, so the daemon's `gpu.rs` needs one `#[cfg(target_os = "macos")]` line next to the
Windows one. That path cannot set the non-activating panel bit (an `NSWindow` is not an `NSPanel`), so it relies on
`ignoresMouseEvents` and `orderFrontRegardless` alone. The probe measures both paths (`--window panel|winit`); the
panel is the default until a real Mac shows the winit path never activates either.

The process is a background agent: `LSUIElement` in the app bundle (ADR-0111), `ActivationPolicy::Accessory` in the
probe. An accessory app owns no menu bar and is never made frontmost by showing a window.

### Pixels

wgpu 30, Metal backend. The surface is created from the panel's content view (`RawWindowHandle::AppKit`), and
wgpu (`raw-window-metal`) adds a `CAMetalLayer` that follows the view's bounds and backing scale. Metal offers
`Opaque` and `PostMultiplied` composite alpha (wgpu-hal `metal/adapter.rs`); `PostMultiplied` sets the layer
non-opaque. Core Animation composites the layer as premultiplied, which is what the Halqa shader writes, so the
same `vutric_overlay::Renderer` is used unchanged. `PresentMode::Fifo` turns on `displaySyncEnabled`, and
`desired_maximum_frame_latency = 2` gives three drawables, the ADR-0004 setting.

### Level

| Level (key) | Value | Over menu bar and Dock | Over another app's full screen | Over open menus | Over drags, screen saver |
|---|---|---|---|---|---|
| floating | 3 | no | yes | no | no |
| **status (default)** | 25 | yes | yes | no | no |
| popup-menu | 101 | yes | yes | yes | no |
| screen-saver | 1000 | yes | yes | yes | yes |

Status is the lowest level that keeps the throw visible from the screen edge over the menu bar and the Dock. Going
higher covers a menu the user has open or a file they are dragging, and at the screen-saver level it would draw over
the screen saver itself. The levels are resolved with `CGWindowLevelForKey`, never hard-coded; a macOS test checks
the documented values. The choice is a parameter (`OverlayLevel`), so the real-Mac run can compare all four.

### ProMotion

The timeline is a function of time (`Halqa::sample(t)`), not of frame count, so any refresh rate plays the same
motion. With `Fifo`, Metal presents at the display's own rate: 120 Hz on a ProMotion panel when frames keep coming,
and macOS may lower it (Low Power Mode, thermal). The probe therefore judges every run against the screen's
`maximumFramesPerSecond`: fps must be within 2 % of it, and a frame interval longer than 1.5 refreshes is a hitch
whose missed refreshes are counted (`crates/vutric-host-mac/src/pacing.rs`). A 60 fps run on a 120 Hz screen fails
by that rule, on purpose. If a real ProMotion Mac shows the system holding us at 60 Hz, the follow-up is a
`CADisplayLink` from `NSView.displayLink(target:selector:)` (macOS 14+) with `preferredFrameRateRange` 80–120, which
is how AppKit lets an app ask for the high rate.

### Measurement

`vutric-mac-probe` is the macOS twin of the Phase 0.1 spike. It plays Halqa 5 times, and after every frame it checks:

- the frontmost app is still the one from before the run (TextEdit in CI);
- `NSApp.isActive` is false;
- the panel is not the key window.

It writes fps, p50/p95/p99/max frame time, hitches, missed refreshes, first-frame latency, the resolved window
settings, the screen list with scale and max fps, and the adapter and alpha mode, as JSON. A watchdog thread writes
an error JSON if the process hangs. `tools/measure.sh` runs it three ways (panel/status, winit/status,
panel/screen-saver), takes screenshots while the effect plays, and puts a table into the job summary. The CI job
`.github/workflows/macos-desktop.yml` runs this on `macos-15-intel` and, optionally, on `macos-15` (Apple silicon).

## CI results

GitHub's macOS runners are virtual machines with a virtual display, so their numbers show that the code works on a
real WindowServer, not how a real Mac performs. Run 44325731135 (PR #25, 2026-11-18), 5 effects per row, raw JSON,
machine notes and screenshots in [`../measurements/2026-11-18-w5/`](../measurements/2026-11-18-w5/).

| Runner | Display (as reported) | GPU | Mode, level | fps (5 runs) | p99 ms | max ms | missed refreshes | first frame ms | our app ever frontmost / active / key |
|---|---|---|---|---|---|---|---|---|---|
| `macos-15` (Apple M1, virtual), macOS 15.8.2 | 1024×768 @ 60 Hz, 1x | Apple Paravirtual device | panel, status (25) | 59.2–60.5 | 19.3–34.2 | 19.9–72.9 | 0–7 | 0.56–3.98 | never |
| | | | winit, status (25) | 59.5–60.5 | 19.2–21.7 | 19.9–60.6 | 0–3 | 0.27–1.94 | never |
| | | | panel, screen-saver (1000) | 59.6–60.5 | 18.7–21.9 | 20.5–46.7 | 0–2 | 0.31–1.26 | never |
| `macos-15-intel` (i7-8700B, virtual), macOS 15.7.9 | 1920×1080 @ 30 Hz, 1x | Apple Paravirtual device | panel, status (25) | 26.1–30.5 | 35.5–39.3 | 36.1–480.1 | 0–13 | 0.62–7.38 | never ¹ |
| | | | winit, status (25) | 30.3–31.3 | 36.1–40.1 | 36.2–59.1 | 0–1 | 0.77–4.79 | never |
| | | | panel, screen-saver (1000) | 30.4–31.4 | 35.9–40.0 | 35.9–41.7 | 0 | 0.59–5.00 | never |

What this shows:

- **It works on a real WindowServer.** Metal adapter found, `PostMultiplied` alpha, the panel reads back as
  `NSPanel`, style mask `0x80` (non-activating), collection behaviour `0x40151`, `ignoresMouseEvents`, non-opaque, and
  the requested level (25 or 1000, from `CGWindowLevelForKey`). The screenshots show the card and the ring drawn over
  the desktop with TextEdit visible next to them and still frontmost in the menu bar: per-pixel transparency works.
- **Focus:** in 30 runs `NSApp.isActive` stayed false at every frame (so our process was never frontmost) and the
  panel (or winit window) was never key. ¹ On the Intel runner the first row started with the system's `UserNotificationCenter` prompt frontmost
  instead of TextEdit, so the old check (frontmost unchanged) read false although nothing of ours took focus; the
  probe now reports "frontmost is us" (the failure) separately from "frontmost changed" (information), as the Windows
  spike does, and `measure.sh` gives focus back to TextEdit before each run.
- **Pacing follows the display.** The probe holds the virtual display's own rate (60 Hz on Apple silicon, 30 Hz on
  Intel) with p50 within 0.04 ms (Apple silicon) and 0.7 ms (Intel) of the refresh period. The 480 ms gap is the first run's cold start on Intel; the
  other stalls are single frames, which these shared VMs show on Windows runners too. No real-Mac claim is made from
  them.
- **First frame** from a warm panel: 0.27–7.38 ms, inside the 16 ms target.
- winit's `NSWindow` passed the same focus checks as the panel on both VMs, so the daemon may keep its winit window
  for now (`make_overlay`); the panel stays the default until a real Mac confirms both.

## Consequences

- The macOS host is ~250 lines of safe Rust over `objc2` 0.6 / `objc2-app-kit` 0.3 (MIT, and Zlib OR Apache-2.0 OR
  MIT), with no Objective-C files and no build script.
- The daemon can keep its winit window (`make_overlay`) or switch to `OverlayPanel` (create the wgpu surface with
  `create_surface_unsafe` from `raw_handles()`); the second needs no winit window at all. Which one ships is decided
  by the real-Mac focus check; both are in the probe.
- Screen recording and screenshots include the overlay (sharing type left at the default). If the owner wants the
  effect hidden from screen sharing, that is one line (`sharingType = .none`) and a product decision.
- Multi-monitor: one panel per screen, as on Windows (D8 decides how the target screen is chosen).

## Real-Mac checklist (owed, `[~]` until done)

- [~] Intel Mac, macOS 15: probe passes in panel mode (≥ 59 fps at 60 Hz, first frame < 16 ms warm, focus kept)
  while typing in TextEdit.
- [~] Apple silicon Mac, macOS 15: the same; on a ProMotion MacBook Pro also ≥ 117.6 fps at 120 Hz, or the
  `CADisplayLink` follow-up.
- [~] macOS 26: the same on either machine.
- [~] Over another app's full-screen Space (Safari full screen, Keynote presenting): the effect shows.
- [~] Every Space: switch Spaces during the effect; it stays put (`stationary`).
- [~] Levels: status covers the menu bar and the Dock, does not cover an open menu; screen-saver covers it.
- [~] Transparency: the desktop shows through everywhere outside the ring and card (no black or grey rectangle).
- [~] winit mode: the same focus checks pass, or it is dropped in favour of the panel.
- [~] Two screens with different scale (Retina + 1x external): the panel matches each screen's pixels.
