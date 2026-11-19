# ADR-0118: macOS instant cards: W2's CPU-drawn cards in a non-activating, clickable `NSPanel`

- Status: accepted for implementation (`crates/vutric-host-mac/src/card.rs`, `examples/cards.rs`,
  `tools/cards.sh`); measured on CI virtual machines (Intel and Apple silicon), including a real click through
  the window server. Not yet driven by the daemon (W2 #30).
- Date: 2026-11-18
- Stream: W5 (macOS). The macOS host for [ADR-0032](0032-overlay-cards-without-focus.md) (W2, PR #17). The cards
  themselves (`vutric_overlay::card`) and their state (`vutric_daemon::cards::Deck`) are W2's and are used
  unchanged.

## Context

ADR-0032 draws the four cards ("Qabul qilasizmi?", the progress ring, "Tushdi", the pairing QR) on the CPU into
premultiplied BGRA and shows them in a small Windows window that takes clicks without taking focus
(`WS_EX_NOACTIVATE`, `MA_NOACTIVATE`). The Mac needs the same thing: a window the user can click while their own
app keeps the keyboard, and the overlay (ADR-0110) stays click-through and off the GPU at rest.

## Decision

1. **Window.** `CardPanel` is a borderless `NSPanel` with the non-activating style: on every Space, over
   full-screen apps, no animation (the overlay's collection behaviour), at the status-bar level. It is not
   click-through. Its content view returns `acceptsFirstMouse: true`, so the first click on it is a click rather
   than "activate the window first". A borderless panel cannot become key, and the non-activating style keeps
   Vutric from becoming the active app.
2. **Pixels.** The BGRA buffer from `vutric_overlay::card::render` becomes a `CGImage`
   (`kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little`, the same memory layout) set as the view
   layer's `contents` at the screen's backing scale. No Metal and no drawing code of our own; the card's
   own shadow and rounded corners come through as alpha.
3. **Place and scale.** Bottom-right of the visible frame (menu bar and Dock excluded) of the screen under the
   pointer, 16 pt in. The card is rendered at that screen's `backingScaleFactor` (2 on Retina).
4. **Clicks.** `mouseUp` reports the point in card pixels (top-left origin, backing pixels), the same space as
   `Rendered::buttons`. The daemon calls `Rendered::hit` and then `Deck`, as on Windows.
5. **Measurement.** `examples/cards.rs` is the macOS twin of W2's `crates/vutric-daemon/examples/cards.rs`:
   - the same four `Deck` cards, 200 renders and shows each;
   - the same JSON fields, plus the macOS focus checks (frontmost app unchanged, Vutric never active, panel never
     key);
   - a real left click posted through the window server (`CGEventPost` at the "Qabul qilish" button), recording
     which button the card received.

   The daemon is a dev-dependency only (the daemon will depend on this crate for its macOS cards, W2 #30).

## Measured

`.github/workflows/macos-desktop.yml` job `overlay`, run [44354941609](https://github.com/wonmortech/vutric/actions/runs/44354941609).
Conditions: TextEdit frontmost, 200 runs per card, scale 1.0 (the VMs' screens are not Retina). Raster is the
CPU drawing; show is `CGImage` + `setFrame` + `setContents` + `orderFrontRegardless`. Times in ms.

| Card | Size (px) | Apple silicon VM (macOS 15.8.2): raster / show / total, p50 · p99 | Intel VM (macOS 15.7.9): raster / show / total, p50 · p99 |
|---|---|---|---|
| Qabul qilasizmi? | 400×176 | 2.09 · 4.54 / 0.38 · 3.10 / **2.52 · 7.25** | 4.86 · 8.37 / 0.79 · 3.12 / **5.88 · 9.90** |
| Qabul qilinmoqda | 400×124 | 1.81 · 5.32 / 0.40 · 2.38 / **2.31 · 6.43** | 3.56 · 5.53 / 0.65 · 1.52 / **4.25 · 6.37** |
| Tushdi | 400×124 | 1.42 · 3.91 / 0.36 · 1.59 / **1.90 · 4.78** | 3.32 · 5.05 / 0.67 · 1.20 / **4.04 · 5.69** |
| Telefonni ulash (QR) | 400×398 | 5.07 · 12.09 / 0.42 · 1.81 / **5.68 · 15.02** | 10.81 · 15.05 / 1.21 · 2.04 / **12.02 · 16.44** |

Focus and click, on both VMs:
- Focus was kept through 800 shows and the click: TextEdit stayed frontmost, Vutric never became active, and the
  panel never became key. The screenshots show TextEdit's name in the menu bar with the card on screen.
- The posted click arrived as `Accept`. `CGPreflightPostEventAccess` was true on the runners.
- The first show took 2.43 ms (Apple silicon) and 4.88 ms (Intel). Fonts loaded in 3.5 ms and 0.2 ms.

Against the ADR-0009 target of 16 ms p99, every card is under it on Apple silicon. On the Intel VM the QR card is
at 16.44 ms (raster 15.05 ms); the three other cards meet it.

## Known gaps

- **Fonts.** `vutric_overlay::card::Fonts::system` looks for Segoe UI (Windows) and DejaVu (Linux). On macOS it
  falls back to the embedded Geist subset, which is Latin only, so a device name in another script would not
  render. The patch for W2 (add `/System/Library/Fonts/SFNS.ttf` and a CJK/Cyrillic fallback) is on #30; this ADR
  does not change W2's crate.
- **Scale 2.** Retina rendering is wired up, but the CI screens are scale 1, so it has not been measured.
- **Slide-in motion.** None, as on Windows (ADR-0032 Consequences).

## Real-Mac checklist (owner)

- [ ] Retina MacBook: cards are sharp (scale 2.0 in the JSON); QR total p99 ≤ 16 ms.
- [ ] Type in TextEdit, click "Qabul qilish" on a card: typing continues in TextEdit without clicking back.
- [ ] Card over a full-screen app and on another Space.
- [ ] Two screens: the card appears on the screen under the pointer.
