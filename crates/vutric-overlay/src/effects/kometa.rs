//! Kometa (E2): the card is the head of a comet. It streaks in from beyond the Send Zone edge along the throw
//! direction, decelerating, with a tapered light streak and a stream of sparks behind it; a short flash marks the
//! landing, then it rests and docks like Halqa.
//!
//! | Phase | Time (of `travel` = 1.05 s) | What happens |
//! |---|---|---|
//! | Streak | 0 → 55 % | Quadratic path from off-screen, entering along the throw (the angle tilts it along the edge); quartic ease-out, so it arrives fast and brakes. Scale 0.22 → 1. |
//! | Flash | 55 → 55 % + 0.35 s | A soft sand-coloured bloom around the card; the tail and sparks fade over 0.30 s. |
//! | Hold, Dock | as Halqa | Rests, then flies to the notification corner. |
//!
//! fx slots (mirrored in `kometa.wgsl`):
//! - `fx[0]`: head x, y, head radius px, unused
//! - `fx[1..=4]`: tail points, newest first: x, y, half width px, unused
//! - `fx[5]`: tail alpha, spark spread px, spark flow phase, landing flash
//! - `fx[6]`: travel direction x, y (unit), spark count, unused

use vutric_core::SendZone;

use super::math::{self, Rect, SHADOW_PAD};
use super::scene::{FX_LEN, Kind, Scene};
use super::{Pose, Setup};
use crate::timeline::{Vec2, ease_in_cubic, ease_in_out_cubic, ease_out_cubic, phase, spring};

/// Tail points behind the head.
const TAIL: usize = 4;
/// Sparks drawn per pixel at most; the shader loops this many times inside the tail rectangle.
const SPARKS: f32 = 28.0;

/// One Kometa fling laid out on a screen.
#[derive(Clone, Debug)]
pub struct Kometa {
    screen: Vec2,
    start: Vec2,
    control: Vec2,
    land: Vec2,
    dock_to: Vec2,
    entry: Vec2,
    half: Vec2,
    tilt: f32,
    energy: f32,
    fly: f32,
    lag: f32,
    travel: f32,
    hold: f32,
    dock: f32,
}

impl Kometa {
    #[must_use]
    pub fn new(setup: &Setup) -> Self {
        let [w, h] = setup.screen;
        let short = w.min(h);
        let travel = math::finite_or(setup.cfg.travel, 1.05).max(0.2);
        let edge = math::edge(setup.zone, setup.screen, setup.gesture.exit_x);
        let energy = (math::finite_or(setup.gesture.speed, 0.0) / 3000.0).clamp(0.3, 1.0);
        let angle = math::finite_or(setup.gesture.angle, 0.0).clamp(-0.9, 0.9);

        // The comet enters along the throw: the phone's angle tilts the entry along the edge.
        let entry = normalize(
            math::add(edge.inward, math::scale(edge.along, angle * 0.8)),
            edge.inward,
        );
        let depth = match setup.zone {
            SendZone::Top => h * (0.26 + 0.14 * energy),
            SendZone::Left | SendZone::Right => w * (0.20 + 0.12 * energy),
        };
        let land = math::add(
            math::add(edge.point, math::scale(edge.inward, depth)),
            math::scale(edge.along, angle * short * 0.08),
        );
        Self {
            screen: setup.screen,
            start: math::add(edge.point, math::scale(entry, -short * 0.30)),
            control: math::add(edge.point, math::scale(entry, depth * 0.55)),
            land,
            dock_to: [w - short * 0.20, h - short * 0.12],
            entry,
            half: math::card_half(short * 0.34, setup.image_aspect),
            tilt: angle * 0.45,
            energy,
            fly: 0.55 * travel,
            lag: 0.05 * travel / 1.05,
            travel,
            hold: math::finite_or(setup.cfg.hold, 1.4).max(0.0),
            dock: math::finite_or(setup.cfg.dock, 0.45).max(0.05),
        }
    }

    /// When the head stops at its resting place.
    #[must_use]
    pub fn landed_at(&self) -> f32 {
        self.fly
    }

    #[must_use]
    pub fn duration(&self) -> f32 {
        self.travel + self.hold + self.dock
    }

    #[must_use]
    pub fn sample(&self, t: f32) -> Scene {
        let t = t.max(0.0);
        let card = self.card_at(t);

        let mut points = [card.center; TAIL + 1];
        for (k, p) in points.iter_mut().enumerate().skip(1) {
            *p = self.path((t - self.lag * k as f32).max(0.0));
        }
        let head_r = card.half[0].min(card.half[1]) * card.scale * 0.9;
        let arrived = phase(t, self.fly, self.fly + 0.30);
        let tail_alpha = if self.docking(t) > 0.0 {
            0.0
        } else {
            (1.0 - ease_in_cubic(arrived)) * card.opacity * (0.6 + 0.4 * self.energy)
        };
        let spread = head_r * 0.9 * (0.5 + 0.5 * self.energy);
        let flash = if t >= self.fly && self.docking(t) <= 0.0 {
            (1.0 - ease_out_cubic(phase(t, self.fly, self.fly + 0.35))) * self.energy
        } else {
            0.0
        };
        let dir = normalize(sub(points[0], points[1]), self.entry);

        let mut fx = [[0.0; 4]; FX_LEN];
        fx[0] = [points[0][0], points[0][1], head_r, 0.0];
        for (k, (slot, p)) in fx.iter_mut().zip(points).enumerate().skip(1) {
            let width = head_r * 0.55 * (1.0 - k as f32 / (TAIL as f32 + 1.0));
            *slot = [p[0], p[1], width, 0.0];
        }
        fx[5] = [tail_alpha, spread, t * 1.6, flash];
        fx[6] = [dir[0], dir[1], SPARKS, 0.0];

        let mut tail: Rect = math::EMPTY;
        if tail_alpha > 0.0 {
            for (k, p) in points.iter().enumerate() {
                let reach = fx[k][2].max(head_r * 0.6) * 3.0 + spread * 1.3 + 8.0;
                tail = math::union(tail, math::around(*p, reach));
            }
        }
        if flash > 0.0 {
            tail = math::union(tail, math::around(points[0], head_r * 4.0));
        }
        Scene {
            kind: Kind::Kometa,
            card,
            fx,
            regions: [
                math::card_region(&card, SHADOW_PAD, self.screen),
                math::clamp_rect(tail, self.screen),
                math::EMPTY,
            ],
        }
    }

    fn docking(&self, t: f32) -> f32 {
        let start = self.travel + self.hold;
        ease_in_out_cubic(phase(t, start, start + self.dock))
    }

    /// The head's position along the streak (before docking).
    fn path(&self, t: f32) -> Vec2 {
        let s = ease_out_quart(phase(t, 0.0, self.fly));
        let a = math::lerp2(self.start, self.control, s);
        let b = math::lerp2(self.control, self.land, s);
        math::lerp2(a, b, s)
    }

    fn card_at(&self, t: f32) -> Pose {
        let flying = phase(t, 0.0, self.fly);
        let s = ease_out_quart(flying);
        let mut center = self.path(t);
        let since = (t - self.fly).max(0.0);
        // A small bounce after the brake, ramped in over 50 ms so the scale never jumps.
        let bump = if flying >= 1.0 {
            0.04 * self.energy * spring(since, 3.0, 0.45) * phase(since, 0.0, 0.05)
        } else {
            0.0
        };
        let mut scale = math::lerp(0.22, 1.0, ease_out_cubic(flying)) * (1.0 + bump);
        let mut rotation = self.tilt * (1.0 - s);
        let mut opacity = ease_out_cubic(phase(flying, 0.0, 0.18));

        let d = self.docking(t);
        if d > 0.0 {
            center = math::lerp2(self.land, self.dock_to, d);
            scale *= math::lerp(1.0, 0.22, d);
            rotation *= 1.0 - d;
            opacity *= 1.0 - ease_in_cubic(d);
        }
        if flying <= 0.0 {
            opacity = 0.0;
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

fn ease_out_quart(x: f32) -> f32 {
    1.0 - (1.0 - x).powi(4)
}

fn sub(a: Vec2, b: Vec2) -> Vec2 {
    [a[0] - b[0], a[1] - b[1]]
}

/// `v` scaled to length 1, or `fallback` when it is too short to have a direction.
fn normalize(v: Vec2, fallback: Vec2) -> Vec2 {
    let len = v[0].hypot(v[1]);
    if len > 1e-3 && len.is_finite() {
        [v[0] / len, v[1] / len]
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{SCREEN, every_zone, setup};
    use super::*;

    #[test]
    fn enters_from_beyond_the_edge_and_brakes() {
        let k = Kometa::new(&setup(SendZone::Right));
        let start = k.path(0.0);
        assert!(start[0] > SCREEN[0], "starts off-screen: {start:?}");
        let early = k.path(k.fly * 0.2)[0] - k.path(k.fly * 0.1)[0];
        let late = k.path(k.fly * 0.9)[0] - k.path(k.fly * 0.8)[0];
        assert!(
            early.abs() > late.abs() * 3.0,
            "decelerates: {early} vs {late}"
        );
    }

    #[test]
    fn tail_follows_the_head_then_disappears() {
        for zone in every_zone() {
            let k = Kometa::new(&setup(zone));
            let mid = k.sample(k.fly * 0.5);
            assert!(mid.fx[5][0] > 0.0, "{zone:?}: tail lit in flight");
            assert!(mid.regions[1] != [0.0; 4], "{zone:?}: tail region");
            let rest = k.sample(k.fly + 0.5);
            assert_eq!(rest.fx[5][0], 0.0, "{zone:?}: tail gone after landing");
            assert_eq!(rest.fx[5][3], 0.0, "{zone:?}: flash over");
            assert_eq!(rest.regions[1], [0.0; 4]);
        }
    }

    #[test]
    fn direction_is_a_unit_vector() {
        let k = Kometa::new(&setup(SendZone::Top));
        for i in 0..50 {
            let s = k.sample(i as f32 * 0.02);
            let [x, y] = [s.fx[6][0], s.fx[6][1]];
            assert!((x.hypot(y) - 1.0).abs() < 1e-3, "t = {}", i as f32 * 0.02);
        }
    }
}
