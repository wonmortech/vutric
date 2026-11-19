# ADR-0122: The effects next to Halqa: Kometa, Pufak, Oddiy, and the many-files stack

- Status: accepted for implementation in `crates/vutric-overlay/src/effects/`. The timelines and shaders are
  written with tests as code; nothing has run on a GPU yet. The macOS probe (`vutric-mac-probe --effect`) measures
  them on the next CI run. They are not yet wired into the daemon (patch for W2 in `docs/streams/w5-notes.md`).
- Date: 2026-11-19
- Stream: W5, on the coordinator's request (E2–E5), in new files only. W2's Halqa (`timeline.rs`, `renderer.rs`,
  `halqa.wgsl`) is unchanged; `lib.rs` gains two registration lines.
- Builds on: docs/effects.md ("Planned"), ADR-0004 (60 fps, ≤ 2 ms GPU per frame), `vutric_core::EffectId`.

## Decision

1. **One shape for every effect.**
   - Each effect has a pure timeline, `new(&Setup)`, `sample(t) -> Scene`, `landed_at()`, `duration()`, unit-tested
     without a GPU.
   - Each also has one WGSL fragment shader appended to a shared prelude (`common.wgsl`: uniform layout, SDF
     helpers, `draw_card`).
   - `Scene` carries the card pose, 8 effect parameter slots and up to 3 lit rectangles.
2. **Selection.** `Fling::new(EffectId, Setup)` picks the timeline, and Halqa uses W2's own `Halqa`.
   `Fling::still(Setup)` is the reduced-motion choice: Oddiy with no slide and no scale, only a fade. A host checks
   the OS setting (`vutric_host_mac::reduce_motion()` on macOS) and calls `still`. The user's effect choice is not
   consulted at all then (docs/effects.md: "All effects must respect the OS reduce motion setting").
3. **Rendering.** `Effects` owns W2's `Renderer` and a `SceneRenderer` (one pipeline per kind, the same bind group
   shape).
   - A main layer clears and writes without blending, so overlapping rectangles are harmless, as in Halqa.
   - The stack is an overlay layer: it loads the target, blends premultiplied alpha, and is drawn once over the
     union of its rectangles.
   - The main and overlay layers use separate uniform buffers, so both can be written before one submit.
4. **The effects** (times are for the default `travel` of 1.05 s):

| Effect | What it does | Per-pixel cost inside its rectangles |
|---|---|---|
| **Oddiy** (E4) | 0.22 s slide of 5 % of the short side in from the Send Zone edge, scale 0.96 → 1, rest, 0.28 s fade drifting back. `still`: fade only. | 1 texture sample, 2 rounded-box SDFs |
| **Kometa** (E2) | The card is a comet head. It enters from off-screen along the throw (the angle tilts it along the edge) and brakes with a quartic ease-out. A tapered streak and up to 28 hashed sparks flow back along a 4-point tail, then a 0.35 s bloom marks the landing. | 4 segment distances, ≤ 28 sparks with a 4-radius early out, 1 texture sample |
| **Pufak** (E3) | The card rides at 58 % inside a bubble that floats out of the edge with squash and sway. The bubble is a lens: sample points are pulled to the centre near the rim. It has a thin-film Fresnel rim (Signal ↔ Glow), two highlights and a faint night-blue body. It pops at 56–70 %: the skin becomes an expanding ring, 12 droplets fall away, and the card back-eases to full size. | 1 texture sample (refraction moves the coordinate), ~10 exponentials, 12 droplet discs only during the pop |
| **Stack** (E5) | Overlay for a fling of several files. Once the top card lands, up to 3 blank cards fan out behind it (Night face, Glow rim, soft shadow). A Signal badge with seven-segment SDF digits shows the count, capped at 99. It follows the top card's pose, so it rests, docks and fades with it. | ≤ 3 cards × 2 SDFs, ≤ 14 segment distances in the badge |

5. **No fonts, textures or offscreen passes beyond the card image.** The colours come from the existing `Palette`
   (Signal 500, Glow 200, Night 950). Nothing reproduces a reference product's effect (docs/effects.md).

## Budget reasoning (ADR-0004)

Every pass is one scissored full-screen triangle per rectangle, so the cost scales with lit pixels, not screen
size. The largest rectangles:
- Kometa's tail in the first 100 ms: roughly 1000 × 500 px on a 2560 × 1440 screen.
- Pufak's bubble during the pop: radius × 3.7 square, about 900 × 900 px.

At about 40 ALU operations per spark and 28 sparks, the tail costs roughly 0.5 M px × 1.2 k ops ≈ 0.6 G ops per
frame. That is about 1 ms on a 2019 Intel UHD 630 (≈ 400 G ops/s), inside the 2 ms budget, and the rectangle
shrinks quickly as the head brakes. This is an estimate. The probe's numbers (fps, p99, missed refreshes) per effect
replace it after the next CI run (`measure.sh` rows `*-kometa-1`, `*-pufak-1`, `*-oddiy-1`, `*-halqa-12`).

## Consequences

- The daemon changes three things (W2):
  - `Run.halqa` becomes `Run.fling`;
  - `gpu.rs` holds `Effects` instead of `Renderer`;
  - `play()` picks `Fling::still` or `Fling::new(prefs.effect, ..)`, plus `.with_files(total_files)`.
- docs/effects.md (shared) should move Kometa, Pufak and Oddiy from "Planned" to "implemented", with the tables in
  each module's doc comment. Proposed in the W5 notes, not edited here.
- The settings preview (ADR-0030) can play any effect from the same timelines once it uses `Effects`.

## Not verified

- No effect has been seen on screen. The shaders pass naga validation in `effects::scene` tests (once those tests
  run), but their look and real GPU cost are unmeasured.
- Kometa's spark hash (`fract(sin(x) * 43758.5)`) can band on some mobile GPUs at high `x`. The seeds here are
  below 100.
