use serde::{Deserialize, Serialize};

/// The desktop effect a fling lands with. The phone suggests one; the desktop setting wins unless set to "follow
/// the phone".
///
/// Wire values are explicit and stable: never renumber, only append.
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum EffectId {
    /// A ring opens at the Send Zone edge; the item flies through it with a short light trail.
    Halqa = 1,
    /// The item streaks in as a comet with a particle tail.
    Kometa = 2,
    /// The item arrives inside a soft bubble that pops on landing.
    Pufak = 3,
    /// A plain, fast slide-in for people who want no show.
    Oddiy = 4,
}

impl EffectId {
    pub const ALL: [Self; 4] = [Self::Halqa, Self::Kometa, Self::Pufak, Self::Oddiy];

    /// Stable key used in settings files and i18n (`effect.<key>.title`).
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Halqa => "halqa",
            Self::Kometa => "kometa",
            Self::Pufak => "pufak",
            Self::Oddiy => "oddiy",
        }
    }

    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.key() == key)
    }
}

/// The screen edge where flung items enter the desktop.
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SendZone {
    Left,
    #[default]
    Right,
    Top,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_round_trip() {
        for effect in EffectId::ALL {
            assert_eq!(EffectId::from_key(effect.key()), Some(effect));
        }
        assert_eq!(EffectId::from_key("rainbow"), None);
    }
}
