# Vutric — design direction

One-page contract for how Vutric looks and moves. Every surface inherits it: the **phone app**
(W1), the **desktop overlay** (W2) and the **vutric.wonmor.io design-system site**. It does not
restate the full token list — that lives in [`design/claude-design/README.md`](design/claude-design/README.md)
and on the live site at `/#colour` — it fixes the *direction* so a "vau" pass adds the signature
instead of only polishing speed and spacing.

Related: [`docs/effects.md`](docs/effects.md) (effect phases), [ADR-0016](docs/decisions/0016-halqa-3d-signature.md)
(the 3D signature decision), [ADR-0011](docs/decisions/0011-phone-ui-from-claude-design.md) (phone UI).

## Direction card

| Field | Value |
|---|---|
| **Subject noun** | a photo card **thrown off a phone's edge**, and the luminous **Halqa** portal it flies through |
| **States** | Open → Fly → Land → Dock — the throw's own four phases ([effects.md](docs/effects.md)); one per scroll chapter on the site |
| **Register** | **product** (a consumer/developer tool; dark is the default, light is a real second theme) |
| **Accent** | **Signal 500 `#2F6BFF`** (portal rim, live-connection dot) with **Glow 200 `#F4D9A6`** (inner rim, hot core) — taken from the effect's own light, never arbitrary |
| **Type** | **Geist** display/body + **Geist Mono** for data and check-codes; SF Pro on Apple surfaces. Geist has full Cyrillic, so Russian never falls back |
| **Signature** | **the Halqa throw** — a card flung from the Send Zone edge, through a volumetric portal, landing on an underdamped spring (3.2 Hz, ζ 0.42). *Technique today:* 2D `<canvas>` on the site + WGPU on the desktop overlay. *Vau upgrade:* the site hero becomes a real 3D scene (three.js / R3F + GLSL) that answers cursor and scroll, with the 2D canvas as its fallback |
| **Second moment** | **the material morph** — one control swaps glass ↔ flat ↔ thick-outline and the tokens re-resolve live. At most this one extra moment |
| **Differentiators (shown first)** | (1) **offline** — no cloud, no account, travels inside one Wi-Fi; (2) **the throw gesture** itself; (3) **any-to-any, any file** — phone↔desktop↔phone |
| **Deliberately off** | no floating orbs, particle fields or aurora; no fake 3D replica of a real object; no AI-generated hero video; no fade-up on every section; no invented numbers or testimonials |

> Test (vau-design): would this card fit another business? No — the noun is the throw+portal, which
> only Vutric has. If a future edit makes the card generic, the noun drifted; come back here.

## Colour — cobalt and sand

Neutral interface; colour only for **action** and **the effect**. Full values on the live site `/#colour`.

| Token | Hex | Role |
|---|---|---|
| Cobalt 900 | `#062B8A` | ink, primary button, focus (light theme) — 8.9:1 on Sand 200 |
| Signal 500 | `#2F6BFF` | portal outer rim, links on night, live dot |
| Glow 200 | `#F4D9A6` | portal inner rim / hot core; primary action (dark theme) |
| Sand 200 | `#E6DBC7` | ground of the light theme |
| Night 950 | `#040C29` | inside the portal; dark-theme ground / hero island |

Keep the hero a **dark island** even in light mode (`data-theme="dark"` on the hero) — shaders and 3D
read better on Night, and tokens re-resolve inside it.

## Motion contract

- **Interface motion stays < 250 ms**; only the effect runs longer (travel ≈ 1.05 s).
- **Easing:** out `cubic-bezier(.2,.8,.2,1)` 180 ms · open/back `cubic-bezier(.34,1.6,.64,1)` 250 ms ·
  land = underdamped spring **3.2 Hz, ζ 0.42**. Soft and physical — never snap, never linear.
- **Throw physics** (phone, from the handoff): drag 1:1, tilt `rotate = dx×0.05° − up×0.02`, scale −14%
  on rise; threshold `dy < −150 px` OR `vy < −0.5 px/ms`; out 280 ms, back 560 ms easeOutBack; light haptic.
- **Never tween** `filter`, `backdrop-filter` or `box-shadow`; progress is `scaleX`.
- **Reduced motion → Oddiy:** every effect collapses to a 200 ms fade, no travel. This is also a chosen
  effect, not only the accessibility fallback.

## Where "vau" lives

| Surface | Stream | Today | Vau target |
|---|---|---|---|
| Site hero (`site/`) | design (W1-adjacent) | 2D canvas Halqa | real 3D portal-throw (R3F + GLSL), cursor/scroll, 2D fallback |
| Desktop catch (`crates/vutric-overlay`) | **W2** | WGPU, Halqa | Kometa/Pufak + many-file stack; parity with the site timeline |
| Phone flick (Flutter) | **W1** | spec + prototype | the throw gesture with the physics above + Halqa on-device |

## Scope — the site shows everything

The owner's brief (2026-11-19) overrides the usual "differentiators first, commodity later": a visitor must be
able to see **every capability and every case**, built or not. So the page leads with the signature and the three
differentiators, then opens into a **complete capability tour** — routes R0–R10, discovery & pairing, security,
storage, platforms, interop adapters, file behaviour, companion extras, and the full set of states and failure
cases — each with an honest status chip (`Built` / `In progress` / `Planned P1–P5` / `Opt-in` / `Watched` /
`Not possible`). The design-system reference stays beneath it. Content map: [`docs/reports/0016-vutric-site-capability-map.md`](docs/reports/0016-vutric-site-capability-map.md).

**Honesty banner (non-negotiable, ADR-0007 §3):** no real-device Wi-Fi throughput number exists yet — only the
overlay FPS/latency is production-representative. The site quotes **only** the measured numbers listed in the
capability map; "Built · device check pending" reads as *built, verifying on devices*, never "works", and no
real-world speed is claimed for any route.

## Quality gates (owner's bar: fastest, lightest, measured)

- **Frame budget:** ≥ 60 FPS, frame-time **p99 < 16.7 ms**; first effect frame **< 16 ms** after trigger
  (matches the Phase 0.1 overlay exit criteria — the site hero holds the same bar or it is cheapened).
- **Site vitals:** LCP, CLS, INP measured before/after; the 3D hero must not regress LCP. Lazy-load the
  3D, cap DPR, fall back to the 2D canvas when WebGL is unavailable.
- **No performance claim ships without a measurement**, written into an ADR or a `docs/reports/` file.
- **One system:** logo, type, colour, buttons, scrollbars and any third-party widget read as one hand.
- **Prove it in a real browser:** desktop 1440×900 + phone 390×844, light + dark, every language, console
  clean; frames measured while scrolling.

## Do not reproduce

The reference product's effects are off-limits (rainbow aurora, concentric fire rings, paper plane,
volcano). Our effects are **Halqa** (default), and planned **Kometa**, **Pufak**, **Oddiy** only.
