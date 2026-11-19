# ADR-0032: Instant cards are drawn on the CPU into their own no-activate window

- Status: accepted, implemented (`crates/vutric-overlay/src/card.rs`, `crates/vutric-host-win/src/card.rs`,
  `crates/vutric-daemon/src/cards.rs`). Measured on Windows 10; Windows 11 and mixed-DPI monitors are open (D8).
- Date: 2026-11-18
- Builds on: [ADR-0009](0009-native-modules-kotlin-swift.md) §1 and Consequences (cards in the overlay, clickable
  without focus, ≤ 16 ms), [ADR-0031](0031-desktop-daemon.md) (no GPU at rest).
- Changes: ADR-0009 said the cards are drawn **in the wgpu overlay**. They are now drawn by the daemon on the CPU
  into a separate small window. The Flutter-window fallback ADR-0009 kept for cards is dropped.
- Backlog: D2, and the pairing card that D3 builds on.

## Context

Four cards appear without opening any app window:

| Card | Shown when | Buttons |
|---|---|---|
| Qabul qilasizmi? | an untrusted device offers files (declined after 60 s unanswered) | Rad etish, Qabul qilish |
| Qabul qilinmoqda + progress ring | a transfer is running | – |
| Tushdi | files landed | – (hides after 3 s) |
| Telefonni ulash (QR) | tray "Telefonni ulash", or a second launch | Yopish |

Two facts made the overlay the wrong place for them:

1. The overlay surface covers the whole monitor and is click-through (`WS_EX_TRANSPARENT`, ADR-0004). A clickable
   card inside it means hit-testing a full-screen window and switching its styles while the user's mouse is moving.
2. ADR-0031 releases the GPU at rest. Creating it takes 416 ms on the test machine, so a card drawn by the GPU
   would either keep 120 MB committed all day or arrive 400 ms late.

## Decision

- **Raster:** `tiny-skia` (anti-aliased paths, gradients, shadow) and `ab_glyph` (text) render the card into a
  premultiplied BGRA buffer at the DPI of the monitor under the cursor. The QR is drawn from `qrcode` modules as
  crisp squares. Pure Rust, no GPU, unit-tested without a desktop.
- **Fonts:** Segoe UI from `%WINDIR%\Fonts`, loaded on the first card only. An embedded Geist subset (SIL OFL,
  `crates/vutric-overlay/fonts/`) is the fallback, so a card can always be drawn.
- **Window:** one `WS_POPUP` with `WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST`, pixels pushed
  with `UpdateLayeredWindow` (per-pixel alpha, no paint messages), shown with `SW_SHOWNOACTIVATE`, DWM transitions
  off. `WM_MOUSEACTIVATE` returns `MA_NOACTIVATE`, so a click is delivered without moving keyboard focus. It sits at
  the bottom-right of the work area of the monitor under the cursor.
- **State:** `Deck` (`crates/vutric-daemon/src/cards.rs`) is a pure state machine: which card shows, its timeout
  (an unanswered offer is declined, never left hanging), and what a button press means. A newer offer declines the
  older one.

## Measurements

Intel UHD 730, 2560×1440 at 100 % scale, Windows 10 19045, release build of `examples/cards.rs`, 200 runs per card,
interactive session. Raster = CPU drawing; show = `UpdateLayeredWindow` + `SetWindowPos`. Raw files:
[../measurements/2026-11-18-w2/](../measurements/2026-11-18-w2/) (`cards-*.json`, `card-*.png`).

| Card | Size (px) | Raster p50 / p99 | Show p50 / p99 | Total p50 / p99 |
|---|---|---|---|---|
| Qabul qilasizmi? | 400×176 | 1.82 / 2.48 ms | 0.12 / 0.85 ms | 1.95 / 4.04 ms |
| Qabul qilinmoqda | 400×124 | 1.30 / 1.63 ms | 0.09 / 1.59 ms | 1.40 / 2.91 ms |
| Tushdi | 400×124 | 1.14 / 1.50 ms | 0.09 / 4.62 ms | 1.23 / 5.75 ms |
| Telefonni ulash (QR) | 400×398 | 3.76 / 5.76 ms | 0.25 / 11.22 ms | 4.01 / 14.97 ms |

- Fonts load: 1.0 ms. Foreground window after 800 cards: unchanged (`focus_kept: true`).
- Second run (`cards-r2.json`, final build): totals p99 3.96 / 2.11 / 1.95 / 5.69 ms in the same order, focus kept.
- Inside the running daemon, the first card (window creation and fonts included) was on screen 16.7 ms and, in the
  final build, 11.3 ms after the request (`d1-idle2.json`, `d1-idle3.json` traces). Later cards reuse the window.
- Memory: the pairing card on screen adds 1.6 MB private to the daemon (ADR-0031 table).

Target ≤ 16 ms (ADR-0009): met at p99 for every card in both runs. The very first card of a session, which creates the
window, was 16.7 ms once and 11.3 ms once.

## Consequences

- The overlay stays a pure effect surface: click-through, GPU only during effects.
- Cards are static images, redrawn by the CPU only when their content changes, not at 60 fps. Progress redraws at
  most once per percent (101 times per transfer). No slide-in motion yet.
- Linux needs its own card window per compositor (layer-shell surface with an input region, or the GNOME
  extension). That is D10.

## Still unmeasured

- A real click on "Qabul qilish" while another app has focus. `examples/cards.rs` now sends one through the input
  queue and records whether the card got it and focus stayed; until that run is in this table, D2 stays `[~]`.
- 150 % and 200 % scaling, a second monitor with another DPI, Windows 11 (D8).
