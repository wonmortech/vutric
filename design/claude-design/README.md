# Handoff: Vutric — flick a photo from phone to desktop

## Overview
Vutric is a local‑first app that sends a photo (or video, file, clipboard) from a phone to a desktop by **flicking it off the screen edge**. The item flies through a glowing "portal" and lands on the computer, right where the user is looking. No cloud, no account — everything travels inside one Wi‑Fi, pairing by QR.

This bundle covers two deliverables:
1. **The app** (`Vutric App v4.dc.html`) — iOS/Android phone UI + a macOS catch scene, with 9 selectable themes, and a working flick prototype.
2. **The marketing / design‑system site** (`Vutric Site.dc.html`) — a Canopy‑style landing page with an animated logo and 5 themes.

## About the Design Files
The files in this bundle are **design references authored in HTML** (they open in a browser; they are "Design Components" that stream a React‑like template). They are **not production code to copy directly**. The task is to **recreate these designs in the target codebase's environment** using its established patterns:
- The app is specced for **Flutter / Material 3** (primary device iPhone 16 Pro, 393×852 pt; Android too). Recreate the screens as Flutter widgets.
- The site should be recreated in the repo's stack (this project's source repo is **wonmortech/wonmor.io**, a Vite/React + TypeScript site — build it as React components there).

If recreating in React/SwiftUI/Flutter, reproduce the look pixel‑for‑pixel but use the platform's native navigation, lists, haptics, and animation primitives.

## Fidelity
**High‑fidelity.** Final colors, typography, spacing, radii, shadows and interaction timing are all specified below and in the HTML. Recreate pixel‑perfectly, then wire real data/transport.

## Brand & Logo
- **Mark:** a single stylized **"V"** — a separated top chevron bar over a funnel that points down. It is NOT a diamond and NOT two overlapping V's.
- Source of truth: `logo.svg` (app icon: `#2969FF` rounded square, white V, corner radius 10.75/48 ≈ 22.4%) and `logo-mark.svg` (bare blue V on transparent).
- Brand blue: **`#2969FF`** (Vutric Blue). Use this for the logo and brand surfaces.
- Note: the **iOS‑native theme** intentionally uses the Apple system blue **`#007AFF`** for in‑app actions — that is correct platform behavior, not a brand mismatch.

## Screens / Views (app)
Device frame everywhere: 393×852 pt, corner radius 55 px, titanium bezel, Dynamic‑Island notch (124×36, top 11). Status bar 54 px tall, SF Pro 17/600 "9:41" left, signal/wifi/battery glyphs right. Home indicator: 134×5 pill, bottom 8, 35% ink.

**S1 — Home** (3 states: ready / preparing (QR sheet) / failed)
- Top: logo + "vutric" wordmark, right‑aligned editable device‑name pill ("iPhone").
- Title "Nimani yuboramiz?" (34/600, −5%).
- Large dark primary panel "Rasm otish" with a round 56 px accent send button; a secondary 92 px "Rasm qabul qilish" row; a QR row.
- Floating Liquid‑Glass tab bar at the bottom (Otish / Qabul / Tarix) + a separate round QR button.

**S2 — QR scanner** — black screen, four accent corner brackets (56 px, 4 px, radius 28), animated scan line (`vs-scan`/`vt-scan` 2.6 s ease‑in‑out), title "Kompyuterdagi QR kodni skanerlang", mono hint `vutric --listen`, glass "Havolani qoʻlda kiritish".

**S3 — Send (the key screen, 9 states):** ulanmagan · ulanmoqda · ulangan rasm yoʻq · rasm tayyor · sudrash paytida · uchib ketdi · yetib bordi · rad etildi · javob yoʻq.
- Connected device pill (green dot + halo, name, "· Windows", mono check‑code `000092`).
- Image card centered: **250×334, radius 22, 5 px raised ring, shadow `0 36px 60px -24px rgba(0,0,0,.5)`**.
- Hint "Yuborish uchun yuqoriga suring" with two rising chevrons (`vt-rise` 1.6 s, staggered .3 s).
- Buttons row `1fr 1.4fr`, height 56, radius pill: "Boshqa rasm" (secondary), "Yuborish" (accent, send icon).
- Rule: the card never goes under the device pill; result/error pills sit at the bottom near the thumb.

**S4 — Receive (5 states):** tanlov · kutish (my QR) · ulangan kutilmoqda · rasm keldi · aloqa uzildi.

**macOS catch scene** — MacBook frame: menu bar (Finder/Fayl/Tahrir/Koʻrinish/Oʻtish/Oyna), notch that expands into a Liquid‑Glass island showing the incoming photo + progress, portal slit on the right edge, photo card landing center (300×400, 5 px white ring), glass toast top‑right, glass Dock bottom. Phone on the right flicks the card off its **left** edge.

## Interactions & Behavior
- **Flick gesture (prototype logic in the DC class):**
  - Drag: card follows the finger 1:1. Tilt `rotate = dx × 0.05° − up × 0.02`; scale down up to 14% as it rises. Top‑edge portal opacity ramps `min(up/170, 1)`.
  - Throw threshold: `dy < −150 px` OR velocity `vy < −0.5 px/ms`.
  - Fling out: `transform 280 ms cubic-bezier(.3,.7,.4,1)`, travels above everything, `scale .7`; `navigator.vibrate(12)` (light haptic).
  - Return: from below, `560 ms cubic-bezier(.34,1.56,.64,1)` (easeOutBack).
  - Result pill fades in 220 ms + 8 px rise.
  - **Reduced motion:** every effect becomes "Oddiy" — a 200 ms fade, no travel.
- **Effect phases (brand effect "Halqa", travel ≈ 1.05 s):** Open 0–24% (ring grows, back‑ease), Fly 10–62% (Bézier shaped by throw, 4 contour echoes), Land 62–100% (spring 3.2 Hz ζ 0.42), Close 60–90%, Hold 1.4 s, Dock 0.45 s (shrinks to 22%).
- **Themes:** 9 in the app, 5 on the site; switching swaps CSS custom properties on the root — color, font family, radius scale and control material (glass / flat / thick‑outline) all change together.

## State Management
- `phase`: ready | sending | arrived | rejected | timeout (drives S3).
- `theme`: active theme id; `outcome`: Yetib bordi | Rad etildi | Javob yoʻq (prototype outcome); `motion`: full | reduced.
- `photo` index + `sent[]` history (last 5); `ms` latency label.
- Real app adds: pairing session (QR payload `vutric://pair?v=1&n=<name>`), transport socket state, received‑files queue.

## Design Tokens
**iOS‑native (default) — light:** bg `#F2F2F7`, raised `#FFFFFF`, ink `#000000`, muted `rgba(60,60,67,.6)`, line `rgba(60,60,67,.12)`, action/accent `#007AFF`, ok `#34C759`, err `#FF3B30`, glass `rgba(255,255,255,.62)`.
**iOS‑native — dark:** bg `#000000`, raised `#1C1C1E`, ink `#FFFFFF`, muted `rgba(235,235,245,.6)`, accent `#0A84FF`.
**Brand blue:** `#2969FF`. **Other site themes:** Daylight, Graphite (`#6E9BFF`), Ocean (`#2BD4C4`), Sand (`#C8631C`).
**Radius:** 8 / 14 / 22 / 999 (pill). Card 22. Panel 26. **Button height:** 44 (desktop) / 48–56 (phone). **Touch target ≥ 44.**
**Shadow (glass):** `inset 0 .5px 0 rgba(255,255,255,.95), inset 0 0 0 .5px rgba(255,255,255,.6), 0 0 0 .5px rgba(0,0,0,.06), 0 8px 24px -8px rgba(0,0,0,.16)`.
**Type:** SF Pro Text/Display on Apple (`-apple-system, BlinkMacSystemFont, 'SF Pro Text'`), Geist elsewhere; mono SF Mono / Geist Mono. Scale: Display 64/700 −5%, Title 32/600 −3%, Heading 20/600, Body 17/400, Label 13/500, Mono 14/400.
**Easing:** out `cubic-bezier(.2,.8,.2,1)` 180 ms; open back `cubic-bezier(.34,1.6,.64,1)` 250 ms; land spring 3.2 Hz ζ 0.42. Interface motion stays < 250 ms; only the effect runs longer.
**Copy language:** Uzbek Latin, apostrophe ʻ (U+02BB) in oʻ/gʻ. Sentence case.

## Assets
- `logo.svg`, `logo-mark.svg` — the Vutric V mark (regenerated from `wonmortech/wonmor.io` `public/brand/icon-blue.svg` + `mark-blue.svg`, reduced to a single V and recentered).
- `qr-big.svg`, `qr-small.svg` — decorative QR patterns (replace with a real QR encoder in production).
- `fonts/geist.woff2`, `fonts/geist-mono.woff2` — Geist variable fonts (OFL).
- All photo thumbnails are CSS gradients standing in for real images — swap for real media.

## Files
- `Vutric App v4.dc.html` — full app (phone screens + macOS scene + 9 themes + flick prototype). Newest app version.
- `Vutric Site.dc.html` — Canopy‑style landing / design‑system site (animated logo, 5 themes).
- `logo.svg`, `logo-mark.svg`, `qr-big.svg`, `qr-small.svg`, `fonts/` — assets referenced above.
- Earlier iterations (`Vutric App.dc.html` … `v3`) are historical and can be ignored.
