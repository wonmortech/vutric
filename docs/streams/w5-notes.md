# W5 notes (macOS + effects E2–E5)

W5's working notes for the coordinator and the other streams. Shared docs (backlog, workstreams, roadmap,
owner-questions) are not edited in this mode; their rows are proposed here instead.

## Lines W5 added to other streams' files

| File (owner) | Lines | Why |
|---|---|---|
| `crates/vutric-overlay/src/lib.rs` (W2) | `pub mod effects;` and `pub use effects::{Effects, Fling};` | Registers the new effects module. Nothing else in W2's files changed. |

## Effects (E2 Kometa, E3 Pufak, E4 Oddiy, E5 many files)

All of them are new files under `crates/vutric-overlay/src/effects/`:

- each effect is a timeline (pure Rust, unit-tested) plus one WGSL file;
- the WGSL is appended to `common.wgsl`;
- `scene.rs` is the renderer for all of them.

Halqa and W2's `Renderer` are unchanged. `effects::Effects` wraps both renderers, and `effects::Fling` chooses the
timeline from `EffectId`.

### Daemon patch for W2 (not applied; W2's file)

In `crates/vutric-daemon/src/bin/vutric-daemon/main.rs` and `gpu.rs`:

```rust
// main.rs
use vutric_overlay::effects::{Fling, Setup};
struct Run { fling: Fling, started: Instant, notify: Option<oneshot::Sender<Instant>> }

// play(): replaces `halqa: Halqa::new(cfg, gpu.screen(), zone, fling.gesture, aspect)`
let setup = Setup { cfg, screen: gpu.screen(), zone, gesture: fling.gesture, image_aspect: aspect };
let reduce = host_reduce_motion(); // Windows: SPI_GETCLIENTAREAANIMATION == FALSE; macOS: vutric_host_mac::reduce_motion()
let played = if reduce { Fling::still(setup) } else { Fling::new(self.prefs.effect, setup) };
// many files: `.with_files(total_files)` (E5); the daemon knows the count from `Message::Offer::total_files`

// draw(): replaces run.halqa.* with run.fling.* ; `gpu.draw(&sample, t)`

// gpu.rs: `renderer: vutric_overlay::Effects` instead of `Renderer`;
// `Effects::new(&device, &queue, config.format, Palette::vutric())`, same `set_image`,
// `draw(&mut self, sample: &vutric_overlay::effects::Sample, t)` calls `self.renderer.encode(.., sample, t)`.
```

`prefs.effect_for(reduce)` already picks Oddiy when the OS asks for reduced motion. `Fling::still` goes further:
the card does not even slide.

### E2 Kometa (`effects/kometa.rs`, `kometa.wgsl`)

The card is the head of a comet. It enters from off-screen along the throw, with a quartic ease-out so it brakes
into place, followed by a tapered streak and up to 28 sparks drawn in the shader, then a 0.35 s bloom when it
lands. Rest and dock work as in Halqa.

GPU cost: two scissored rectangles (card and tail). Inside the tail, 4 segment distances plus a spark loop with
early outs per pixel. Not measured on a GPU yet.

### E3 Pufak (`effects/pufak.rs`, `pufak.wgsl`)

The card rides at 58 % size inside a bubble that floats out of the edge with a wobble. The bubble works as a
lens: the card's sample point is pulled towards the centre near the rim, so the card looks magnified. It has a
thin-film rim (Signal to Glow), two highlights and a faint night-blue body. On landing the bubble pops over 14 %
of `travel`: the skin becomes an expanding ring, 12 droplets fall away, and the card springs to full size.

GPU cost: still one texture sample per pixel; the refraction only moves the coordinate.

### E5 many files (`effects/stack.rs`, `stack.wgsl`)

The first item is thrown with the chosen effect. Once it lands, up to 3 blank cards fan out behind it. Their face
is Night mixed towards white, with a Glow rim and a soft shadow. A Signal badge with seven-segment digits sits on
the top card's corner and shows the file count, capped at 99. The stack follows the top card's pose, so it rests,
docks and fades with it.

The stack is an overlay layer: it is blended over the main layer once per pixel, over the union of its
rectangles, and hidden where the top card covers it.

Host API: `Fling::new(effect, setup).with_files(n)`. One file adds nothing.

## macOS items

### Retina scale-2 card path

`crates/vutric-host-mac/examples/cards.rs --scale 2` forces the 2x path on the 1x CI screens, and
`tools/cards.sh` now runs it as a second pass. ADR-0118's Known gaps section is updated. No numbers yet: the
next CI run produces `cards-mac-2x.json`.

### Quick Action label i18n

The Quick Action's translated title is the top-level `CFBundleDisplayName` ("Vutric bilan yuborish" / "Send with
Vutric"). The nested preview-pane and Touch Bar labels are now just "Vutric" and are not translated, because
`InfoPlist.strings` is not documented to reach nested keys. `xcstrings.py` enforces top-level-only keys, and the
smoke test checks the built titles. Finder's menu itself still needs a look on a real Mac (ADR-0115 checklist).

### App sandbox and entitlements review (ADR-0120)

Decisions:
- The agent stays unsandboxed (Developer ID).
- No hardened-runtime exceptions on the agent or any helper.
- The extensions carry exactly app-sandbox + user-selected.read-only.
- The privacy keys follow the code.

New `packaging/macos/check-entitlements.sh` runs in the app job. It reads the Bonjour types from
`vutric_net::MDNS_SERVICE` and `vutric_compat_quickshare::endpoint::SERVICE_TYPE`. If W2 or W3 rename either
constant or move its file, the script's `sed` pattern must follow, or the check fails (on purpose).
Owner-questions row proposed (not edited): "Mac App Store distribution: yes/no? A yes means an XPC split of the
helpers (ADR-0120 §1)."
