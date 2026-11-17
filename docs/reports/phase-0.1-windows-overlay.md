# Phase 0.1 report — overlay spike on Windows

Date: 2026-11-17 · Binary: `apps/overlay-spike` (release) · Machine: Intel UHD Graphics 730 (DX12, integrated),
primary monitor 2560×1440 @ 60 Hz, second monitor attached, Windows 10 Pro 19045.

## Exit criteria

| Criterion | Target | Result |
|---|---|---|
| Frame rate during the effect | ≥ 60 FPS | **60.3 FPS**, 177 frames per 2.93 s run, 5/5 runs |
| Frame pacing | no missed vblank | p50 16.6–16.8 ms, p99 19.1–20.5 ms, max 25.2 ms, 0 missed vblanks |
| First frame after trigger (warm window) | < 16 ms | 0.56–10.67 ms |
| Focus never taken | 0 | 0 in 15 runs (`GetForegroundWindow` sampled every frame) |
| Transparency | per-pixel | `PreMultiplied` via DirectComposition; desktop visible through the portal glow |
| Flicker on show/hide | none | window shown once, kept warm; no show/hide during effects |

**Verdict: PASS on this machine.** See ADR-0004 for the A/B that led to `frame_latency = 2` and split scissors.

## How to reproduce

```powershell
cargo build -p vutric-overlay-spike --release
.\target\release\vutric-overlay-spike.exe --runs 5 --interval 0.6
```

Run it from an interactive desktop session. A process in session 0 (a service, an SSH shell, some agent sandboxes)
cannot create a visible surface: `Surface::configure` fails with "Invalid surface". The report prints one table row
per run and `PASS`/`FAIL` against the criteria above; the process exits non-zero on `FAIL`.

Do not screen-record during a measurement: GDI capture (`ffmpeg -f gdigrab`) of a 2560×1440 desktop at 30 fps
dropped the same build to ~41 FPS with ~40 missed vblanks per run.

## Not covered yet

- NVIDIA / AMD discrete GPUs, hybrid laptops (which adapter DirectComposition composes on).
- Windows 11 24H2+, mixed-DPI multi-monitor, HDR desktops.
- Full-screen exclusive games and video players (the overlay should yield; to be designed).
- Spike 0.2 (network) and 0.3 (KDE / GNOME): need the Flutter/Android toolchain and a Linux machine.
