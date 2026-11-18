//! The route picker and the "Avtomatik" order (backlog Rx, ADR-0007 part 1 and 3, ADR-0060).
//!
//! Pure data: which routes to try, in which order, for the user's choice and what the device can do right now,
//! and the badge a live session shows. Opening sockets, hotspots and cables is the host's job; the UI copy lives
//! behind the i18n keys returned here.

use serde::{Deserialize, Serialize};

/// The user's route choice (settings, "Yo'l").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Picker {
    /// "Avtomatik": same network, then internet direct, then our relay, then offer hotspot, Aware or cable.
    #[default]
    Auto,
    /// "Faqat shu tarmoq": the current local network only; nothing leaves it, no relay, no lookup.
    LanOnly,
    /// "Internet orqali": straight to the internet routes (direct, then relay), for two devices known to be on
    /// different networks; skips the LAN search.
    Internet,
    /// "Tarmoqsiz": no router and no internet: hotspot, Wi-Fi Aware or cable, offered to the user.
    Offline,
    /// "Bluetooth": finding and handshake over BLE, then a hotspot for the data (R4). Not built yet.
    Bluetooth,
}

/// Reads a [`Picker`] from settings, taking a value this version does not know (written by a newer one) as
/// [`Picker::Auto`], so the rest of the settings file still loads.
pub fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Picker, D::Error> {
    let s = String::deserialize(d)?;
    Ok(Picker::ALL
        .into_iter()
        .find(|p| p.key().strip_prefix("route.picker.") == Some(s.as_str()))
        .unwrap_or_default())
}

/// One thing to try, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Step {
    /// The current network: mDNS, beacon, the QR's address hints.
    SameNetwork,
    /// A hole-punched internet path, found by key.
    InternetDirect,
    /// Our end-to-end encrypted relay (n0 only as fallback, ADR-0053).
    Relay,
    /// Needs the user: open or join a hotspot, start Wi-Fi Aware, plug a cable.
    Offer(Offer),
}

/// A route the user has to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Offer {
    Hotspot,
    WifiAware,
    Cable,
}

/// What this device can do at the moment the plan is made. The host fills it from the OS.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // independent capabilities, read from the OS one by one
pub struct Situation {
    /// The device has a usable local network address (Wi-Fi, Ethernet, a hotspot it is on).
    pub lan: bool,
    /// The internet answers (a home relay was reached, or the OS says so).
    pub internet: bool,
    /// This device can open or join a hotspot (Android LOHS, Windows mobile hotspot, iOS join only).
    pub hotspot: bool,
    /// Wi-Fi Aware works here (Android 8+ with the feature, iOS 26+): R6.
    pub wifi_aware: bool,
    /// A cable route is possible (USB tethering, ADB, Ethernet between two computers): R5.
    pub cable: bool,
}

/// Why a plan is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoRoute {
    /// The chosen route is not built on this device yet (Bluetooth until R4).
    NotBuilt,
    /// The choice needs something the device lacks right now ("Faqat shu tarmoq" with no network, "Internet
    /// orqali" offline, "Tarmoqsiz" with no hotspot, Aware or cable).
    Unavailable,
}

impl NoRoute {
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::NotBuilt => "route.none.not_built",
            Self::Unavailable => "route.none.unavailable",
        }
    }
}

impl Picker {
    /// Every choice, in the order the picker lists them.
    pub const ALL: [Self; 5] = [
        Self::Auto,
        Self::LanOnly,
        Self::Internet,
        Self::Offline,
        Self::Bluetooth,
    ];

    /// i18n key of the picker row (`route.picker.<key>`).
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Auto => "route.picker.auto",
            Self::LanOnly => "route.picker.lan_only",
            Self::Internet => "route.picker.internet",
            Self::Offline => "route.picker.offline",
            Self::Bluetooth => "route.picker.bluetooth",
        }
    }

    /// Whether this choice lets anything leave the local network (relay, lookup by key). The endpoint is bound
    /// with internet reach only then; "Faqat shu tarmoq" also brings back the "Hech narsa internetga chiqmaydi"
    /// line (S6).
    #[must_use]
    pub fn uses_internet(self) -> bool {
        matches!(self, Self::Auto | Self::Internet)
    }

    /// Whether the address hints and the LAN search are used. "Internet orqali" skips them.
    #[must_use]
    pub fn uses_lan(self) -> bool {
        !matches!(self, Self::Internet)
    }

    /// The privacy lines under the send button (backlog S6), as i18n keys, in order: `privacy.encrypted`
    /// ("Fayllar shifrlangan, faqat siz va qabul qiluvchi ko'radi") always; `privacy.nothing_leaves` ("Hech narsa
    /// internetga chiqmaydi") only when the choice keeps everything off the internet, because only then is it
    /// true.
    #[must_use]
    pub fn privacy_lines(self) -> &'static [&'static str] {
        if self.uses_internet() {
            &["privacy.encrypted"]
        } else {
            &["privacy.encrypted", "privacy.nothing_leaves"]
        }
    }

    /// Whether the choice exists on any device yet.
    #[must_use]
    pub fn built(self) -> bool {
        !matches!(self, Self::Bluetooth)
    }

    /// The steps to try, best first. Automatic steps come before the offers; offers the device cannot start are
    /// left out.
    pub fn plan(self, s: Situation) -> Result<Vec<Step>, NoRoute> {
        if !self.built() {
            return Err(NoRoute::NotBuilt);
        }
        let mut out = Vec::new();
        let offers = |out: &mut Vec<Step>| {
            for (ok, o) in [
                (s.hotspot, Offer::Hotspot),
                (s.wifi_aware, Offer::WifiAware),
                (s.cable, Offer::Cable),
            ] {
                if ok {
                    out.push(Step::Offer(o));
                }
            }
        };
        match self {
            Self::Auto => {
                if s.lan {
                    out.push(Step::SameNetwork);
                }
                if s.internet {
                    out.extend([Step::InternetDirect, Step::Relay]);
                }
                offers(&mut out);
            }
            Self::LanOnly => {
                if s.lan {
                    out.push(Step::SameNetwork);
                }
            }
            Self::Internet => {
                if s.internet {
                    out.extend([Step::InternetDirect, Step::Relay]);
                }
            }
            Self::Offline => offers(&mut out),
            Self::Bluetooth => unreachable!("not built"),
        }
        if out.is_empty() {
            Err(NoRoute::Unavailable)
        } else {
            Ok(out)
        }
    }
}

/// The badge a live session shows: the route type, never a promise (ADR-0007 part 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Badge {
    /// "To'g'ridan-to'g'ri · LAN".
    DirectLan,
    /// "To'g'ridan-to'g'ri · internet".
    DirectInternet,
    /// "Relay orqali".
    Relay,
    /// "Hotspot".
    Hotspot,
    /// "Kabel".
    Cable,
    /// Connecting, no path chosen yet.
    Connecting,
}

impl Badge {
    /// i18n key (`route.badge.<key>`).
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::DirectLan => "route.badge.direct_lan",
            Self::DirectInternet => "route.badge.direct_internet",
            Self::Relay => "route.badge.relay",
            Self::Hotspot => "route.badge.hotspot",
            Self::Cable => "route.badge.cable",
            Self::Connecting => "route.badge.connecting",
        }
    }

    /// Whether the packets leave the local link (for the privacy line, S6).
    #[must_use]
    pub fn crosses_internet(self) -> bool {
        matches!(self, Self::DirectInternet | Self::Relay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_ON: Situation = Situation {
        lan: true,
        internet: true,
        hotspot: true,
        wifi_aware: true,
        cable: true,
    };

    #[test]
    fn auto_follows_the_adr_order() {
        assert_eq!(
            Picker::Auto.plan(ALL_ON).unwrap(),
            [
                Step::SameNetwork,
                Step::InternetDirect,
                Step::Relay,
                Step::Offer(Offer::Hotspot),
                Step::Offer(Offer::WifiAware),
                Step::Offer(Offer::Cable),
            ]
        );
        let field = Situation {
            hotspot: true,
            ..Situation::default()
        };
        assert_eq!(
            Picker::Auto.plan(field).unwrap(),
            [Step::Offer(Offer::Hotspot)],
            "no network, no internet: only what the user can start"
        );
    }

    #[test]
    fn forced_choices_stay_inside_their_route() {
        assert_eq!(Picker::LanOnly.plan(ALL_ON).unwrap(), [Step::SameNetwork]);
        assert_eq!(
            Picker::Internet.plan(ALL_ON).unwrap(),
            [Step::InternetDirect, Step::Relay]
        );
        assert_eq!(
            Picker::Offline.plan(ALL_ON).unwrap(),
            [
                Step::Offer(Offer::Hotspot),
                Step::Offer(Offer::WifiAware),
                Step::Offer(Offer::Cable)
            ]
        );
        assert!(!Picker::LanOnly.uses_internet() && !Picker::Offline.uses_internet());
        assert!(!Picker::Internet.uses_lan());
    }

    #[test]
    fn privacy_line_only_promises_what_is_true() {
        for p in Picker::ALL {
            let lines = p.privacy_lines();
            assert_eq!(lines[0], "privacy.encrypted");
            assert_eq!(
                lines.contains(&"privacy.nothing_leaves"),
                !p.uses_internet(),
                "{p:?}"
            );
        }
    }

    #[test]
    fn empty_plans_say_why() {
        assert_eq!(Picker::Bluetooth.plan(ALL_ON), Err(NoRoute::NotBuilt));
        let none = Situation::default();
        for p in [
            Picker::Auto,
            Picker::LanOnly,
            Picker::Internet,
            Picker::Offline,
        ] {
            assert_eq!(p.plan(none), Err(NoRoute::Unavailable), "{p:?}");
        }
        assert_eq!(NoRoute::NotBuilt.key(), "route.none.not_built");
    }

    #[test]
    fn keys_are_unique_and_serde_is_stable() {
        let keys: std::collections::HashSet<_> = Picker::ALL.iter().map(|p| p.key()).collect();
        assert_eq!(keys.len(), Picker::ALL.len());
        assert_eq!(
            serde_json::to_string(&Picker::LanOnly).unwrap(),
            "\"lan_only\""
        );
        assert_eq!(
            serde_json::from_str::<Picker>("\"auto\"").unwrap(),
            Picker::Auto
        );
        let mut de = serde_json::Deserializer::from_str("\"wifi_direct\"");
        assert_eq!(lenient(&mut de).unwrap(), Picker::Auto, "a newer value");
        let mut de = serde_json::Deserializer::from_str("\"offline\"");
        assert_eq!(lenient(&mut de).unwrap(), Picker::Offline);
        assert!(Badge::Relay.crosses_internet() && !Badge::Hotspot.crosses_internet());
    }
}
