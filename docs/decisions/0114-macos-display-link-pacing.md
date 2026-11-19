# ADR-0114: macOS frame pacing from a `CADisplayLink`, asking for 80–120 Hz (ProMotion)

- Status: accepted for implementation (`crates/vutric-host-mac/src/link.rs`, `vutric-mac-probe --pacing link`);
  measured only on CI virtual machines (60 Hz and 30 Hz screens). The 120 Hz claim needs a ProMotion Mac.
- Date: 2026-11-18
- Stream: W5 (macOS). Follows [ADR-0110](0110-macos-overlay-nspanel.md) §"ProMotion", which left pacing on Fifo.

## Context

ADR-0110 drew the effect from winit's `about_to_wait` with `ControlFlow::Poll` and let Fifo's drawable wait pace
it. On a ProMotion panel (MacBook Pro 14/16 since 2021, 24–120 Hz) macOS picks the rate per window from what it
sees presented. A window that only presents is not promised 120 Hz; Apple's way to ask for it is a display link
with a `preferredFrameRateRange`. On macOS 14 and later `NSView.displayLink(target:selector:)` gives a
`CADisplayLink` that follows the screen the view is on, so it also handles a window moving between a 120 Hz
built-in panel and a 60 Hz external screen.

## Options

| Option | For | Against |
|---|---|---|
| A. Keep Fifo + `Poll` (ADR-0110) | works on macOS 13; already measured | no request for 120 Hz; a busy loop between frames while the effect plays |
| B. `CVDisplayLink` | macOS 10.4+ | deprecated since macOS 15; fires on its own thread at the screen's maximum rate, no frame-rate range; the frame then has to be handed to the main thread |
| C. `NSView.displayLink` (`CADisplayLink`), range 80–120 Hz, preferred 120 | the supported API; main-thread callback; follows the view's screen; paused when idle | macOS 14+ only |

## Decision

**C, with A as the fallback on macOS 13.**

1. `vutric_host_mac::link::DisplayLink::new(mtm, ns_view, callback)` creates the link from the overlay's view,
   sets `preferredFrameRateRange = {80, 120, 120}`, adds it to the main run loop in the common modes (so it keeps
   firing while a menu is open) and leaves it **paused**. `None` before macOS 14.
2. `set_running(true)` when an effect starts, `false` when it ends: no callbacks at rest (ADR-0031 "no GPU at
   rest" extends to no wake-ups at rest). Dropping it invalidates the link.
3. Each callback marks a frame as due; the host draws one frame in winit's next `about_to_wait` and the event loop
   sleeps (`ControlFlow::Wait`) until the next callback. Present mode stays Fifo, so a frame never tears.
4. The callback also records `targetTimestamp - timestamp`, the period the system actually granted; the probe
   reports its median as `link_hz`. That number is what shows whether the 120 Hz request was honoured.
5. The pass rule is unchanged (ADR-0110): fps ≥ 0.98 × the screen's `maximumFramesPerSecond`, so on a ProMotion
   Mac a run passes only at about 118 fps or more.

## Measured

CI virtual machines only (`.github/workflows/macos-desktop.yml`, job `overlay`, `measure.sh` runs
`panel status link` and `winit status link` next to the Fifo runs). Numbers: "CI evidence" below.

## CI evidence

Run [44340716090](https://github.com/wonmortech/vutric/actions/runs/44340716090), `macos-15-intel` (macOS 15, Apple Paravirtual GPU, 1920×1080 at 30 Hz), 5 runs per row, TextEdit
frontmost throughout; focus kept and frontmost unchanged in all 25 runs, every row passed:

| Row | pacing | `link_hz` | fps (5 runs) | first frame ms |
|---|---|---|---|---|
| panel / status | fifo | – | 30.8, 30.4, 30.3, 30.4, 30.3 | 10.28, 0.74, 0.62, 0.74, 1.00 |
| panel / status | link | 30.7 | 30.0, 30.2, 29.5, 30.0, 30.3 | 3.99, 0.96, 1.23, 1.14, 0.98 |
| winit / status | fifo | – | 31.1, 30.3, 30.5, 30.4, 30.4 | 4.56, 0.50, 0.88, 0.58, 0.99 |
| winit / status | link | 30.3 | 30.3, 30.0, 30.2, 30.1, 30.2 | 3.82, 0.72, 1.07, 0.70, 0.74 |
| panel / screen-saver | fifo | – | 30.6, 30.4, 30.3, 30.4, 30.6 | 3.97, 0.49, 0.48, 0.58, 0.93 |

What this shows: the link is created, runs, pauses between effects and paces the frames; on a 30 Hz screen the
80–120 Hz request is clamped to the screen's rate, as expected. It does **not** show 120 Hz: no runner has a
ProMotion screen. The Apple silicon job (`macos-15`, optional) got no runner in that run.

## Real-Mac checklist (owner)

- [ ] MacBook Pro with ProMotion, built-in screen: `vutric-mac-probe --pacing link --runs 10` → `link_hz` ≈ 120
      and fps ≥ 118 in every run; the same with `--pacing fifo` for comparison.
- [ ] Same Mac, overlay on a 60 Hz external screen: `link_hz` ≈ 60, run passes.
- [ ] Low Power Mode on: record what `link_hz` drops to (expected 60).
- [ ] macOS 13 Mac: `--pacing link` exits 2 with "CADisplayLink needs macOS 14"; `--pacing fifo` passes.

## Consequences

- The daemon on macOS (W2 #30) should drive the effect from `DisplayLink`, not from a polling loop.
- When `CVDisplayLink` would have been the only way (macOS 13), the host keeps Fifo pacing and does not ask for a
  rate.
