//! The effects next to Halqa (docs/effects.md, "Planned"): Kometa, Pufak, Oddiy, the many-files stack,
//! and the selection a host plays.
//!
//! Each effect is a pure timeline that turns a time into a [`Scene`] (unit-tested, no GPU) and one WGSL fragment
//! shader appended to the shared prelude `common.wgsl`. Halqa keeps W2's [`crate::Renderer`]; [`Effects`] owns
//! that renderer and the [`SceneRenderer`] for the others, so a host swaps one type and calls the same three
//! methods (`new`, `set_image`, `encode`). [`Fling`] picks the timeline from the [`EffectId`].
//!
//! GPU budget (ADR-0004: 60 fps, at most 2 ms of GPU time per frame on an integrated GPU): one full-screen triangle
//! per lit rectangle, scissored, analytic shading only, one texture sample per pixel, no offscreen passes.

pub mod kometa;
mod math;
pub mod oddiy;
pub mod pufak;
pub mod scene;
pub mod stack;

pub use kometa::Kometa;
pub use oddiy::Oddiy;
pub use pufak::Pufak;
pub use scene::{FX_LEN, Kind, Scene, SceneRenderer};
pub use stack::Stack;

use vutric_core::{EffectId, Gesture, SendZone};

use crate::palette::Palette;
use crate::renderer::Renderer;
use crate::timeline::{Frame, Halqa, HalqaConfig, Vec2};

/// Everything a fling's layout depends on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Setup {
    /// Timing. Every effect scales with `travel` the way Halqa does; `hold` is the rest before leaving.
    pub cfg: HalqaConfig,
    /// Target size in physical pixels.
    pub screen: Vec2,
    pub zone: SendZone,
    pub gesture: Gesture,
    /// Width / height of the flung item.
    pub image_aspect: f32,
}

/// The card's transform at one instant: the part every effect shares and the many-files stack follows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub center: Vec2,
    /// Half extents at scale 1.
    pub half: Vec2,
    pub scale: f32,
    pub rotation: f32,
    pub opacity: f32,
}

impl Pose {
    fn of_frame(f: &Frame) -> Self {
        Self {
            center: f.card_center,
            half: f.card_half,
            scale: f.card_scale,
            rotation: f.card_rotation,
            opacity: f.card_opacity,
        }
    }
}

/// The main layer of one frame: Halqa's own frame, or a [`Scene`] of one of the newer effects.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layer {
    Halqa(Frame),
    Scene(Scene),
}

/// One frame of a [`Fling`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub layer: Layer,
    /// Drawn over the main layer (the many-files stack), if any.
    pub overlay: Option<Scene>,
}

impl Sample {
    /// The flung card's transform.
    #[must_use]
    pub fn pose(&self) -> Pose {
        match &self.layer {
            Layer::Halqa(f) => Pose::of_frame(f),
            Layer::Scene(s) => s.card,
        }
    }

    /// One rectangle containing every lit pixel of both layers (all zero when nothing is lit).
    #[must_use]
    pub fn bounds(&self) -> [f32; 4] {
        let main = match &self.layer {
            Layer::Halqa(f) => f.bounds,
            Layer::Scene(s) => s.bounds(),
        };
        self.overlay
            .as_ref()
            .map_or(main, |o| math::union(main, o.bounds()))
    }
}

#[derive(Clone, Debug)]
enum Motion {
    Halqa(Halqa, HalqaConfig),
    Kometa(Kometa),
    Oddiy(Oddiy),
    Pufak(Pufak),
}

/// One fling with the effect chosen for it. Construct once per fling, then [`Self::sample`] every frame.
#[derive(Clone, Debug)]
pub struct Fling {
    effect: EffectId,
    motion: Motion,
    screen: Vec2,
    stack: Option<Stack>,
}

impl Fling {
    /// The effect the user picked (after the "reduce motion" rule; see [`Self::still`]).
    #[must_use]
    pub fn new(effect: EffectId, setup: Setup) -> Self {
        let motion = match effect {
            EffectId::Oddiy => Motion::Oddiy(Oddiy::new(&setup, false)),
            EffectId::Kometa => Motion::Kometa(Kometa::new(&setup)),
            EffectId::Pufak => Motion::Pufak(Pufak::new(&setup)),
            EffectId::Halqa => Motion::Halqa(
                Halqa::new(
                    setup.cfg,
                    setup.screen,
                    setup.zone,
                    setup.gesture,
                    setup.image_aspect,
                ),
                setup.cfg,
            ),
        };
        Self {
            effect,
            motion,
            screen: setup.screen,
            stack: None,
        }
    }

    /// What plays when the OS asks for reduced motion: Oddiy with no movement at all, only a fade.
    #[must_use]
    pub fn still(setup: Setup) -> Self {
        Self {
            effect: EffectId::Oddiy,
            motion: Motion::Oddiy(Oddiy::new(&setup, true)),
            screen: setup.screen,
            stack: None,
        }
    }

    /// A fling of `files` items (E5): the first one plays the effect, the rest appear stacked behind it with a
    /// counter once it has landed. One file (or zero) changes nothing.
    #[must_use]
    pub fn with_files(mut self, files: u32) -> Self {
        self.stack = Stack::new(files, self.landed_at(), self.screen);
        self
    }

    /// The effect that actually plays.
    #[must_use]
    pub fn effect(&self) -> EffectId {
        self.effect
    }

    #[must_use]
    pub fn duration(&self) -> f32 {
        match &self.motion {
            Motion::Halqa(h, _) => h.duration(),
            Motion::Kometa(k) => k.duration(),
            Motion::Pufak(p) => p.duration(),
            Motion::Oddiy(o) => o.duration(),
        }
    }

    #[must_use]
    pub fn is_finished(&self, t: f32) -> bool {
        t >= self.duration()
    }

    /// When the card has arrived where it rests (the stack of a many-files fling appears from here).
    #[must_use]
    pub fn landed_at(&self) -> f32 {
        match &self.motion {
            // Halqa's fly phase ends at 62 % of `travel` (timeline.rs).
            Motion::Halqa(_, cfg) => 0.62 * cfg.travel,
            Motion::Kometa(k) => k.landed_at(),
            Motion::Pufak(p) => p.landed_at(),
            Motion::Oddiy(o) => o.landed_at(),
        }
    }

    #[must_use]
    pub fn sample(&self, t: f32) -> Sample {
        let t = t.clamp(0.0, self.duration());
        let layer = match &self.motion {
            Motion::Halqa(h, _) => Layer::Halqa(h.sample(t)),
            Motion::Kometa(k) => Layer::Scene(k.sample(t)),
            Motion::Pufak(p) => Layer::Scene(p.sample(t)),
            Motion::Oddiy(o) => Layer::Scene(o.sample(t)),
        };
        let mut sample = Sample {
            layer,
            overlay: None,
        };
        let pose = sample.pose();
        sample.overlay = self.stack.map(|s| s.sample(t, &pose));
        sample
    }
}

/// Halqa's renderer and the scene renderer behind one interface. A host that today holds a [`Renderer`] and
/// calls `encode(.., &frame, t)` holds this instead and calls `encode(.., &fling.sample(t), t)`.
#[derive(Debug)]
pub struct Effects {
    halqa: Renderer,
    scene: SceneRenderer,
}

impl Effects {
    #[must_use]
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        palette: Palette,
    ) -> Self {
        Self {
            halqa: Renderer::new(device, queue, format, palette),
            scene: SceneRenderer::new(device, queue, format, palette),
        }
    }

    /// Replaces the card image in both renderers. `rgba` is straight sRGB, `width * height * 4` bytes.
    pub fn set_image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) {
        self.halqa.set_image(device, queue, rgba, width, height);
        self.scene.set_image(device, queue, rgba, width, height);
    }

    /// Records one frame: the main layer clears the target, an overlay is blended on top.
    pub fn encode(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        target_size: [u32; 2],
        sample: &Sample,
        time: f32,
    ) {
        match &sample.layer {
            Layer::Halqa(frame) => {
                self.halqa
                    .encode(queue, encoder, target, target_size, frame, time);
            }
            Layer::Scene(scene) => {
                self.scene
                    .encode(queue, encoder, target, target_size, scene, time);
            }
        }
        if let Some(overlay) = &sample.overlay {
            self.scene
                .encode(queue, encoder, target, target_size, overlay, time);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const SCREEN: Vec2 = [2560.0, 1440.0];

    pub fn every_zone() -> [SendZone; 3] {
        [SendZone::Left, SendZone::Right, SendZone::Top]
    }

    pub fn setup(zone: SendZone) -> Setup {
        Setup {
            cfg: HalqaConfig::default(),
            screen: SCREEN,
            zone,
            gesture: Gesture {
                speed: 2400.0,
                angle: 0.3,
                exit_x: 0.5,
            },
            image_aspect: 4.0 / 3.0,
        }
    }

    /// Hostile inputs every effect must survive with finite, on-screen output.
    pub fn odd_setups() -> Vec<Setup> {
        let mut out = Vec::new();
        for zone in every_zone() {
            for (speed, angle, exit_x) in [(0.0, -3.0, 0.0), (9000.0, 3.0, 1.0), (1200.0, 0.0, 0.5)]
            {
                for aspect in [0.0, 0.3, 1.0, 9.0, f32::NAN] {
                    out.push(Setup {
                        gesture: Gesture {
                            speed,
                            angle,
                            exit_x,
                        },
                        image_aspect: aspect,
                        ..setup(zone)
                    });
                }
            }
        }
        out
    }

    pub fn assert_sane(s: &Sample, what: &str) {
        let p = s.pose();
        let values = [p.center[0], p.center[1], p.scale, p.rotation, p.opacity];
        assert!(values.iter().all(|v| v.is_finite()), "{what}: {p:?}");
        let b = s.bounds();
        assert!(b.iter().all(|v| v.is_finite()), "{what}: {b:?}");
        assert!(b[0] <= b[2] && b[1] <= b[3], "{what}: {b:?}");
        assert!(b[2] <= SCREEN[0] && b[3] <= SCREEN[1], "{what}: {b:?}");
        if let Layer::Scene(scene) = &s.layer {
            assert!(scene.fx.iter().flatten().all(|v| v.is_finite()), "{what}");
        }
    }

    #[test]
    fn every_effect_is_finite_and_on_screen_at_240_hz() {
        for effect in EffectId::ALL {
            for setup in odd_setups() {
                let fling = Fling::new(effect, setup);
                let steps = (fling.duration() * 240.0) as usize;
                for i in 0..=steps {
                    assert_sane(
                        &fling.sample(i as f32 / 240.0),
                        &format!("{effect:?} {setup:?}"),
                    );
                }
            }
        }
    }

    #[test]
    fn every_effect_ends_invisible() {
        for effect in EffectId::ALL {
            let fling = Fling::new(effect, setup(SendZone::Right));
            assert!(fling.is_finished(fling.duration()));
            let end = fling.sample(fling.duration());
            assert!(end.pose().opacity < 1e-3, "{effect:?}");
        }
    }

    #[test]
    fn every_effect_lands_at_full_size_inside_the_screen() {
        for effect in EffectId::ALL {
            for zone in every_zone() {
                let fling = Fling::new(effect, setup(zone));
                let p = fling.sample(fling.landed_at() + 0.6).pose();
                assert!(
                    (p.scale - 1.0).abs() < 0.03,
                    "{effect:?} {zone:?}: {}",
                    p.scale
                );
                assert!(p.opacity > 0.99, "{effect:?} {zone:?}");
                let [x, y] = p.center;
                assert!(
                    x > 0.0 && x < SCREEN[0] && y > 0.0 && y < SCREEN[1],
                    "{effect:?} {zone:?}"
                );
            }
        }
    }

    #[test]
    fn many_files_stack_over_every_effect() {
        for effect in EffectId::ALL {
            let fling = Fling::new(effect, setup(SendZone::Right)).with_files(12);
            assert!(fling.sample(0.0).overlay.is_some_and(|o| o.fx[0][3] == 0.0));
            let rest = fling.sample(fling.landed_at() + 0.5);
            let stack = rest.overlay.expect("stack layer");
            assert_eq!(stack.kind, Kind::Stack);
            assert_eq!(stack.fx[0][0], 12.0);
            assert!(stack.fx[0][3] > 0.99, "{effect:?}");
            assert_eq!(stack.card, rest.pose(), "{effect:?}: follows the top card");
            assert_sane(&rest, &format!("{effect:?} stack"));
        }
        assert!(
            Fling::new(EffectId::Halqa, setup(SendZone::Left))
                .with_files(1)
                .sample(1.0)
                .overlay
                .is_none()
        );
    }

    #[test]
    fn reduced_motion_is_still_oddiy() {
        let fling = Fling::still(setup(SendZone::Top));
        assert_eq!(fling.effect(), EffectId::Oddiy);
        let a = fling.sample(0.1).pose().center;
        let b = fling.sample(fling.duration() - 0.1).pose().center;
        assert_eq!(a, b);
    }
}
