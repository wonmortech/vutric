# ADR-0016: The site's signature is a 3D Halqa throw, and the site shows every capability

- Status: **proposed** (owner go/no-go pending — the roadmap requires an explicit go/no-go per phase).
  Nothing is built yet; this records the direction before code. Stream W1 (design owns `design/**`); the
  marketing/design-system site in `site/` has no single stream owner today — see the open question below.
- Date: 2026-11-19
- Builds on: [DESIGN.md](../../DESIGN.md) (direction card), [ADR-0011](0011-phone-ui-from-claude-design.md)
  (phone UI + throw numbers), [docs/effects.md](../effects.md) (the Halqa phases),
  [ADR-0005](0005-any-to-any-offline-first.md) (the feel is the product).
- Does not affect: W3's `vutric-core`/`vutric-net` API. W2's overlay only in that the site's 3D timeline
  must stay in parity with the WGPU overlay timeline.

## Context

The owner asked (2026-11-19) for "the most modern, 3D, animated, very professional, very *vau*" design for
Vutric, and then added: **a visitor to the site must be able to see every capability and every case Vutric
offers, including items that are planned but not built yet.**

The live site `vutric.wonmor.io` today is a competent **design-system reference** — sections `colour, type,
foundations, components, effect, screens, desktop`. Measured baseline (2026-11-19, desktop, this machine):
FCP 168 ms, load 147 ms, **71 KB** transferred (7 KB JS, 52 KB fonts), 7 requests, **60 FPS** hero, console
clean, Lighthouse A11y 96 / Best-Practices 100 / SEO 100 / Agentic 50. It is already very fast and very light.

Two gaps, against the two asks:

1. **Signature strength.** The hero is quiet: the Halqa portal is drawn only in the Effect section (a 2D
   `<canvas>`, DPR 1), not as the first thing a stranger sees. vau-design rule 1: the living thing must be the
   product's own subject, shown first. Ours exists (the throw through the portal) but is understated.
2. **Capability coverage.** The site shows the design system and the first-release screens, but not the product
   capability map — transfer routes R0–R10, discovery & pairing, security & trust, interop adapters (ADR-0010),
   platform roadmap, storage settings, and the full set of states and failure cases. The owner wants all of it
   visible.

## Decision

| # | Decision | Why | Where |
|---|---|---|---|
| 1 | **The hero signature is the Halqa throw rendered as real 3D** — a photo card flung from the Send Zone edge, through a volumetric portal, landing on the underdamped spring (3.2 Hz, ζ 0.42). three.js / react-three-fiber + a GLSL portal shader; it answers cursor and scroll. Not a decorative orb, blob or particle field | The throw+portal is the one thing only Vutric does; vau is the signature, not polish | `site/` hero; see DESIGN.md "Signature" |
| 2 | **The 3D is lazy-loaded and code-split; the 2D canvas is the fallback.** The base document must stay in the current ~71 KB class. WebGL absent → the existing 2D Halqa canvas. `prefers-reduced-motion` → **Oddiy** (200 ms fade, no travel) | The owner's bar is "fastest and lightest"; a 3D hero that regresses the lean baseline fails the bar | `site/` boot path; fallback already present as `canvas[data-halqa]` |
| 3 | **The site shows every capability and every case, built or not.** Each item carries an honest status chip — `Built` / `In progress` / `Planned P1–P5` / `Opt-in` / `Watched` / `Not possible` — sourced from the ADRs, roadmap and backlog. An unbuilt capability gets a real mockup with a `Soon` chip, never a blank or a false claim | The owner's explicit brief (2026-11-19); vau-design rule 6: the truth is visible, and Soon is honest | the capability-map content, derived in `docs/reports/0016-vutric-site-capability-map.md` |
| 4 | **Only measured numbers appear on the site.** FPS, frame-time and preview latency come from the phase reports; nothing is invented | vau-design rule 6; the owner's "no claim without a measurement" | numbers traced to `docs/reports/phase-0.*.md` |
| 5 | **Frame budget holds at ≥ 60 FPS, frame-time p99 < 16.7 ms**, first effect frame < 16 ms after trigger (the Phase 0.1 overlay bar). Before/after is measured in a real browser and written into the report | Parity with the product's own overlay bar; no regression | `docs/reports/0016-vutric-site-vau.md` |
| 6 | **Page order:** signature hero → differentiators (offline, the throw, any-to-any) → the full capability tour (all cases, Soon chips) → the design-system reference stays beneath. The reference is not removed | The owner wants everything shown; vau leads with the signature, then completeness | `site/index.html` section order |

## Consequences

- **Positive.** A stranger sees the throw in 3D within three seconds and understands the product; a serious
  visitor can find every route, adapter, platform and case with an honest status. The design-system reference
  is kept for implementers.
- **Cost / risk.** 3D adds bundle weight — mitigated by lazy-load, code-split, DPR cap and the 2D fallback
  (decision 2); the before/after report must prove the base stayed lean. The capability tour must track the
  backlog, or it drifts out of date — it cites ADR/backlog ids so drift is visible.
- **Negative.** More surface to maintain; the capability map is a living document tied to `docs/backlog.md`.

## Open question (for the owner)

`site/` has no single stream owner in `docs/workstreams.md` (W1 owns `design/**`, not `site/**`). Who owns the
site work — W1, a new design stream, or the separate `wonmortech/wonmor.io` repo the handoff mentions? Logged to
`docs/owner-questions.md`.

## Measurements

- **Before (2026-11-19, desktop 1440×900, this machine):** FCP 168 ms · load 147 ms · 71 KB (JS 7 KB, fonts
  52 KB, img 1 KB) · 7 requests · hero 60 FPS · console clean · Lighthouse A11y 96 / BP 100 / SEO 100 / Agentic 50.
- **After (targets):** base document still ~71 KB-class before the lazy 3D chunk; 3D hero ≥ 60 FPS, p99 < 16.7 ms;
  LCP not regressed; A11y ≥ 96; console clean; every language and both themes verified.
