# Phase 0.3 report: overlay spike on Linux (W4)

Date: 2026-11-18 · Binaries: `vutric-surface` and `examples/surface_driver` (release, `crates/vutric-host-linux`)
· Where: GitHub Actions `ubuntu-24.04` runner, workflow `linux-desktop.yml`, **headless compositors with software
rendering**. No real Linux machine has run it yet.

**Verdict: the path works end to end on wlroots (layer-shell); the GNOME path is not proven yet (see below).
Frame numbers below are from Mesa llvmpipe on a CI VM and say nothing about real GPUs.**

## Layer-shell on headless sway (KDE / wlroots / COSMIC path)

Run 44328329184, commit 4ab8bf5. sway 1.9 (`WLR_BACKENDS=headless`, `WLR_RENDERER=pixman`, one 1920×1080 @ 60 Hz
output), Vulkan 1.4.318 on llvmpipe (LLVM 20.1.2), kernel 7.0.0-1014-aws.

`vutric-surface --self-test 5 --cards 50`: layer surface on the overlay layer, GPU warm 76.8 ms.

| run | frames | fps | first (ms) | p50 (ms) | p95 (ms) | p99 (ms) | max (ms) | hitches |
|---|---|---|---|---|---|---|---|---|
| 1 | 166 | 59.1 | 140.82 | 17.00 | 17.18 | 17.24 | 17.28 | 0 |
| 2 | 174 | 59.2 | 2.21 | 17.00 | 17.18 | 17.24 | 17.29 | 0 |
| 3 | 174 | 59.2 | 1.84 | 16.98 | 17.18 | 17.27 | 17.35 | 0 |
| 4 | 174 | 59.1 | 1.92 | 16.99 | 17.21 | 17.29 | 17.33 | 0 |
| 5 | 174 | 59.2 | 1.93 | 16.99 | 17.22 | 17.34 | 17.43 | 0 |

Cards: 50 shown as a second layer surface (`wl_shm`), show p50 0.09 ms, p99 0.61 ms.

Through the daemon protocol (`surface_driver --spawn`, socket + postcard, as the daemon will use it): `Ready`
(layer-shell, scale 1), `Warmed` 61.2 ms, three effects first frame 13.5 / 16.0 / 15.9 ms after `Play`, 58.6–58.9 fps,
p99 17.3–17.4 ms, 0 hitches.

Reading: the run-1 first frame (140.8 ms) is shader/pipeline creation on llvmpipe; later runs are < 2.3 ms in the
self-test. The 17 ms p50 is sway's headless 60 Hz frame callback, not our render time. These prove the surface is
created, configured, paced by frame callbacks and stays alive across effects; they do not measure a real GPU.

## GNOME Shell 46 headless (extension path)

`gnome-shell --headless --wayland --virtual-monitor 1920x1080` with the extension enabled: the extension loads and
answers `Ping` (`(uint32 1, '46.0')`), `ShowCard` round trip 2.2–4.1 ms, `SpawnSurface` returns a pid. The surface
process launched through `Meta.WaylandClient` then **exits before sending `Ready`**; diagnosis in progress (the
surface now reports Wayland-connect failures and panics over the socket, and the extension can log its stderr via
`VUTRIC_SURFACE_LOG`). Until this passes, the GNOME overlay is unproven.

## How to reproduce

```bash
cargo build --release -p vutric-host-linux --bin vutric-surface --example surface_driver
bash packaging/linux/ci/sway-headless.sh out-sway                       # needs sway, mesa-vulkan-drivers
dbus-run-session -- bash packaging/linux/ci/gnome-headless.sh out-gnome  # needs gnome-shell
```

On a real desktop session just run `vutric-surface --self-test 5 --cards 50 --out st.json`
(checklist row 1, [`../checklists/linux-desktop.md`](../checklists/linux-desktop.md)).

## Not verified

Focus never taken, click-through, transparency and Alt+Tab hiding are set in code (ADR-0090) but a headless
compositor has no input or screen to check them on. All of these, and real-GPU pacing, are checklist rows 1–9.
