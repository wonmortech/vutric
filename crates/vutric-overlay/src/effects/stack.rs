//! Many files (E5): the first item is thrown with the chosen effect. When it lands, the rest appear stacked
//! behind it, up to three blank cards fanned out a little, with a counter badge on its top-right corner showing
//! how many files arrived. The stack follows the top card's [`Pose`], so it rests and docks with it.
//!
//! It is an overlay layer: blended over whatever the main effect drew, and hidden where the top card covers it.
//!
//! fx slots (mirrored in `stack.wgsl`):
//! - `fx[0]`: files, cards behind (0..=3), appear (0..1, overshoots), opacity
//! - `fx[1]`: badge x, y, radius px, unused
//! - `fx[2]`: offset per card behind x, y px (screen space), rotation per card behind rad, unused

use super::Pose;
use super::math::{self, Rect};
use super::scene::{FX_LEN, Kind, Scene};
use crate::timeline::{Vec2, ease_out_back, ease_out_cubic, phase};

/// Blank cards drawn behind the top one at most.
pub const MAX_BEHIND: u32 = 3;
/// The badge shows at most two digits.
pub const MAX_SHOWN: u32 = 99;

const APPEAR: f32 = 0.32;
/// Offset of each card behind, in top-card pixels at scale 1, before rotation.
const STEP: Vec2 = [10.0, 9.0];
const TURN: f32 = 0.045;

/// The stack of a fling of `files` items.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stack {
    files: u32,
    appear_at: f32,
    screen: Vec2,
}

impl Stack {
    /// `None` for a single file. `appear_at` is when the top card has landed.
    #[must_use]
    pub fn new(files: u32, appear_at: f32, screen: Vec2) -> Option<Self> {
        (files > 1).then_some(Self {
            files,
            appear_at: math::finite_or(appear_at, 0.0).max(0.0),
            screen,
        })
    }

    #[must_use]
    pub fn files(&self) -> u32 {
        self.files
    }

    #[must_use]
    pub fn sample(&self, t: f32, top: &Pose) -> Scene {
        let rise = phase(t, self.appear_at, self.appear_at + APPEAR);
        let appear = ease_out_back(rise, 1.4);
        let opacity = (ease_out_cubic(rise) * top.opacity).clamp(0.0, 1.0);
        let behind = (self.files - 1).min(MAX_BEHIND);
        let (sin, cos) = top.rotation.sin_cos();
        let rotate = |v: Vec2| [cos * v[0] - sin * v[1], sin * v[0] + cos * v[1]];

        let step = rotate(math::scale(STEP, top.scale));
        let corner = rotate(math::scale([top.half[0], -top.half[1]], top.scale));
        let badge = math::add(top.center, corner);
        let badge_r = (top.half[0].min(top.half[1]) * 0.16).clamp(14.0, 40.0) * top.scale;

        let mut fx = [[0.0; 4]; FX_LEN];
        fx[0] = [
            self.files.min(MAX_SHOWN) as f32,
            behind as f32,
            appear,
            opacity,
        ];
        fx[1] = [badge[0], badge[1], badge_r, 0.0];
        fx[2] = [step[0], step[1], TURN, 0.0];

        let mut lit: Rect = math::EMPTY;
        if opacity > 0.0 {
            let reach = top.half[0].hypot(top.half[1]) * top.scale + 36.0;
            for k in 1..=behind {
                let behind_center =
                    math::add(top.center, math::scale(step, k as f32 * appear.max(0.0)));
                lit = math::union(lit, math::around(behind_center, reach));
            }
            // The badge overshoots to 1.4x while appearing.
            lit = math::union(lit, math::around(badge, badge_r * 1.5 + 4.0));
        }
        Scene {
            kind: Kind::Stack,
            card: *top,
            fx,
            regions: [math::clamp_rect(lit, self.screen), math::EMPTY, math::EMPTY],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::SCREEN;
    use super::*;

    fn top() -> Pose {
        Pose {
            center: [1800.0, 700.0],
            half: [240.0, 180.0],
            scale: 1.0,
            rotation: 0.1,
            opacity: 1.0,
        }
    }

    #[test]
    fn one_file_has_no_stack() {
        assert!(Stack::new(1, 0.6, SCREEN).is_none());
        assert!(Stack::new(0, 0.6, SCREEN).is_none());
    }

    #[test]
    fn appears_after_landing_and_caps_what_it_shows() {
        let s = Stack::new(250, 0.6, SCREEN).expect("many files");
        let before = s.sample(0.5, &top());
        assert_eq!(before.fx[0][3], 0.0);
        assert_eq!(before.regions[0], [0.0; 4]);
        let shown = s.sample(1.2, &top());
        assert_eq!(shown.fx[0][0], 99.0, "badge caps at 99");
        assert_eq!(shown.fx[0][1], 3.0, "at most three cards behind");
        assert!(shown.fx[0][3] > 0.99);
        let b = shown.regions[0];
        let [bx, by] = [shown.fx[1][0], shown.fx[1][1]];
        assert!(
            b[0] < bx && bx < b[2] && b[1] < by && by < b[3],
            "badge inside the lit region"
        );
    }

    #[test]
    fn two_files_show_one_card_behind() {
        let s = Stack::new(2, 0.0, SCREEN).expect("two files");
        let f = s.sample(1.0, &top());
        assert_eq!(f.fx[0][..2], [2.0, 1.0]);
        assert!(f.kind.is_overlay());
    }

    #[test]
    fn fades_with_the_top_card() {
        let s = Stack::new(5, 0.0, SCREEN).expect("five files");
        let fading = Pose {
            opacity: 0.25,
            ..top()
        };
        assert!((s.sample(2.0, &fading).fx[0][3] - 0.25).abs() < 1e-4);
    }
}
