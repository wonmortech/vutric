# ADR-0030: Desktop windows are Slint with the software renderer

- Status: proposed by W2 from the M1 measurements; the owner confirms (ADR-0009 "Verification").
- Date: 2026-11-18
- Builds on: [ADR-0009](0009-native-modules-kotlin-swift.md) §4 (big windows in a separate process that exits on
  close), [ADR-0031](0031-desktop-daemon.md), [ADR-0032](0032-overlay-cards-without-focus.md).
- Changes: ADR-0009 kept Flutter desktop as the provisional toolkit and rejected Slint ("rich shader effects are
  hard"). The measurements below reverse that for the desktop windows. The phone stays Flutter.
- Backlog: M1; it sets the toolkit for D3, D4, D6, T8.

## Context

The owner's criteria (2026-11-18): the UI must give the "wow" effect, be very fast and light; a big download is fine,
but **while running it must use little RAM** and be smooth in real use; rewriting later is acceptable.

The desktop shape is fixed by ADR-0009 §4: an always-on Rust daemon (tray, toasts, IPC, transfers, the GPU overlay
that plays the effects, the instant cards) and a separate window process for settings, history, devices and the big
send window, which runs only while open and exits fully on close. The "wow" moment of the product, the throw landing
on the desktop, is drawn by the daemon's wgpu overlay, not by this window. The window must be light, fast to open,
and polished.

## The spike

`apps/desktop/m1-spike/`: the same settings screen and the same looping Halqa preview (one fling every 2.9 s) in
Slint 1.19, Tauri 2.12 (WebView2) and Flutter 3.50 desktop, plus the shipping LocalSend 1.18.2 (Flutter) as a
reference. `measure.ps1` launches each app 5 times in the interactive session and records the whole process tree.
Two screenshots 400 ms apart prove the preview is really drawn (`animates`).

**Machine:** Intel UHD 730, 2560×1440 @ 60 Hz, 100 % scaling, Windows 10 19045, release builds. Window about 976×679.

## Measurements (medians of 5 runs, [`m1-r2.json`](../measurements/2026-11-18-w2/m1-r2.json); first run [`m1-full.json`](../measurements/2026-11-18-w2/m1-full.json) agrees)

| App | Window shown | First frame | Private MB open / +10 s | Working set +10 s | Processes | fps | p99 frame | CPU, % of one core | Exit | Left after close |
|---|---|---|---|---|---|---|---|---|---|---|
| **Slint, software renderer** | **132 ms** | **141 ms** | **6.3 / 6.3** | **20.4 MB** | 1 | 60.6 | 18.1 ms | **7.6** | 98 ms | 0 |
| Slint, femtovg (OpenGL) | 258 ms | 242 ms | 353 / 417 | 460 MB | 1 | 59.9 | 21.8 ms | 20.0 | 136 ms | 0 |
| Tauri (WebView2) | 45 ms | 348 ms | 184 / 186 | 380 MB | 8 | 60.0 | 16.9 ms | 38.3 | 257 ms | 0 |
| Flutter desktop | 908 ms | 120 ms | 203 / 206 | 192 MB | 1 | 59.9 | 20.7 ms | 24.3 | 274 ms | 0 |
| LocalSend 1.18.2 (reference, idle) | 366 ms | – | 159 / 167 | 165 MB | 1 | – | – | 4.0 | 101 ms | 0 |

Notes on the numbers:

- One run of each app was disturbed by other builds on the same machine (for example Slint software run 5: 30 fps,
  p99 414 ms; Tauri run 1: first frame 3.3 s). Medians absorb them; the raw runs are in the JSON files.
- In the first measurement (`m1-full.json`) the software renderer logged no frames: Slint's rendering notifier does
  not exist there. The probe now times winit `RedrawRequested` instead (`slint/src/main.rs`), and the screenshots
  show the ring and card moving (`animates: true` in all 5 runs).
- Flutter shows its window only after the first frame, so its "first frame" comes before "window shown". Its cold
  open is the window time, 0.7–1.2 s.
- LocalSend runs no animation, so its CPU is an idle figure.

## Decision

The desktop windows (`vutric-app`: settings, history, devices, big send window, QR pairing) are written in **Slint**
with the **software renderer** and the winit backend, in Rust, in the same workspace as the daemon.

Against the owner's criteria:

| Criterion | Slint software | Best alternative |
|---|---|---|
| Little RAM while open | 6.3 MB private, 20 MB working set | LocalSend 159 MB, Tauri 184 MB × 8 processes |
| Fast to open (ADR-0009: ≤ 400 ms) | 132 ms | Tauri window 45 ms but first frame 348 ms; Flutter 0.9 s fails |
| Smooth | 60.6 fps, p99 18.1 ms | Tauri p99 16.9 ms, at 6× the CPU and 30× the memory |
| Exits fully on close | yes, 98 ms | all do |
| "Wow" | effects live in the wgpu overlay; the window gets the Claude Design tokens, spring motion and the live Halqa preview | Flutter/Tauri make richer in-window shaders easier |

Also in favour: one language with the daemon, so the IPC types (`vutric_daemon::ipc`) are shared directly, with no
bridge and no second serializer; Slint has AccessKit accessibility and IME through winit.

Rules that follow:

1. The software renderer is the default. A window that ever needs GPU shading (for example a full-window blur)
   switches only that window to Skia, after a measurement shows the cost.
2. Interface motion stays short (< 250 ms, design README). The only long animation in a window is the Halqa preview.
3. The window talks to the daemon only through its IPC (ADR-0009, workstreams "The interface between streams").

Rejected:

- **Slint femtovg (OpenGL):** 353–417 MB private on this Intel driver, 55× the software renderer, for no visible gain.
- **Tauri:** 8 processes and 184 MB private; highest CPU (38 % of a core for the same preview).
- **Flutter desktop:** cold open 0.9 s fails the 400 ms target; 203 MB private, more than LocalSend.

## Consequences

- ADR-0009 §4's RAM target for the window (≤ 120 MB open, less than LocalSend) is met by a factor of 20.
- The software renderer's p99 (18.1 ms) is above 16.7 ms, as the overlay's is (ADR-0004: 19.1–20.5 ms with latency
  2). The preview loses about one frame in a hundred; the window has no other continuous animation.
- Slint's licence must be chosen: GPLv3, the royalty-free licence (attribution in the About screen) or a commercial
  one. The repository is Apache-2.0. Asked in [owner-questions.md](../owner-questions.md); meanwhile the default is
  the royalty-free licence with the attribution.
- `apps/desktop/m1-spike/` stays as the record of the measurement and is not built by CI.

## Still unmeasured

- 150 % and 200 % scaling (2–4× the pixels for the software renderer), a 4K monitor, and the big send window with
  many thumbnails.
- Ubuntu (GNOME and KDE): software renderer through winit on Wayland and X11.
- Windows 11, NVIDIA and AMD (D8).
- Open time from the tray click (the spike measured process start → window; the tray path adds process spawn only).
