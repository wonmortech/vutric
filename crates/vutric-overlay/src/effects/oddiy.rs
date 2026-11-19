//! Oddiy (E4): a fast, quiet slide-in just inside the Send Zone edge, a rest, and a fade.
//!
//! It is also what plays when the OS asks for reduced motion (docs/effects.md): built with `still`, nothing moves
//! or scales; the card only fades in where it rests and fades out again.
//!
//! | Phase | Time | What happens |
//! |---|---|---|
//! | Enter | 0 → 0.22 s | Slides 5 % of the short screen side in from the edge (ease-out), scale 0.96 → 1, fades in. |
//! | Hold | `HalqaConfig::hold` | Rests. |
//! | Leave | 0.28 s | Fades out, drifting 2 % back towards the edge. |
//!
//! fx slots: none.

use vutric_core::SendZone;

use super::math::{self, SHADOW_PAD};
use super::scene::{FX_LEN, Kind, Scene};
use super::{Pose, Setup};
use crate::timeline::{Vec2, ease_in_cubic, ease_out_cubic, phase};

const ENTER: f32 = 0.22;
const LEAVE: f32 = 0.28;

/// One Oddiy fling laid out on a screen.
#[derive(Clone, Debug)]
pub struct Oddiy {
    screen: Vec2,
    from: Vec2,
    land: Vec2,
    to: Vec2,
    half: Vec2,
    enter: f32,
    hold: f32,
    still: bool,
}

impl Oddiy {
    #[must_use]
    pub fn new(setup: &Setup, still: bool) -> Self {
        let [w, h] = setup.screen;
        let short = w.min(h);
        let half = math::card_half(short * 0.30, setup.image_aspect);
        let edge = math::edge(setup.zone, setup.screen, setup.gesture.exit_x);
        // Rest just inside the edge the item came from, so the eye does not have to travel.
        let depth = match setup.zone {
            SendZone::Top => half[1],
            SendZone::Left | SendZone::Right => half[0],
        } + short * 0.06;
        let land = math::add(edge.point, math::scale(edge.inward, depth));
        let slide = if still { 0.0 } else { short * 0.05 };
        Self {
            screen: setup.screen,
            from: math::add(land, math::scale(edge.inward, -slide)),
            land,
            to: math::add(land, math::scale(edge.inward, -slide * 0.4)),
            half,
            enter: ENTER,
            hold: math::finite_or(setup.cfg.hold, 1.4).max(0.0),
            still,
        }
    }

    /// When the card is fully in place.
    #[must_use]
    pub fn landed_at(&self) -> f32 {
        self.enter
    }

    #[must_use]
    pub fn duration(&self) -> f32 {
        self.enter + self.hold + LEAVE
    }

    #[must_use]
    pub fn sample(&self, t: f32) -> Scene {
        let t = t.max(0.0);
        let e = ease_out_cubic(phase(t, 0.0, self.enter));
        let l = ease_in_cubic(phase(t, self.enter + self.hold, self.duration()));
        let center = if l > 0.0 {
            math::lerp2(self.land, self.to, l)
        } else {
            math::lerp2(self.from, self.land, e)
        };
        let scale = if self.still {
            1.0
        } else {
            math::lerp(0.96, 1.0, e)
        };
        let card = Pose {
            center,
            half: self.half,
            scale,
            rotation: 0.0,
            opacity: (e * (1.0 - l)).clamp(0.0, 1.0),
        };
        Scene {
            kind: Kind::Oddiy,
            card,
            fx: [[0.0; 4]; FX_LEN],
            regions: [
                math::card_region(&card, SHADOW_PAD, self.screen),
                math::EMPTY,
                math::EMPTY,
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{SCREEN, every_zone, setup};
    use super::*;

    #[test]
    fn rests_inside_the_screen_near_its_edge() {
        for zone in every_zone() {
            let o = Oddiy::new(&setup(zone), false);
            let f = o.sample(o.landed_at() + 0.3);
            let [x, y] = f.card.center;
            assert!(
                x > 0.0 && x < SCREEN[0] && y > 0.0 && y < SCREEN[1],
                "{zone:?}"
            );
            assert!((f.card.scale - 1.0).abs() < 1e-4 && f.card.opacity > 0.999);
            let near = match zone {
                SendZone::Right => SCREEN[0] - x,
                SendZone::Left => x,
                SendZone::Top => y,
            };
            assert!(near < SCREEN[1] * 0.4, "{zone:?}: {near} px from the edge");
        }
    }

    #[test]
    fn still_never_moves_or_scales() {
        let o = Oddiy::new(&setup(SendZone::Right), true);
        let rest = o.sample(o.landed_at()).card.center;
        let mut t = 0.0;
        while t <= o.duration() {
            let f = o.sample(t);
            assert_eq!(f.card.center, rest, "t = {t}");
            assert_eq!(f.card.scale, 1.0);
            t += 1.0 / 120.0;
        }
    }

    #[test]
    fn starts_and_ends_invisible() {
        let o = Oddiy::new(&setup(SendZone::Left), false);
        assert_eq!(o.sample(0.0).card.opacity, 0.0);
        assert!(o.sample(o.duration()).card.opacity < 1e-6);
        assert_eq!(o.sample(o.duration()).bounds(), [0.0; 4]);
    }
}
