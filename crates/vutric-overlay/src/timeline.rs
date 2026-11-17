//! Pure effect math. Given a time, produce everything the shader needs for one frame.
//!
//! Nothing here touches the GPU, so every curve is unit-tested and the same numbers drive the renderer, the
//! settings preview and (later) the Flutter fallback animation.

use vutric_core::{Gesture, SendZone};

pub type Vec2 = [f32; 2];

/// Tunables for the Halqa effect. Times in seconds, lengths in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HalqaConfig {
    /// Portal open → card landed → portal closed.
    pub travel: f32,
    /// How long the landed card stays in place before it docks.
    pub hold: f32,
    /// Card flying to the notification corner and fading out.
    pub dock: f32,
}

impl Default for HalqaConfig {
    fn default() -> Self {
        Self {
            travel: 1.05,
            hold: 1.4,
            dock: 0.45,
        }
    }
}

/// Everything the renderer needs for one frame of one fling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub ring_center: Vec2,
    pub ring_radius: f32,
    pub ring_opacity: f32,
    /// Horizontal squash of the ring (1 = circle). The portal is seen edge-on, so it is an ellipse.
    pub ring_aspect: f32,
    /// Shimmer phase for the ring gradient, radians.
    pub ring_phase: f32,
    pub card_center: Vec2,
    /// Half extents of the card at scale 1.
    pub card_half: Vec2,
    pub card_scale: f32,
    pub card_rotation: f32,
    pub card_opacity: f32,
    /// Ghost positions behind the card: `[x, y, alpha, scale]`, newest first.
    pub trail: [[f32; 4]; TRAIL_LEN],
    /// Screen-space rectangle that contains every lit pixel: `[min_x, min_y, max_x, max_y]`.
    pub bounds: [f32; 4],
    /// Tighter, possibly overlapping rectangles (portal, card + trail) whose union covers every lit pixel.
    /// The renderer shades only these; an all-zero rectangle is empty.
    pub regions: [[f32; 4]; 2],
}

pub const TRAIL_LEN: usize = 4;
const TRAIL_STEP: f32 = 0.026;

/// One Halqa fling laid out on a screen. Construct once per fling, then [`Self::sample`] every frame.
#[derive(Clone, Debug)]
pub struct Halqa {
    cfg: HalqaConfig,
    screen: Vec2,
    zone: SendZone,
    ring_center: Vec2,
    ring_radius: f32,
    start: Vec2,
    control_a: Vec2,
    control_b: Vec2,
    land: Vec2,
    dock_to: Vec2,
    card_half: Vec2,
    tilt: f32,
    energy: f32,
}

impl Halqa {
    /// `image_aspect` is width / height of the flung item; the card keeps it.
    #[must_use]
    pub fn new(
        cfg: HalqaConfig,
        screen: Vec2,
        zone: SendZone,
        gesture: Gesture,
        image_aspect: f32,
    ) -> Self {
        let [w, h] = screen;
        let short = w.min(h);
        let along = 0.30 + 0.40 * gesture.exit_x.clamp(0.0, 1.0);
        let (ring_center, inward, perp) = match zone {
            SendZone::Right => ([w, h * along], [-1.0, 0.0], [0.0, 1.0]),
            SendZone::Left => ([0.0, h * along], [1.0, 0.0], [0.0, 1.0]),
            SendZone::Top => ([w * along, 0.0], [0.0, 1.0], [1.0, 0.0]),
        };
        let ring_radius = short * 0.17;

        let longest = short * 0.34;
        let aspect = if image_aspect.is_finite() && image_aspect > 0.0 {
            image_aspect.clamp(0.4, 2.5)
        } else {
            1.0
        };
        let card_half = if aspect >= 1.0 {
            [longest * 0.5, longest * 0.5 / aspect]
        } else {
            [longest * 0.5 * aspect, longest * 0.5]
        };

        // A hard flick lands deeper and swings more; a soft one barely clears the portal.
        let energy = (gesture.speed / 3000.0).clamp(0.25, 1.0);
        let depth = match zone {
            SendZone::Top => h * (0.26 + 0.14 * energy),
            _ => w * (0.20 + 0.12 * energy),
        };
        let swing = gesture.angle.clamp(-0.9, 0.9) * short * 0.22;
        let land = add(
            add(ring_center, scale(inward, depth)),
            scale(perp, swing * 0.35),
        );
        let start = add(
            ring_center,
            scale(inward, -card_half[0].max(card_half[1]) * 0.6),
        );
        let control_a = add(
            add(ring_center, scale(inward, depth * 0.35)),
            scale(perp, swing),
        );
        let control_b = add(land, scale(inward, -depth * 0.12));

        Self {
            cfg,
            screen,
            zone,
            ring_center,
            ring_radius,
            start,
            control_a,
            control_b,
            land,
            dock_to: [w - short * 0.20, h - short * 0.12],
            card_half,
            tilt: gesture.angle.clamp(-0.9, 0.9) * 0.55,
            energy,
        }
    }

    #[must_use]
    pub fn duration(&self) -> f32 {
        self.cfg.travel + self.cfg.hold + self.cfg.dock
    }

    #[must_use]
    pub fn is_finished(&self, t: f32) -> bool {
        t >= self.duration()
    }

    #[must_use]
    pub fn sample(&self, t: f32) -> Frame {
        let t = t.max(0.0);
        let travel = self.cfg.travel;
        let u = t / travel;

        let (ring_radius, ring_opacity) = self.ring_at(u);
        let (card_center, card_scale, card_rotation, card_opacity) = self.card_at(t);

        let mut trail = [[0.0; 4]; TRAIL_LEN];
        let flying = phase(u, 0.10, 0.62);
        if flying > 0.0 && flying < 1.0 {
            for (i, ghost) in trail.iter_mut().enumerate() {
                let (pos, s, _, _) = self.card_at(t - TRAIL_STEP * (i as f32 + 1.0));
                let fade = 1.0 - i as f32 / TRAIL_LEN as f32;
                let speed_fade = (1.0 - flying).powf(0.6) * self.energy;
                *ghost = [pos[0], pos[1], 0.42 * fade * speed_fade * card_opacity, s];
            }
        }

        let mut frame = Frame {
            ring_center: self.ring_center,
            ring_radius,
            ring_opacity,
            ring_aspect: if self.zone == SendZone::Top {
                1.0
            } else {
                0.42
            },
            ring_phase: t * 2.4,
            card_center,
            card_half: self.card_half,
            card_scale,
            card_rotation,
            card_opacity,
            trail,
            bounds: [0.0; 4],
            regions: [[0.0; 4]; 2],
        };
        frame.regions = [self.ring_bounds(&frame), self.card_bounds(&frame)];
        frame.bounds = union(frame.regions[0], frame.regions[1]);
        frame
    }

    fn ring_at(&self, u: f32) -> (f32, f32) {
        let open = phase(u, 0.0, 0.24);
        let close = phase(u, 0.60, 0.90);
        let radius =
            self.ring_radius * (ease_out_back(open, 1.4) * (1.0 - 0.35 * ease_in_cubic(close)));
        let opacity = ease_out_cubic(open) * (1.0 - ease_in_out_cubic(close));
        (radius.max(0.0), opacity.clamp(0.0, 1.0))
    }

    fn card_at(&self, t: f32) -> (Vec2, f32, f32, f32) {
        let travel = self.cfg.travel;
        let u = t / travel;
        let fly = phase(u, 0.10, 0.62);
        let s = ease_out_cubic(fly);
        let mut pos = cubic_bezier(self.start, self.control_a, self.control_b, self.land, s);

        let since_land = (t - 0.62 * travel).max(0.0);
        let settle = if fly >= 1.0 {
            spring(since_land, 3.2, 0.42)
        } else {
            1.0
        };
        let mut scale =
            lerp(0.18, 1.0, ease_out_back(fly, 0.9)) * (1.0 + 0.035 * settle * self.energy);
        let mut rotation = self.tilt * (1.0 - s) + self.tilt * 0.35 * settle;
        let mut opacity = ease_out_cubic(phase(fly, 0.0, 0.22));

        let dock_start = travel + self.cfg.hold;
        let d = ease_in_out_cubic(phase(t, dock_start, dock_start + self.cfg.dock));
        if d > 0.0 {
            pos = lerp2(self.land, self.dock_to, d);
            scale *= lerp(1.0, 0.22, d);
            rotation *= 1.0 - d;
            opacity *= 1.0 - ease_in_cubic(d);
        }
        if fly <= 0.0 {
            opacity = 0.0;
        }
        (pos, scale, rotation, opacity.clamp(0.0, 1.0))
    }

    fn ring_bounds(&self, f: &Frame) -> [f32; 4] {
        if f.ring_opacity <= 0.0 {
            return [0.0; 4];
        }
        // The glow is exp(-d / (0.10 r)); at 0.55 r it is below 0.5 % and invisible.
        let glow = self.ring_radius * 0.55;
        let c = f.ring_center;
        let (rx, ry) = (f.ring_radius * f.ring_aspect + glow, f.ring_radius + glow);
        self.clamp_rect([c[0] - rx, c[1] - ry, c[0] + rx, c[1] + ry])
    }

    fn card_bounds(&self, f: &Frame) -> [f32; 4] {
        if f.card_opacity <= 0.0 {
            return [0.0; 4];
        }
        let diag = f.card_half[0].hypot(f.card_half[1]);
        // Rotation-safe radius plus the drop shadow (offset 22 px, blur 46 px).
        let r = diag * f.card_scale + 72.0;
        let c = f.card_center;
        let mut b = [c[0] - r, c[1] - r, c[0] + r, c[1] + r];
        for g in f.trail.iter().filter(|g| g[2] > 0.0) {
            // Trail edge glow falls off as exp(-d / 9 px); 60 px past the ghost edge is invisible.
            let gr = diag * g[3] + 60.0;
            b = union(b, [g[0] - gr, g[1] - gr, g[0] + gr, g[1] + gr]);
        }
        self.clamp_rect(b)
    }

    fn clamp_rect(&self, b: [f32; 4]) -> [f32; 4] {
        let r = [
            b[0].clamp(0.0, self.screen[0]),
            b[1].clamp(0.0, self.screen[1]),
            b[2].clamp(0.0, self.screen[0]),
            b[3].clamp(0.0, self.screen[1]),
        ];
        if r[0] >= r[2] || r[1] >= r[3] {
            [0.0; 4]
        } else {
            r
        }
    }
}

fn is_empty(r: [f32; 4]) -> bool {
    r[0] >= r[2] || r[1] >= r[3]
}

fn union(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    match (is_empty(a), is_empty(b)) {
        (true, true) => [0.0; 4],
        (true, false) => b,
        (false, true) => a,
        (false, false) => [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ],
    }
}

/// Maps `x` from `[from, to]` onto `[0, 1]`, clamped.
#[must_use]
pub fn phase(x: f32, from: f32, to: f32) -> f32 {
    ((x - from) / (to - from)).clamp(0.0, 1.0)
}

#[must_use]
pub fn ease_out_cubic(x: f32) -> f32 {
    1.0 - (1.0 - x).powi(3)
}

#[must_use]
pub fn ease_in_cubic(x: f32) -> f32 {
    x * x * x
}

#[must_use]
pub fn ease_in_out_cubic(x: f32) -> f32 {
    if x < 0.5 {
        4.0 * x * x * x
    } else {
        1.0 - (-2.0 * x + 2.0).powi(3) / 2.0
    }
}

/// Overshoots by an amount controlled by `k` (≈ 1.70 is the classic value), then settles at 1.
#[must_use]
pub fn ease_out_back(x: f32, k: f32) -> f32 {
    let x = x - 1.0;
    1.0 + (k + 1.0) * x * x * x + k * x * x
}

/// Underdamped spring displacement, starting at 1 and decaying to 0.
#[must_use]
pub fn spring(t: f32, freq_hz: f32, damping: f32) -> f32 {
    let omega = core::f32::consts::TAU * freq_hz;
    let damped = omega * (1.0 - damping * damping).max(0.0).sqrt();
    (-damping * omega * t).exp() * (damped * t).cos()
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp2(a: Vec2, b: Vec2, t: f32) -> Vec2 {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t)]
}

fn add(a: Vec2, b: Vec2) -> Vec2 {
    [a[0] + b[0], a[1] + b[1]]
}

fn scale(a: Vec2, s: f32) -> Vec2 {
    [a[0] * s, a[1] * s]
}

fn cubic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    let inv = 1.0 - t;
    let weights = [
        inv * inv * inv,
        3.0 * inv * inv * t,
        3.0 * inv * t * t,
        t * t * t,
    ];
    let points = [p0, p1, p2, p3];
    let mut out = [0.0; 2];
    for (w, p) in weights.iter().zip(points) {
        out[0] += w * p[0];
        out[1] += w * p[1];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Vec2 = [2560.0, 1440.0];

    fn gesture(speed: f32, angle: f32, exit_x: f32) -> Gesture {
        Gesture {
            speed,
            angle,
            exit_x,
        }
    }

    fn halqa(zone: SendZone) -> Halqa {
        Halqa::new(
            HalqaConfig::default(),
            SCREEN,
            zone,
            gesture(2400.0, 0.3, 0.5),
            4.0 / 3.0,
        )
    }

    #[test]
    fn easing_endpoints() {
        for f in [ease_out_cubic, ease_in_cubic, ease_in_out_cubic] {
            assert!(f(0.0).abs() < 1e-6 && (f(1.0) - 1.0).abs() < 1e-6);
        }
        assert!((ease_out_back(1.0, 1.7) - 1.0).abs() < 1e-6);
        assert!(ease_out_back(0.8, 1.7) > 1.0, "back easing must overshoot");
        assert!((spring(0.0, 3.0, 0.4) - 1.0).abs() < 1e-6);
        assert!(spring(3.0, 3.0, 0.4).abs() < 1e-3);
    }

    #[test]
    fn card_is_invisible_before_it_leaves_the_portal() {
        let f = halqa(SendZone::Right).sample(0.0);
        assert_eq!(f.card_opacity, 0.0);
        assert_eq!(f.ring_opacity, 0.0);
    }

    #[test]
    fn card_lands_inside_the_screen_at_full_size() {
        for zone in [SendZone::Left, SendZone::Right, SendZone::Top] {
            let h = halqa(zone);
            let f = h.sample(HalqaConfig::default().travel + 0.5);
            assert!(
                (f.card_scale - 1.0).abs() < 0.02,
                "{zone:?}: scale {}",
                f.card_scale
            );
            assert!(f.card_opacity > 0.99);
            let [x, y] = f.card_center;
            assert!(
                x > 0.0 && x < SCREEN[0] && y > 0.0 && y < SCREEN[1],
                "{zone:?}: {x},{y}"
            );
            assert!(f.ring_opacity < 1e-3, "portal must be closed after landing");
        }
    }

    #[test]
    fn right_zone_travels_leftwards() {
        let h = halqa(SendZone::Right);
        let early = h.sample(0.25).card_center[0];
        let late = h.sample(0.6).card_center[0];
        assert!(late < early);
    }

    #[test]
    fn finishes_invisible() {
        let h = halqa(SendZone::Right);
        assert!(h.is_finished(h.duration()));
        assert!(h.sample(h.duration()).card_opacity < 1e-3);
    }

    #[test]
    fn every_frame_is_finite_and_bounded() {
        for zone in [SendZone::Left, SendZone::Right, SendZone::Top] {
            for g in [
                gesture(0.0, -3.0, 0.0),
                gesture(9000.0, 3.0, 1.0),
                gesture(1200.0, 0.0, 0.5),
            ] {
                for aspect in [0.0, 0.3, 1.0, 9.0, f32::NAN] {
                    let h = Halqa::new(HalqaConfig::default(), SCREEN, zone, g, aspect);
                    let steps = (h.duration() * 240.0) as usize;
                    for i in 0..=steps {
                        let f = h.sample(i as f32 / 240.0);
                        let values = [
                            f.ring_radius,
                            f.ring_opacity,
                            f.card_scale,
                            f.card_rotation,
                            f.card_opacity,
                            f.card_center[0],
                            f.card_center[1],
                        ];
                        assert!(
                            values.iter().all(|v| v.is_finite()),
                            "{zone:?} {g:?} {aspect}"
                        );
                        let b = f.bounds;
                        assert!(b[0] <= b[2] && b[1] <= b[3]);
                        assert!(b[2] <= SCREEN[0] && b[3] <= SCREEN[1]);
                    }
                }
            }
        }
    }

    #[test]
    fn bounds_cover_the_landed_card() {
        let frame = halqa(SendZone::Right).sample(1.6);
        let [cx, cy] = frame.card_center;
        let [hw, hh] = frame.card_half;
        let bounds = frame.bounds;
        assert!(bounds[0] < cx - hw && bounds[2] > cx + hw);
        assert!(bounds[1] < cy - hh && bounds[3] > cy + hh);
        let card = frame.regions[1];
        assert!(
            card[0] < cx - hw && card[2] > cx + hw,
            "card region must cover the card"
        );
    }
}
