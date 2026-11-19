//! Opt-in anonymous route telemetry (backlog M2, ADR-0077): how many sessions ended up direct on the LAN, direct
//! over the internet, on the hotspot or cable, through the relay, or failed. The answer decides how much relay
//! capacity Vutric needs and whether the hole punching works. Pure: counting, coarsening and the report; sending
//! is `vutric_net::telemetry`.
//!
//! - Off by default ([`TelemetrySettings::enabled`]); nothing is counted for sending while it is off.
//! - No identifiers: no device id, no install id, no IP, no peer, no file data, no app version. A report is one
//!   UTC day's coarse counts plus the platform family.
//! - Counts are rounded down to buckets ([`bucket`]) so a report cannot tell one session from two-ish.
//! - Only finished days are sent, never "today", and at most one report per day.

use serde::{Deserialize, Serialize};

use crate::Platform;
use crate::route::Badge;

pub const REPORT_VERSION: u8 = 1;

/// Bucket floors: a count is reported as the largest floor not above it.
pub const BUCKETS: [u32; 9] = [0, 1, 2, 3, 6, 11, 21, 51, 101];

/// The user's choice, off by default. `endpoint` is empty until the owner picks a collector (ADR-0077), so even an
/// enabled client sends nothing before that.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TelemetrySettings {
    pub enabled: bool,
    pub endpoint: String,
}

impl TelemetrySettings {
    /// Whether a report may be sent: on, and an `https` collector set.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.enabled
            && self.endpoint.starts_with("https://")
            && self.endpoint.len() > 8
            && self.endpoint.len() <= 512
    }
}

/// How sessions ended, for one day.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tally {
    pub direct_lan: u32,
    pub direct_internet: u32,
    pub hotspot: u32,
    pub cable: u32,
    pub relay: u32,
    /// Started on the relay and moved to a direct path (hole punching worked late).
    pub upgraded: u32,
    pub failed: u32,
}

impl Tally {
    /// Counts one session by the badge it had when it ended (`Connecting` = never got a path = failed).
    pub fn record(&mut self, last: Badge, started_relayed: bool) {
        let slot = match last {
            Badge::DirectLan => &mut self.direct_lan,
            Badge::DirectInternet => &mut self.direct_internet,
            Badge::Hotspot => &mut self.hotspot,
            Badge::Cable => &mut self.cable,
            Badge::Relay => &mut self.relay,
            Badge::Connecting => &mut self.failed,
        };
        *slot = slot.saturating_add(1);
        if started_relayed && !matches!(last, Badge::Relay | Badge::Connecting) {
            self.upgraded = self.upgraded.saturating_add(1);
        }
    }

    #[must_use]
    pub fn total(&self) -> u64 {
        [
            self.direct_lan,
            self.direct_internet,
            self.hotspot,
            self.cable,
            self.relay,
            self.failed,
        ]
        .iter()
        .map(|&n| u64::from(n))
        .sum()
    }

    /// Every count rounded down to its bucket.
    #[must_use]
    pub fn coarse(&self) -> Self {
        Self {
            direct_lan: bucket(self.direct_lan),
            direct_internet: bucket(self.direct_internet),
            hotspot: bucket(self.hotspot),
            cable: bucket(self.cable),
            relay: bucket(self.relay),
            upgraded: bucket(self.upgraded),
            failed: bucket(self.failed),
        }
    }
}

/// The largest bucket floor not above `n`.
#[must_use]
pub fn bucket(n: u32) -> u32 {
    BUCKETS.iter().rev().copied().find(|&b| b <= n).unwrap_or(0)
}

/// The day being counted (UTC days since 1970), kept by the host in a small file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Day {
    pub day: u32,
    pub tally: Tally,
}

impl Day {
    /// UTC day number of Unix second `now`.
    #[must_use]
    pub fn number(now: u64) -> u32 {
        u32::try_from(now / 86_400).unwrap_or(u32::MAX)
    }

    /// Moves to `today`: returns the finished day if one had sessions, and starts an empty one.
    pub fn roll(&mut self, today: u32) -> Option<Self> {
        if self.day == today {
            return None;
        }
        let done = *self;
        *self = Self {
            day: today,
            tally: Tally::default(),
        };
        (done.day < today && done.tally.total() > 0).then_some(done)
    }
}

/// What goes over the wire, and nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub v: u8,
    pub day: u32,
    pub platform: Platform,
    pub tally: Tally,
}

/// The report for a finished day, coarsened; `None` when telemetry is off or the day was empty.
#[must_use]
pub fn report(settings: &TelemetrySettings, finished: &Day, platform: Platform) -> Option<Report> {
    if !settings.ready() || finished.tally.total() == 0 {
        return None;
    }
    Some(Report {
        v: REPORT_VERSION,
        day: finished.day,
        platform,
        tally: finished.tally.coarse(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> TelemetrySettings {
        TelemetrySettings {
            enabled: true,
            endpoint: "https://t.example.org/v1/routes".into(),
        }
    }

    #[test]
    fn off_by_default_and_https_only() {
        let s = TelemetrySettings::default();
        assert!(!s.enabled && !s.ready());
        assert!(on().ready());
        let plain = TelemetrySettings {
            endpoint: "http://t.example.org".into(),
            ..on()
        };
        assert!(!plain.ready());
        let no_host = TelemetrySettings {
            enabled: true,
            endpoint: String::new(),
        };
        assert!(!no_host.ready(), "no collector chosen yet");
    }

    #[test]
    fn sessions_are_counted_by_their_last_badge() {
        let mut t = Tally::default();
        t.record(Badge::DirectLan, false);
        t.record(Badge::DirectInternet, true);
        t.record(Badge::Relay, true);
        t.record(Badge::Connecting, false);
        t.record(Badge::Cable, false);
        assert_eq!(
            t,
            Tally {
                direct_lan: 1,
                direct_internet: 1,
                relay: 1,
                failed: 1,
                cable: 1,
                upgraded: 1,
                hotspot: 0,
            }
        );
        assert_eq!(t.total(), 5);
    }

    #[test]
    fn buckets_round_down() {
        assert_eq!(
            [
                0,
                1,
                2,
                3,
                4,
                5,
                6,
                10,
                11,
                20,
                21,
                50,
                51,
                100,
                101,
                u32::MAX
            ]
            .map(bucket),
            [0, 1, 2, 3, 3, 3, 6, 6, 11, 11, 21, 21, 51, 51, 101, 101]
        );
    }

    #[test]
    fn only_finished_non_empty_days_are_reported_and_coarse() {
        let mut d = Day {
            day: 20_000,
            tally: Tally::default(),
        };
        assert_eq!(d.roll(20_000), None);
        for _ in 0..7 {
            d.tally.record(Badge::Relay, false);
        }
        assert_eq!(d.roll(20_000), None, "today is never reported");
        let done = d.roll(20_001).expect("yesterday");
        assert_eq!(d.day, 20_001);
        assert_eq!(d.tally.total(), 0);
        let r = report(&on(), &done, Platform::Linux).unwrap();
        assert_eq!(r.tally.relay, 6);
        assert_eq!(r.day, 20_000);
        assert_eq!(
            report(&TelemetrySettings::default(), &done, Platform::Linux),
            None
        );
        assert_eq!(d.roll(20_002), None, "an empty day is not reported");
        assert_eq!(Day::number(1_795_000_000), 20_775);
    }

    #[test]
    fn a_report_has_no_identifiers() {
        let mut d = Day::default();
        d.tally.record(Badge::DirectLan, false);
        let r = report(&on(), &d, Platform::Android).unwrap();
        let json = serde_json::to_string(&r).unwrap();
        let keys: serde_json::Value = serde_json::from_str(&json).unwrap();
        let mut top: Vec<&str> = keys
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        top.sort_unstable();
        assert_eq!(top, ["day", "platform", "tally", "v"]);
        for banned in ["id", "ip", "name", "version", "peer"] {
            assert!(!json.contains(&format!("\"{banned}\"")), "{json}");
        }
    }
}
