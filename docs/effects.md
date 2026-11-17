# Effects

Our own effects. Do not reproduce the reference product's effects (rainbow aurora, concentric fire rings, paper
plane, volcano).

## Halqa (default) — implemented in `crates/vutric-overlay`

A single luminous portal opens at the Send Zone edge and the item flies out of it.

| Phase | Time (of `travel` = 1.05 s) | What happens |
|---|---|---|
| Open | 0 → 24 % | Ring radius grows with a back-ease overshoot; a night-blue lens with slow ripples gives depth. |
| Fly | 10 → 62 % | The card follows a cubic Bézier shaped by the throw: harder flicks land deeper, the angle bends the path and tilts the card. Four contour echoes trail it (signal blue → glow sand). |
| Land | 62 % → | Underdamped spring (3.2 Hz, ζ 0.42) settles scale and tilt; soft drop shadow. |
| Close | 60 → 90 % | Ring shrinks 35 % and fades. |
| Hold | 1.4 s | The card rests where it landed. |
| Dock | 0.45 s | It flies to the notification corner, shrinks to 22 % and fades; the toast takes over. |

Colours (sRGB, from the [design system](https://vutric.wonmor.io/#colour)): Signal 500 `#2F6BFF` → Glow 200 `#F4D9A6` on the rim, Night 950 `#040C29` inside.

## Planned

- **Kometa** — a streak with a particle tail, entering along the throw direction.
- **Pufak** — the item arrives inside a soft refractive bubble that pops on landing.
- **Oddiy** — a fast, quiet slide-in for people who want no show (also the reduced-motion choice).

All effects must respect the OS "reduce motion" setting by falling back to Oddiy.
