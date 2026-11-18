//! Which `AppKit` window settings make the overlay (ADR-0110). Pure values, so they are checked on every CI OS; the
//! macOS tests in `imp` compare them with the SDK's own constants.

use std::fmt;
use std::str::FromStr;

/// How high the overlay sits. Resolved through `CGWindowLevelForKey` on macOS, never hard-coded there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayLevel {
    /// Above normal windows only. Under the menu bar and the Dock, so a throw from the screen edge is hidden.
    Floating,
    /// Above the menu bar, the Dock and full-screen apps; below open menus, drags, the screen saver and the lock
    /// screen. The default.
    #[default]
    Status,
    /// Above open menus too.
    PopUpMenu,
    /// Above almost everything, the screen saver included. The roadmap's first guess; kept for measurement only.
    ScreenSaver,
}

impl OverlayLevel {
    pub const ALL: [Self; 4] = [
        Self::Floating,
        Self::Status,
        Self::PopUpMenu,
        Self::ScreenSaver,
    ];

    /// `CGWindowLevelKey` raw value from `CGWindowLevel.h`.
    #[must_use]
    pub const fn cg_key(self) -> i32 {
        match self {
            Self::Floating => 5,
            Self::Status => 9,
            Self::PopUpMenu => 11,
            Self::ScreenSaver => 13,
        }
    }

    /// The level `CGWindowLevelForKey` returns for [`Self::cg_key`] on every macOS release so far. Used only
    /// for reports and tests; the real panel asks the system.
    #[must_use]
    pub const fn documented_level(self) -> isize {
        match self {
            Self::Floating => 3,
            Self::Status => 25,
            Self::PopUpMenu => 101,
            Self::ScreenSaver => 1000,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Floating => "floating",
            Self::Status => "status",
            Self::PopUpMenu => "popup-menu",
            Self::ScreenSaver => "screen-saver",
        }
    }
}

impl fmt::Display for OverlayLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for OverlayLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|l| l.name() == s)
            .ok_or_else(|| {
                format!("unknown level {s:?}; one of floating, status, popup-menu, screen-saver")
            })
    }
}

/// `NSWindowStyleMask` bits.
pub mod style {
    pub const BORDERLESS: usize = 0;
    /// Only meaningful on `NSPanel`: clicking or ordering it front never activates the app.
    pub const NONACTIVATING_PANEL: usize = 1 << 7;
}

/// `NSWindowCollectionBehavior` bits.
pub mod behavior {
    /// On every Space, not only the one it was created on.
    pub const CAN_JOIN_ALL_SPACES: usize = 1 << 0;
    /// Does not move or fade with Mission Control and Exposé.
    pub const STATIONARY: usize = 1 << 4;
    /// Not part of the Command-backtick window cycling.
    pub const IGNORES_CYCLE: usize = 1 << 6;
    /// May share a Space with another app's full-screen window.
    pub const FULL_SCREEN_AUXILIARY: usize = 1 << 8;
    /// macOS 13+: may be shown over other apps' full-screen Spaces. Ignored by older releases.
    pub const CAN_JOIN_ALL_APPLICATIONS: usize = 1 << 18;

    /// Everything the overlay sets.
    pub const OVERLAY: usize = CAN_JOIN_ALL_SPACES
        | STATIONARY
        | IGNORES_CYCLE
        | FULL_SCREEN_AUXILIARY
        | CAN_JOIN_ALL_APPLICATIONS;
}

/// `NSWindowAnimationBehaviorNone`: ordering the panel in or out never animates, so showing it cannot flash.
pub const ANIMATION_NONE: isize = 2;

/// Frame-time limit above which one refresh of the display counts as missed. Half a refresh of slack absorbs timer
/// jitter, the same idea as the Windows spike's 20 ms at 60 Hz.
#[must_use]
pub fn hitch_limit_ms(refresh_hz: f64) -> f64 {
    1.5 * 1000.0 / refresh_hz.max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_round_trip_by_name() {
        for l in OverlayLevel::ALL {
            assert_eq!(l.name().parse::<OverlayLevel>(), Ok(l));
        }
        assert!("top".parse::<OverlayLevel>().is_err());
        assert_eq!(OverlayLevel::default(), OverlayLevel::Status);
    }

    #[test]
    fn levels_are_ordered() {
        let levels: Vec<isize> = OverlayLevel::ALL
            .iter()
            .map(|l| l.documented_level())
            .collect();
        assert!(levels.windows(2).all(|w| w[0] < w[1]), "{levels:?}");
        // Above the menu bar (24) and the Dock (20), below dragging (500).
        assert!(OverlayLevel::Status.documented_level() > 24);
        assert!(OverlayLevel::Status.documented_level() < 500);
    }

    #[test]
    fn overlay_behavior_has_every_space_and_full_screen() {
        assert_eq!(behavior::OVERLAY, 0x4_0151);
        assert_eq!(style::NONACTIVATING_PANEL, 0x80);
    }

    #[test]
    fn hitch_limits() {
        assert!((hitch_limit_ms(60.0) - 25.0).abs() < 1e-9);
        assert!((hitch_limit_ms(120.0) - 12.5).abs() < 1e-9);
        assert!(hitch_limit_ms(0.0).is_finite());
    }
}
