# M1 spike: which toolkit draws the desktop windows

ADR-0009 left the desktop UI toolkit provisional. This spike builds **the same settings screen and the same looping
Halqa preview** in three toolkits and measures them, with LocalSend (a shipping Flutter desktop app) as the
reference. The decision and the numbers are in [ADR-0030](../../../docs/decisions/0030-desktop-ui-toolkit.md).

| Folder | Toolkit | Renderer |
|---|---|---|
| `slint/` | Slint 1.19 | femtovg (OpenGL); `SLINT_BACKEND=winit-software` switches to the software renderer |
| `tauri/` | Tauri 2.12 | WebView2, Canvas 2D |
| `flutter/` | Flutter 3.50 desktop | Skia via ANGLE |

All three are kept out of the Cargo workspace (their own `[workspace]`), so none of their dependencies reach
`Cargo.lock` or CI.

## Measurement contract

Every spike reads two environment variables:

- `VUTRIC_M1_LOG=<file>`: JSON lines are appended to it:
  - `{"event":"first_frame","unix_ms":…}` once, when the first frame was presented;
  - `{"event":"frames","count":…,"fps":…,"p50_ms":…,"p99_ms":…,"max_ms":…}` once, after `VUTRIC_M1_FRAMES`
    presented frames (default 600). The first second is skipped.
- `VUTRIC_M1_FRAMES`: how many frame intervals to collect.

`measure.ps1` launches each app N times and records:

- time to the first visible window and to the first frame;
- private bytes and working set of the whole process tree (WebView2 children included) 3 s after opening and after
  the frame run;
- CPU time of the tree during the frame run, as a percentage of one core;
- time for the whole tree to exit after `WM_CLOSE`;
- one screenshot per app.

It must run in the interactive session. Claude's shell runs where no window can be shown, so it is started through
the task scheduler:

```powershell
schtasks /Create /TN vutric-m1 /TR "powershell -NoProfile -ExecutionPolicy Bypass -File C:\vutric-w2\measure.ps1 -Bin C:\vutric-w2\bin -Out C:\vutric-w2\m1.json -Runs 5" /SC ONCE /ST 23:59 /IT /F
schtasks /Run /TN vutric-m1
```

`-Bin` holds `vutric-m1-slint.exe`, `vutric-m1-tauri.exe`, `flutter\` (the Flutter release folder) and
`localsend\` (the portable LocalSend 1.18.2 zip).

## Build

```powershell
cd slint;   cargo build --release
cd tauri;   cargo build --release
cd flutter; flutter build windows --release
```
