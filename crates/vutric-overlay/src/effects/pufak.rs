//! Pufak (E3): the item drifts in inside a soft refractive bubble, which pops where it lands. The card then
//! springs to full size, rests and docks like Halqa.
//!
//! | Phase | Time (of `travel` = 1.05 s) | What happens |
//! |---|---|---|
//! | Float | 0 → 56 % | The bubble rises out of the Send Zone edge on a gentle curve, wobbling (squash ±4.5 %, sideways sway); the card inside is at 58 % size, magnified towards the bubble's rim (lens refraction), with a thin-film rim and two highlights. |
//! | Pop | 56 → 70 % | The skin flashes into an expanding ring and 12 droplets fly out with a little gravity; the card grows to full size with a back-ease overshoot and loses the refraction. |
//! | Hold, Dock | as Halqa | Rests with its normal shadow, then flies to the notification corner. |
//!
//! fx slots (mirrored in `pufak.wgsl`):
//! - `fx[0]`: bubble x, y, radius px, opacity
//! - `fx[1]`: squash x, squash y, refraction px, pop progress (0 before, 1 done)
//! - `fx[2]`: time s, card shadow strength, unused, unused

use vutric_core::SendZone;

use super::math::{self, Rect, SHADOW_PAD};
use super::scene::{FX_LEN, Kind, Scene};
use super::{Pose, Setup};
use crate::timeline::{
    Vec2, ease_in_cubic, ease_in_out_cubic, ease_out_back, ease_out_cubic, phase, spring,
};

/// Card scale while it rides inside the bubble.
const INNER: f32 = 0.58;

/// One Pufak fling laid out on a screen.
#[derive(Clone, Debug)]
pub struct Pufak {
    screen: Vec2,
    start: Vec2,
    control: Vec2,
    land: Vec2,
    dock_to: Vec2,
    along: Vec2,
    half: Vec2,
    radius: f32,
    energy: f32,
    float: f32,
    pop_end: f32,
    travel: f32,
    hold: f32,
    dock: f32,
}

impl Pufak {
    #[must_use]
    pub fn new(setup: &Setup) -> Self {
        let [w, h] = setup.screen;
        let short = w.min(h);
        let travel = math::finite_or(setup.cfg.travel, 1.05).max(0.2);
        let edge = math::edge(setup.zone, setup.screen, setup.gesture.exit_x);
        let energy = (math::finite_or(setup.gesture.speed, 0.0) / 3000.0).clamp(0.25, 1.0);
        let angle = math::finite_or(setup.gesture.angle, 0.0).clamp(-0.9, 0.9);
        let half = math::card_half(short * 0.34, setup.image_aspect);
        let radius = half[0].hypot(half[1]) * INNER * 1.08 + 6.0;

        // A bubble does not fly: a harder throw only floats it a little deeper.
        let depth = match setup.zone {
            SendZone::Top => h * (0.24 + 0.10 * energy),
            SendZone::Left | SendZone::Right => w * (0.18 + 0.10 * energy),
        };
        let land = math::add(
            math::add(edge.point, math::scale(edge.inward, depth)),
            math::scale(edge.along, angle * short * 0.06),
        );
        let control = math::add(
            math::add(edge.point, math::scale(edge.inward, depth * 0.5)),
            math::scale(edge.along, angle * short * 0.12),
        );
        Self {
            screen: setup.screen,
            start: math::add(edge.point, math::scale(edge.inward, -radius * 0.4)),
            control,
            land,
            dock_to: [w - short * 0.20, h - short * 0.12],
            along: edge.along,
            half,
            radius,
            energy,
            float: 0.56 * travel,
            pop_end: 0.70 * travel,
            travel,
            hold: math::finite_or(setup.cfg.hold, 1.4).max(0.0),
            dock: math::finite_or(setup.cfg.dock, 0.45).max(0.05),
        }
    }

    /// When the bubble has popped and the card is free.
    #[must_use]
    pub fn landed_at(&self) -> f32 {
        self.pop_end
    }

    #[must_use]
    pub fn duration(&self) -> f32 {
        self.travel + self.hold + self.dock
    }

    #[must_use]
    pub fn sample(&self, t: f32) -> Scene {
        let t = t.max(0.0);
        let floating = phase(t, 0.0, self.float);
        let pop = phase(t, self.float, self.pop_end);
        let docking = self.docking(t);
        let card = self.card_at(t, floating, pop, docking);

        let fade_in = ease_out_cubic(phase(floating, 0.0, 0.2));
        let bubble_opacity = if docking > 0.0 || floating <= 0.0 {
            0.0
        } else {
            fade_in * (1.0 - pop) * (1.0 - pop)
        };
        let radius = self.radius * (1.0 + 0.25 * ease_out_cubic(pop));
        let wobble = (t * 6.3).sin() * 0.045 * (1.0 - 0.5 * floating);
        let popping = pop > 0.0 && pop < 1.0 && docking <= 0.0;
        let shadow = if pop > 0.0 { 0.12 + 0.24 * pop } else { 0.12 };

        let mut fx = [[0.0; 4]; FX_LEN];
        fx[0] = [card.center[0], card.center[1], radius, bubble_opacity];
        fx[1] = [
            1.0 + wobble,
            1.0 - wobble,
            self.radius * 0.35 * (1.0 - pop),
            if popping { pop } else { 0.0 },
        ];
        fx[2] = [t, shadow, 0.0, 0.0];

        let bubble: Rect = if bubble_opacity > 0.0 || popping {
            // Droplets fly to about 1.8 radii and fall up to 40 px; the ring reaches 1.3 radii.
            math::clamp_rect(math::around(card.center, radius * 1.85 + 50.0), self.screen)
        } else {
            math::EMPTY
        };
        Scene {
            kind: Kind::Pufak,
            card,
            fx,
            regions: [
                math::card_region(&card, SHADOW_PAD, self.screen),
                bubble,
                math::EMPTY,
            ],
        }
    }

    fn docking(&self, t: f32) -> f32 {
        let start = self.travel + self.hold;
        ease_in_out_cubic(phase(t, start, start + self.dock))
    }

    fn card_at(&self, t: f32, floating: f32, pop: f32, docking: f32) -> Pose {
        let s = ease_out_cubic(floating);
        let a = math::lerp2(self.start, self.control, s);
        let b = math::lerp2(self.control, self.land, s);
        let sway = (t * 2.7).sin() * self.radius * 0.05 * (1.0 - floating);
        let mut center = math::add(math::lerp2(a, b, s), math::scale(self.along, sway));

        let since = (t - self.pop_end).max(0.0);
        let bump = if pop >= 1.0 {
            0.03 * self.energy * spring(since, 3.2, 0.42) * phase(since, 0.0, 0.05)
        } else {
            0.0
        };
        let base = if pop > 0.0 {
            math::lerp(INNER, 1.0, ease_out_back(pop, 1.6))
        } else {
            INNER * math::lerp(0.8, 1.0, s)
        };
        let mut scale = base * (1.0 + bump);
        let mut rotation = (t * 3.1).sin() * 0.05 * (1.0 - pop);
        let mut opacity = ease_out_cubic(phase(floating, 0.0, 0.2));

        if docking > 0.0 {
            center = math::lerp2(self.land, self.dock_to, docking);
            scale *= math::lerp(1.0, 0.22, docking);
            rotation *= 1.0 - docking;
            opacity *= 1.0 - ease_in_cubic(docking);
        }
        Pose {
            center,
            half: self.half,
            scale,
            rotation,
            opacity: opacity.clamp(0.0, 1.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{every_zone, setup};
    use super::*;

    #[test]
    fn card_rides_small_inside_the_bubble() {
        for zone in every_zone() {
            let p = Pufak::new(&setup(zone));
            let s = p.sample(p.float * 0.7);
            assert!(s.fx[0][3] > 0.5, "{zone:?}: bubble visible");
            assert!(s.card.scale < INNER + 1e-4, "{zone:?}: {}", s.card.scale);
            let card_reach = s.card.half[0].hypot(s.card.half[1]) * s.card.scale;
            assert!(card_reach < s.fx[0][2], "{zone:?}: card fits in the bubble");
        }
    }

    #[test]
    fn pops_then_the_bubble_is_gone() {
        let p = Pufak::new(&setup(SendZone::Right));
        let popping = p.sample((p.float + p.pop_end) * 0.5);
        assert!(popping.fx[1][3] > 0.0 && popping.fx[1][3] < 1.0);
        assert!(popping.regions[1] != [0.0; 4]);
        let after = p.sample(p.pop_end + 0.2);
        assert_eq!(after.fx[0][3], 0.0);
        assert_eq!(after.fx[1][3], 0.0);
        assert_eq!(after.regions[1], [0.0; 4]);
        assert!(after.card.scale > 0.97);
    }

    #[test]
    fn scale_is_continuous_through_the_pop() {
        let p = Pufak::new(&setup(SendZone::Left));
        let before = p.sample(p.float - 1e-4).card.scale;
        let after = p.sample(p.float + 1e-4).card.scale;
        assert!((before - after).abs() < 0.01, "{before} vs {after}");
    }
}
