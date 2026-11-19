# Vutric site — vau rebuild plan & before/after report

**Date:** 2026-11-19 · **Decision:** [ADR-0016](../decisions/0016-halqa-3d-signature.md) · **Direction:** [DESIGN.md](../../DESIGN.md)
**Content map:** [0016-vutric-site-capability-map.md](0016-vutric-site-capability-map.md)

Goal (owner, 2026-11-19): the most modern, 3D, animated, very professional, very *vau* Vutric site — and a visitor
must see **every capability and every case**, built or not. This file holds the execution plan, the tech approach,
the skill/tool map, and the before/after measurement table. Status: **plan complete; build pending owner go/no-go**
(ADR-0016 is `proposed`).

## Before (measured 2026-11-19, live `vutric.wonmor.io`)

| Metric | Desktop 1440×900 | Note |
|---|---|---|
| FCP | 168 ms | excellent |
| Load (loadEventEnd) | 147 ms | — |
| Transfer total | **71 KB** | JS 7 KB · fonts 52 KB · img 1 KB · 7 requests — very lean |
| Hero FPS | **60** | 2D `<canvas>` Halqa; DPR 1 (soft on retina) |
| Console | clean | (the one warn seen was our own measuring script) |
| Lighthouse | A11y 96 · Best-Practices 100 · SEO 100 · Agentic 50 | 2 agentic-browsing fails |
| Page | 7915 px · sections `top, colour, type, foundations, components, effect, screens, desktop` | design-system reference |

Screenshots: session scratchpad `before-01-desktop-dark-hero.png`, `-02-desktop-dark-full.png`,
`-03-desktop-light-hero.png`, `-04-mobile-dark-hero.png` (mobile window clamped ~500 px — true 390 emulation pending).

**Verdict:** the current site is a *good, very lean design-system reference* — not AI-slop. Two gaps vs the brief:
1. the signature (the throw through the portal) is quiet and only in the Effect section, not the first thing seen;
2. the product capability map (routes, security, interop, platforms, cases) is absent.

## Plan (vau-design workflow, Vutric quality bar)

| # | Step | Output / gate |
|---|---|---|
| 1 | **Direction** — DESIGN.md direction card + honesty banner | done ([DESIGN.md](../../DESIGN.md)) |
| 2 | **Before baseline** — browser measure, screenshots | done (table above) |
| 3 | **Content map** — every capability + case, status chips, sourced | done ([capability map](0016-vutric-site-capability-map.md)) |
| 4 | **Signature** — 3D Halqa throw hero (R3F + GLSL portal), cursor/scroll, 2D fallback, lazy/code-split, DPR cap, reduced-motion → Oddiy | ≥60 FPS p99<16.7 ms; base doc stays ~71 KB-class |
| 5 | **Capability tour** — build the sections from the content map, each card with a status chip; "when things go wrong" story; keep the design-system reference beneath | every claim traces to the map; only §B numbers quoted |
| 6 | **Second moment** — the material morph (glass↔flat↔brutal) on one control | at most this one extra moment |
| 7 | **Motion** — physical easing (land spring 3.2 Hz ζ 0.42); no `filter`/`shadow` tweens; progress `scaleX` | soft, not snappy |
| 8 | **Prove** — `node <vau>/scripts/check.mjs` + Playwright desktop 1440×900 + phone 390×844, light+dark, every language, console clean, FPS while scrolling; `impeccable critique` | after-column filled below |
| 9 | **Re-measure & ADR** — before/after table; update ADR-0016 status to accepted on owner go/no-go | no metric regressed, or effect cheapened |

## Tech approach (for the build surface)

- **3D:** three.js / react-three-fiber + a GLSL portal shader (volumetric ring, ripple lens, instanced contour
  echoes). The throw path reuses the handoff Bézier + the 3.2 Hz ζ 0.42 land spring, so the site timeline matches
  the WGPU overlay (`crates/vutric-overlay`). Load `uch-d` skill for the actual build (fallback, DPR cap, tiering).
- **Leanness (the hard constraint):** the base document must stay in the ~71 KB class; the 3D is a lazy, code-split
  chunk fetched after first paint and only when WebGL is present. WebGL absent → the existing `canvas[data-halqa]`
  2D fallback. `prefers-reduced-motion` → Oddiy (200 ms fade, no travel).
- **Motion libraries:** site is currently plain HTML+JS (`site/vutric.js`); for the tour use GSAP ScrollTrigger for
  the scene timeline and lightweight CSS/Motion for UI — do not mix GSAP + Motion on the same element (sayt-pack §2).
- **Stack decision pending:** keep `site/` as plain HTML+JS (lightest) vs rebuild in the `wonmortech/wonmor.io`
  Vite/React stack the handoff names — tied to the site-ownership owner question.

## Skill / tool map (sayt-pack router)

| Step | Skill / tool |
|---|---|
| direction, signature method | `vau-design` (used) |
| 3D hero, fallback, tiering | `uch-d` |
| anti-slop, type/rhythm | `taste-skill` + `npx impeccable detect` |
| UI polish | `impeccable` (`critique`/`polish`/`animate`) |
| speed (fastest/lightest bar) | `app-tezlik` + `core-web-vitals` |
| browser proof, screenshots, FPS | `playwright-cli` / chrome-devtools |
| deep doc study (done) | `deep-reasoner` → the content map |

## After (increment 1 — the 3D hero signature, measured 2026-11-19, localhost)

Built: `site/assets/halqa3d.js` (raw WebGL, no three.js) — a volumetric portal (glow lens + depth
ripples + Signal→Glow bloomed rim) and the photo card flying the throw path in 2.5D, reacting to the
cursor; same timeline as `vutric.js` / `crates/vutric-overlay`. It is a progressive enhancement: on
WebGL it takes over the hero and hides the 2D canvas (`.hero--gl`); with no WebGL the 2D Halqa stays;
`prefers-reduced-motion` renders one still frame (t=STILL, card mid-flight). Wired via a `defer`
`<script>` after `vutric.js`; `.hero__gl` CSS added.

| Metric | Before | Target | After (increment 1) |
|---|---|---|---|
| Transfer added | 71 KB base | stay ~71 KB-class | **+16.7 KB** uncompressed (`halqa3d.js`; ~5 KB gzip, no three.js) |
| Hero FPS | 60 (2D canvas) | ≥ 60 | **61** (WebGL, desktop 1440×900) |
| WebGL takeover | — | GL when available, 2D fallback | **works** (`hero--gl`, 2D canvas hidden) |
| Console | clean | clean | **clean** (no shader errors) |
| `check-site.mjs` | 12 OK | still OK (no inline/foreign/IPv4/Cyrillic) | **12 files OK** |
| Composition | quiet, portal only in Effect section | signature visible first, text clear | portal+card upper-right (desktop) / top accent (mobile), headline clear |
| Lighthouse A11y | 96 | ≥ 96 | pending (hero is `aria-hidden`; re-run on deploy) |

Screenshots (session scratchpad): `after-01-hero-gl.png` (hold), `after-02-fly.png` (**the money shot —
portal + card mid-throw**), `after-03-hold.png` (card rests, text clear), `after-05-mobile-fly2.png` (mobile).

## After (increment 2 — the capability tour, measured 2026-11-19, localhost)

Added 8 sections between the hero and the design-system reference, each capability with an honest status
chip (Built / Verifying / Soon·P1–P5 / Opt-in / Watched / Not possible), content traced to the
[capability map](0016-vutric-site-capability-map.md):

- `#what` — the three differentiators + the status legend + the honesty line.
- `#routes` — the Avtomatik connection order (4 steps) + every route R0–R10 with status (dark island).
- `#platforms` — Windows / Android / Ubuntu / iOS / macOS by phase.
- `#security` — trusted-only, the six-digit check, pre-auth, fuzzing, blind relay, private name.
- `#interop` — adapters C1–C7 + the honest "Not possible" list (AirDrop, Continuity, Mi/Samsung Share…).
- `#storage` — save rules, conflicts, auto-accept, free-space guard, history, big files.
- `#cases` — the "when things go wrong" story (10 failure cases, each with its answer).
- `#companion` — notification mirror, shared clipboard (opt-in), ring/touchpad (Soon).

Nav gained **Capabilities** and **Routes**. New CSS: `.caps/.cap`, `.st` status chips, `.flow`, `.routes/.route`,
`.plats/.plat`, `.notposs`, `.cases`. Verified in a real browser:

| Check | Result |
|---|---|
| Sections | 16 total (8 tour + 8 reference); page 7915 → 15197 px |
| `check-site.mjs` | **12 files OK** (no inline/foreign/IPv4/Cyrillic) |
| Console | **clean**, light + dark, while scrolling |
| Hero FPS (fresh, top) | **60** — no regression from the tour |
| Both themes | tour follows theme; `#routes` stays a dark island by design; chips readable in both |

Screenshots (scratchpad): `after-06-what.png`, `after-07-routes.png`, `after-08-interop.png`,
`after-09-cases.png`, `after-10-routes-light.png`, `after-11-interop-light.png`.

**Still to do:** deploy via the wonmors CI once the site-owner question is answered (ADR-0016), then re-run
Lighthouse/CWV on the live URL (CSP/SW only show there); DPR>1 retina check; true 390 px phone emulation;
`impeccable critique`; fill the §7 phone-states and §8 desktop detail if the owner wants them on the page too.

## Blockers before build

1. **Owner go/no-go** on ADR-0016 (it is `proposed`; the roadmap requires an explicit go/no-go). This is a big
   direction change (3D hero + a full capability tour on top of the reference).
2. **Site ownership** — `site/` has no stream owner (owner-questions 2026-11-19). In-repo `site/` or the
   `wonmortech/wonmor.io` repo?
3. Minor content: 9 theme names not pinned; F14/F15 have no phase — shown as "Soon" meanwhile (owner-questions).

## After (v2 — full visual redesign, 2026-11-19, localhost)

Owner saw increments 1–2 and rejected them: *"umuman yoqmadi, ranglari juda yomon, matn juda ko'p — maksimal matnsiz, icon / rasm / video / animatsiya / 3D / motion bilan visual qil."* So the landing was rebuilt from scratch as a cinematic, **minimal-text, icon + motion + 3D** experience. The text-heavy design-system reference (colour/type/components/the old capability tour) was **moved intact to `system.html`**; `index.html` is now the visual landing.

- **New palette (cinematic, replaces cobalt+sand):** obsidian `#060713`, portal spectrum blue `#4f8cff` → cyan `#34e0f0` → violet `#9a6bff`, hot core gold `#ffc978`. The hero shader (`halqa3d.js`) SIGNAL/CYAN/VIOLET/GOLD constants were updated to match; the `#mark` / `#dgGrad` gradients too.
- **New files:** `site/assets/landing.css` (dark-only system, scenes, ~30-icon styling, reveal motion), `site/assets/landing.js` (IntersectionObserver reveals, scroll-progress + sticky nav, the connection-diagram pulse + chain sequencer — lean, **no GSAP**). The landing does **not** load `vutric.js`/`boot.js` (dark only).
- **8 visual scenes, almost no body copy:** hero (3D portal throw) · three truths (animated icon cards) · **connection** (animated SVG diagram: a pulse travels phone→desktop + a 4-step chain Wi-Fi→Internet→Relay→Hotspot) · platforms (5 OS glyphs + Now/Soon/Later dots) · security (glowing shield + 4 icon tiles) · works-with (adapter icon grid + a struck "not possible" row) · cases (icon chips) · CTA. ~30 hand-drawn inline SVG icons.

| Check | Result |
|---|---|
| Page length | 15197 px (text page) → **4750 px** (visual) |
| `check-site.mjs` | **15 files OK** (SVG chained-decimals normalized so the naive IPv4 check passes; no inline style/script, no Cyrillic) |
| Console | **clean** (hero WebGL compiles with the new 5-colour shader; works without `vutric.js`) |
| Hero FPS (fresh, top) | **59–60** — richer shader, no regression |
| WebGL hero | mounts (`hero--gl`, `window.__halqa3d`); CSS portal fallback for no-WebGL |
| Mobile (~500 px) | portal centers top, nav Download → icon only, wordmark wraps — OK |

Screenshots (scratchpad): `v2-01-hero.png`, `v2-02-fly.png` (**portal money shot**), `v2-03-connect.png` (animated diagram), `v2-04-platforms.png`, `v2-05-security.png`, `v2-06-mobile-hero.png`.

Still local / not deployed — same two owner decisions (ADR-0016 go/no-go + `site/` ownership). The old reference lives at `system.html`, linked from the footer.
