//! Small vector and rectangle helpers shared by the effect timelines. Rectangles are
//! `[min_x, min_y, max_x, max_y]` in physical pixels; an all-zero rectangle is empty.

use vutric_core::SendZone;

use super::Pose;
use crate::timeline::Vec2;

pub(super) type Rect = [f32; 4];
pub(super) const EMPTY: Rect = [0.0; 4];

/// The card's drop shadow reaches 22 px down plus a 46 px blur (same numbers as Halqa).
pub(super) const SHADOW_PAD: f32 = 72.0;

pub(super) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub(super) fn lerp2(a: Vec2, b: Vec2, t: f32) -> Vec2 {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t)]
}

pub(super) fn add(a: Vec2, b: Vec2) -> Vec2 {
    [a[0] + b[0], a[1] + b[1]]
}

pub(super) fn scale(a: Vec2, s: f32) -> Vec2 {
    [a[0] * s, a[1] * s]
}

pub(super) fn is_empty(r: Rect) -> bool {
    r[0] >= r[2] || r[1] >= r[3]
}

pub(super) fn union(a: Rect, b: Rect) -> Rect {
    match (is_empty(a), is_empty(b)) {
        (true, true) => EMPTY,
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

/// Clamps to the screen; anything that ends up with no area becomes [`EMPTY`].
pub(super) fn clamp_rect(b: Rect, screen: Vec2) -> Rect {
    let r = [
        b[0].clamp(0.0, screen[0]),
        b[1].clamp(0.0, screen[1]),
        b[2].clamp(0.0, screen[0]),
        b[3].clamp(0.0, screen[1]),
    ];
    if is_empty(r) || r.iter().any(|v| !v.is_finite()) {
        EMPTY
    } else {
        r
    }
}

/// A square around a circle of radius `r`.
pub(super) fn around(c: Vec2, r: f32) -> Rect {
    [c[0] - r, c[1] - r, c[0] + r, c[1] + r]
}

/// Rotation-safe rectangle around the card plus `pad` (shadow, glow), clamped to the screen.
pub(super) fn card_region(p: &Pose, pad: f32, screen: Vec2) -> Rect {
    if p.opacity <= 0.0 {
        return EMPTY;
    }
    let r = p.half[0].hypot(p.half[1]) * p.scale + pad;
    clamp_rect(around(p.center, r), screen)
}

/// Half extents of a card whose longest side is `longest`, keeping the item's aspect (width / height).
pub(super) fn card_half(longest: f32, image_aspect: f32) -> Vec2 {
    let aspect = if image_aspect.is_finite() && image_aspect > 0.0 {
        image_aspect.clamp(0.4, 2.5)
    } else {
        1.0
    };
    if aspect >= 1.0 {
        [longest * 0.5, longest * 0.5 / aspect]
    } else {
        [longest * 0.5 * aspect, longest * 0.5]
    }
}

/// Where a fling enters: the point on the Send Zone edge (placed like Halqa's portal), the unit vector pointing
/// into the screen and the unit vector along the edge.
#[derive(Clone, Copy, Debug)]
pub(super) struct Edge {
    pub point: Vec2,
    pub inward: Vec2,
    pub along: Vec2,
}

pub(super) fn edge(zone: SendZone, screen: Vec2, exit_x: f32) -> Edge {
    let [w, h] = screen;
    let x = if exit_x.is_finite() {
        exit_x.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let at = 0.30 + 0.40 * x;
    let (point, inward, along) = match zone {
        SendZone::Right => ([w, h * at], [-1.0, 0.0], [0.0, 1.0]),
        SendZone::Left => ([0.0, h * at], [1.0, 0.0], [0.0, 1.0]),
        SendZone::Top => ([w * at, 0.0], [0.0, 1.0], [1.0, 0.0]),
    };
    Edge {
        point,
        inward,
        along,
    }
}

/// A finite value or `fallback`.
pub(super) fn finite_or(v: f32, fallback: f32) -> f32 {
    if v.is_finite() { v } else { fallback }
}
