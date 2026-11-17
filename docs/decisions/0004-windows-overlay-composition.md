# ADR-0004: Windows overlay = layered click-through window + DirectComposition swapchain

- Status: accepted (measured, see `docs/reports/phase-0.1-windows-overlay.md`)
- Date: 2026-11-17

## Context

The overlay must draw anti-aliased, partly transparent pixels over every other window, let all input fall
through, and never take keyboard focus. On Windows a DXGI swapchain created from an HWND only offers an opaque
composite alpha mode (wgpu issue gfx-rs/wgpu#5661), which would black out the screen. Zed solved the same problem
with DirectComposition.

## Decision

- wgpu 30, DX12 backend, `Dx12SwapchainKind::DxgiFromVisual`: wgpu creates an `IDCompositionVisual` for the window
  and the surface reports `CompositeAlphaMode::PreMultiplied`. The shader outputs premultiplied linear colour.
- The window is created hidden by winit with `no_redirection_bitmap`, `skip_taskbar`, no decorations, then
  `vutric-host-win` adds `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST`,
  calls `SetLayeredWindowAttributes(255, LWA_ALPHA)`, disables DWM transitions and peek, and shows it with
  `SW_SHOWNOACTIVATE`. The window stays shown (transparent) between effects: no show/hide, so no flicker.
- If the adapter offers no transparent composite mode the overlay refuses to start rather than cover the desktop.
- `desired_maximum_frame_latency = 2`, `PresentMode::Fifo`. Each frame clears the surface and shades only the
  scissored regions returned by the timeline (portal, card + trail), never the whole screen.

## Measurements (Intel UHD 730, 2560×1440 @ 60 Hz, Windows 10 19045)

| Setting | fps | p99 frame | missed vblanks | focus |
|---|---|---|---|---|
| latency 2, split scissor | 60.3 | 19.1–20.5 ms | 0 | never taken |
| latency 1, split scissor | 55.2–58.6 | 30.9–32.4 ms | 5–14 per run | never taken |
| latency 1, one union scissor | 56.9–58.7 | 31.3–32.4 ms | 5–10 per run | never taken |

## Consequences

- One extra frame of queueing (≈ 16.7 ms) is accepted for smooth pacing; first-frame latency from a warm window
  stays 0.5–10.7 ms, well inside the 100 ms release→pixel budget.
- RenderDoc cannot capture `DxgiFromVisual` swapchains; debug captures use an offscreen target instead.
- Still to verify on NVIDIA and AMD discrete GPUs, on multi-monitor setups with mixed DPI, and on Windows 11.
