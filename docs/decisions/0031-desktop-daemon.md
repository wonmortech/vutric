# ADR-0031: The desktop daemon holds no GPU at rest

- Status: accepted, implemented in `crates/vutric-daemon` (Windows measured; Linux builds without a tray until D10).
- Date: 2026-11-18
- Builds on: [ADR-0004](0004-windows-overlay-composition.md) (overlay window), [ADR-0007](0007-every-route-and-storage-settings.md) part 4 (storage settings), [ADR-0009](0009-native-modules-kotlin-swift.md) §4 (always-on daemon, ≤ 30 MB idle).
- Backlog: D1, and the GPU part of D9.

## Context

ADR-0009 §4 puts one Rust process on every desktop, started with the session and never closed: network,
transfers, tray, toasts, the IPC server for the desktop windows, and the overlay. Its idle budget is 30 MB.

The first build created the wgpu device and the overlay surface at startup, as ADR-0004 assumed ("pre-created and
kept warm"). Measured, that one decision cost about 120 MB of committed memory for the whole session, on a
machine where nothing was being sent.

## Decision

### Process

| Part | How | File |
|---|---|---|
| Single instance | a second launch sends `show` over the IPC and exits; the running daemon brings the pairing card up | `src/bin/vutric-daemon/main.rs` |
| Tray | `tray-icon` (Windows): Telefonni ulash, Sozlamalar, To'xtatib turish / Davom ettirish, Chiqish; the glyph turns grey while paused. Left click opens the desktop window. Linux gets a StatusNotifierItem with D10 | `tray.rs`, `tray_none.rs` |
| Toasts | WinRT `ToastNotification` with the buttons Ochish and Papkada ko'rsatish, protocol activation, no COM server | `src/toast.rs` |
| IPC | JSON lines over a per-user named pipe (`\\.\pipe\vutric-<blake3(user)[..16]>`, remote clients rejected) or a Unix socket in `$XDG_RUNTIME_DIR`; protocol `VERSION = 1`; lines over 256 KiB are refused; events are pushed after `subscribe` | `src/ipc.rs`, `src/server.rs` |
| Autostart | `HKCU\…\Run\Vutric = "<exe>" --autostart`, repaired at start when the exe moved; XDG autostart `.desktop` on Linux. `--portable` never writes either | `src/autostart.rs` |
| Settings | receiving settings are W3's `vutric_core::storage::Settings` (one owner, ADR-0008 decision 12); desktop-only preferences (effect, Send Zone, travel, monitor, autostart) in `desktop.json` next to it | `src/prefs.rs` |
| Copy | Uzbek Latin by default, English when the OS locale is English, every string behind a key | `src/i18n.rs` |
| Control | `vutric-daemon ctl status` (or any JSON request) for scripts and measurements | `main.rs` |

### GPU lifecycle

1. **Nothing GPU-related is created at startup.** No wgpu instance, no adapter, no overlay window.
2. **Warm on connect.** When a phone connects (`UiEvent::Peer(true)`), the overlay is created, before its first
   fling can arrive.
3. **Cold fallback.** A fling that arrives with no warm GPU creates it on the spot. The fling waits, it is never
   dropped; if creation fails, the fling is kept and creation is retried with the next one (D9).
4. **Release.** 30 s (`GPU_LINGER`) after the last effect, when no phone is connected, the device, surface and
   window are dropped. Back-to-back flings stay instant; an idle desktop gives the memory back.

The instant cards do not use the GPU at all ([ADR-0032](0032-overlay-cards-without-focus.md)), so a "Qabul qilasizmi?"
card never wakes it.

## Measurements

Intel UHD 730, 2560×1440, Windows 10 19045, release build, `tools/measure-idle.ps1` run in the interactive session
through the task scheduler. Private bytes = committed memory only this process can use; working set = resident
pages. Raw files: [../measurements/2026-11-18-w2/](../measurements/2026-11-18-w2/) (`d1-idle*.json`).

| Build | Idle private (avg, after 60 s) | Idle working set | Idle CPU | Source |
|---|---|---|---|---|
| GPU created at startup (first build) | **128.4 MB** | 45.8 MB avg (Windows trims it to 9 MB, the commit stays) | 119.5 ms/min | `d1-idle.json`, 600 s |
| GPU created on demand (this ADR) | **6.27 MB** (max 6.83) | 11.2 MB | 49 ms/min | `d1-idle2.json`, 300 s, offline |
| GPU created on demand, final build | **6.42 MB** (max 7.91) | 16.2 MB | 83 ms/min | `d1-idle3.json`, 600 s, relay online |

Around one effect (`d1-idle2.json` / `d1-idle3.json`):

| Moment | Private | Working set |
|---|---|---|
| idle, before the effect | 6.5 / 6.7 MB | 9.4 / 11.4 MB |
| during the effect (GPU created cold) | 128.1 / 126.1 MB | 140.4 / 127.7 MB |
| 40 s after (GPU released) | 14.8 / 15.7 MB | 27.1 / 25.2 MB |
| with the pairing card on screen | 16.4 / 16.2 MB | 29.8 / 27.1 MB |

Daemon trace: cold GPU creation took **416 ms / 360 ms**; the GPU was released 33 s after creation; the first card was
on screen 16.7 ms / 11.3 ms after it was requested (window creation and font loading included). Exit after `quit`:
1.05 s, measured from the start of the `ctl` process.

D1 target (≤ 30 MB idle over 10 minutes, ADR-0009 §4): met with a wide margin at rest (6.4 MB) and still met after
one effect (15.7 MB). The 8.5 MB that stays after release is the D3D12 runtime and driver DLLs, which Windows keeps mapped.

## Consequences

- A cold GPU costs 360–416 ms on this machine, far over the 100 ms release→pixel budget. The budget therefore depends on
  step 2 (warm on connect). A phone that sends without an open connection pays the cold start once; D9 measures
  slower GPUs.
- ADR-0004's "kept warm" now means "warm while a phone is connected and 30 s after the last effect".
- The pipe name is predictable. Another local user could create it first, so the daemon would fail to start and a
  window could talk to the wrong process. The fix is a security descriptor that allows only the user's SID plus a
  check of the server process' user on connect. Tracked as a D1 follow-up; it matters on shared PCs only.

## Still unmeasured

- Idle with a phone connected (the GPU then stays warm by design: about 128 MB while connected).
- NVIDIA and AMD GPUs, Windows 11 (D8).
