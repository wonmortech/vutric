//! Who may send to this device (ADR-0007 part 2, backlog S1 and S3).
//!
//! - [`TrustStore`]: the keys of devices the user paired with. A device is trusted only by its endpoint key; a
//!   name or an address never makes a device trusted.
//! - [`Visibility`]: whether unknown devices may knock at all. The default lets only trusted devices in.
//!   "Everyone" lasts [`EVERYONE_FOR`] and then switches itself off, as `AirDrop` does.
//! - [`admit`]: the one decision every host and every route makes for an incoming connection, before anything
//!   the peer sends is parsed.
//!
//! Pure data: the host keeps the file (`vutric_net::trust`) and the clock.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{EndpointId, Platform};

/// Bumped when a field changes meaning; [`TrustStore::migrate`] brings older files forward.
pub const TRUST_VERSION: u32 = 1;

/// How long "everyone can send to me" stays on.
pub const EVERYONE_FOR_SECS: u64 = 10 * 60;

/// [`EVERYONE_FOR_SECS`] as a duration.
pub const EVERYONE_FOR: core::time::Duration = core::time::Duration::from_secs(EVERYONE_FOR_SECS);

/// Most devices kept; pairing one more forgets the one seen longest ago.
pub const MAX_DEVICES: usize = 512;

const MAX_NAME_CHARS: usize = 64;

/// How a device became trusted. Shown on the device screen; a weaker method may later need a re-check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairMethod {
    /// The other device's QR code or pairing link: its key came out of band, the strongest pairing.
    Ticket,
    /// Both users compared the six-digit code of a commit-then-reveal exchange (`crate::sas`).
    Code,
    /// The user accepted a foreign-protocol peer and pinned its fingerprint (ADR-0010 rule 2).
    Pinned,
}

/// One paired device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedDevice {
    /// Display name, as the device called itself when paired or last seen; never used to decide trust.
    pub name: String,
    pub platform: Option<Platform>,
    pub method: PairMethod,
    /// Unix seconds.
    pub paired_at: u64,
    /// Unix seconds of the last session, for "seen 3 days ago" and for forgetting the oldest.
    pub last_seen: u64,
    /// Its key for BLE rotating ids, once it sent one (ADR-0071); lets us recognise it nearby.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ble_key: Option<crate::ble::BleKey>,
}

/// The trusted devices of this device, keyed by endpoint id (64 hex).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrustStore {
    pub version: u32,
    devices: BTreeMap<String, TrustedDevice>,
}

impl Default for TrustStore {
    fn default() -> Self {
        Self {
            version: TRUST_VERSION,
            devices: BTreeMap::new(),
        }
    }
}

fn clean_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(c, '\u{200E}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        })
        .take(MAX_NAME_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn oldest(devices: &BTreeMap<String, TrustedDevice>) -> Option<String> {
    devices
        .iter()
        .min_by_key(|(_, d)| d.last_seen)
        .map(|(k, _)| k.clone())
}

impl TrustStore {
    #[must_use]
    pub fn is_trusted(&self, id: &EndpointId) -> bool {
        self.devices.contains_key(&id.to_string())
    }

    #[must_use]
    pub fn get(&self, id: &EndpointId) -> Option<&TrustedDevice> {
        self.devices.get(&id.to_string())
    }

    /// Every trusted device, ordered by key.
    pub fn iter(&self) -> impl Iterator<Item = (EndpointId, &TrustedDevice)> {
        self.devices
            .iter()
            .filter_map(|(k, d)| k.parse().ok().map(|id| (id, d)))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.devices.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }

    /// Trusts `id`. Pairing again keeps the first `paired_at` and upgrades the method only to a stronger one.
    pub fn trust(
        &mut self,
        id: EndpointId,
        name: &str,
        platform: Option<Platform>,
        method: PairMethod,
        now: u64,
    ) {
        let key = id.to_string();
        if let Some(d) = self.devices.get_mut(&key) {
            d.name = clean_name(name);
            d.platform = platform.or(d.platform);
            d.last_seen = d.last_seen.max(now);
            if strength(method) > strength(d.method) {
                d.method = method;
            }
            return;
        }
        if self.devices.len() >= MAX_DEVICES
            && let Some(k) = oldest(&self.devices)
        {
            self.devices.remove(&k);
        }
        self.devices.insert(
            key,
            TrustedDevice {
                name: clean_name(name),
                platform,
                method,
                paired_at: now,
                last_seen: now,
                ble_key: None,
            },
        );
    }

    /// Records a session with a trusted device (the name it uses now, the time). Unknown ids are ignored:
    /// seeing a device never makes it trusted.
    pub fn seen(&mut self, id: &EndpointId, name: &str, now: u64) {
        if let Some(d) = self.devices.get_mut(&id.to_string()) {
            let name = clean_name(name);
            if !name.is_empty() {
                d.name = name;
            }
            d.last_seen = d.last_seen.max(now);
        }
    }

    /// Keeps the BLE key a trusted device sent (`Message::BleKey`, ADR-0071). Unknown ids are ignored. Returns
    /// whether it was stored.
    pub fn set_ble_key(&mut self, id: &EndpointId, key: crate::ble::BleKey) -> bool {
        self.devices
            .get_mut(&id.to_string())
            .map(|d| d.ble_key = Some(key))
            .is_some()
    }

    /// The BLE keys of the trusted devices, for `ble::resolve`.
    pub fn ble_keys(&self) -> impl Iterator<Item = (EndpointId, &crate::ble::BleKey)> {
        self.devices
            .iter()
            .filter_map(|(k, d)| Some((k.parse().ok()?, d.ble_key.as_ref()?)))
    }

    /// Forgets a device; it then needs pairing again. Returns whether it was trusted.
    pub fn forget(&mut self, id: &EndpointId) -> bool {
        self.devices.remove(&id.to_string()).is_some()
    }

    /// Brings a file written by an older version forward and drops entries that cannot be right.
    #[must_use]
    pub fn migrate(mut self) -> Self {
        self.version = TRUST_VERSION;
        self.devices
            .retain(|k, _| k.parse::<EndpointId>().is_ok() && *k == k.to_ascii_lowercase());
        for d in self.devices.values_mut() {
            d.name = clean_name(&d.name);
        }
        while self.devices.len() > MAX_DEVICES {
            let Some(k) = oldest(&self.devices) else {
                break;
            };
            self.devices.remove(&k);
        }
        self
    }
}

fn strength(m: PairMethod) -> u8 {
    match m {
        PairMethod::Pinned => 0,
        PairMethod::Code => 1,
        PairMethod::Ticket => 2,
    }
}

/// Who may knock. Kept by the host (settings file or memory); the clock is passed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode", content = "until")]
pub enum Visibility {
    /// Not visible: no incoming connection at all (the "pause" switch).
    Off,
    /// Only trusted devices; unknown ones are refused before their first message is read. The default.
    #[default]
    TrustedOnly,
    /// Anyone nearby may offer files (the user is still asked each time), until this Unix second.
    Everyone(u64),
    /// The pairing screen is open (QR or code shown) until this Unix second: unknown devices may connect to pair.
    Pairing(u64),
}

impl Visibility {
    /// "Everyone" from `now` for [`EVERYONE_FOR_SECS`].
    #[must_use]
    pub fn everyone(now: u64) -> Self {
        Self::Everyone(now.saturating_add(EVERYONE_FOR_SECS))
    }

    /// The pairing window from `now` for [`EVERYONE_FOR_SECS`] (closing the screen ends it earlier, with
    /// [`Visibility::TrustedOnly`]).
    #[must_use]
    pub fn pairing(now: u64) -> Self {
        Self::Pairing(now.saturating_add(EVERYONE_FOR_SECS))
    }

    /// What is in force at `now`: an expired window is [`Visibility::TrustedOnly`]. A window that ends further
    /// ahead than [`EVERYONE_FOR_SECS`] counts as expired too, so a clock set back or an edited file cannot keep
    /// "everyone" on.
    #[must_use]
    pub fn at(self, now: u64) -> Self {
        match self {
            Self::Everyone(until) | Self::Pairing(until)
                if until <= now || until - now > EVERYONE_FOR_SECS =>
            {
                Self::TrustedOnly
            }
            v => v,
        }
    }

    /// Seconds left of an open window at `now`, for the countdown on screen.
    #[must_use]
    pub fn remaining(self, now: u64) -> Option<u64> {
        match self.at(now) {
            Self::Everyone(until) | Self::Pairing(until) => Some(until - now),
            _ => None,
        }
    }
}

/// The answer for one incoming connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// A paired device: its offers follow the user's accept rules.
    Trusted,
    /// An unknown device while "everyone" or the pairing screen is on: let it in and ask the user about every
    /// offer (or run the pairing check). Never auto-accepted.
    Unknown,
    /// Close before reading anything.
    Refuse,
}

/// The decision for a connection from `peer` at Unix second `now`.
#[must_use]
pub fn admit(store: &TrustStore, visibility: Visibility, peer: &EndpointId, now: u64) -> Admission {
    match visibility.at(now) {
        Visibility::Off => Admission::Refuse,
        _ if store.is_trusted(peer) => Admission::Trusted,
        Visibility::Everyone(_) | Visibility::Pairing(_) => Admission::Unknown,
        Visibility::TrustedOnly => Admission::Refuse,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: u64 = 1_795_000_000;

    fn id(b: u8) -> EndpointId {
        EndpointId([b; 32])
    }

    #[test]
    fn default_lets_only_trusted_in() {
        let mut s = TrustStore::default();
        let v = Visibility::default();
        assert_eq!(admit(&s, v, &id(1), T), Admission::Refuse);
        s.trust(
            id(1),
            "Pixel",
            Some(Platform::Android),
            PairMethod::Ticket,
            T,
        );
        assert_eq!(admit(&s, v, &id(1), T), Admission::Trusted);
        assert_eq!(admit(&s, v, &id(2), T), Admission::Refuse);
        assert_eq!(admit(&s, Visibility::Off, &id(1), T), Admission::Refuse);
    }

    #[test]
    fn everyone_ends_after_ten_minutes() {
        let s = TrustStore::default();
        let v = Visibility::everyone(T);
        assert_eq!(admit(&s, v, &id(9), T), Admission::Unknown);
        assert_eq!(admit(&s, v, &id(9), T + 599), Admission::Unknown);
        assert_eq!(v.remaining(T + 599), Some(1));
        assert_eq!(admit(&s, v, &id(9), T + 600), Admission::Refuse);
        assert_eq!(v.at(T + 600), Visibility::TrustedOnly);
        assert_eq!(v.remaining(T + 600), None);
    }

    #[test]
    fn a_far_future_window_is_not_honoured() {
        let s = TrustStore::default();
        assert_eq!(
            admit(&s, Visibility::Everyone(T + 86_400), &id(3), T),
            Admission::Refuse
        );
        assert_eq!(
            admit(&s, Visibility::Pairing(u64::MAX), &id(3), T),
            Admission::Refuse
        );
        assert_eq!(
            admit(&s, Visibility::pairing(T), &id(3), T + 1),
            Admission::Unknown
        );
    }

    #[test]
    fn repairing_keeps_history_and_upgrades_only() {
        let mut s = TrustStore::default();
        s.trust(id(1), "a", None, PairMethod::Code, T);
        s.trust(
            id(1),
            "b\u{202E}",
            Some(Platform::Ios),
            PairMethod::Ticket,
            T + 5,
        );
        assert_eq!(s.get(&id(1)).unwrap().name, "b");
        s.trust(id(1), "c", None, PairMethod::Pinned, T + 9);
        let d = s.get(&id(1)).unwrap();
        assert_eq!(d.method, PairMethod::Ticket);
        assert_eq!(d.paired_at, T);
        assert_eq!(d.last_seen, T + 9);
        assert_eq!(d.name, "c");
        assert_eq!(d.platform, Some(Platform::Ios));
        assert!(s.forget(&id(1)));
        assert!(!s.forget(&id(1)));
    }

    #[test]
    fn seeing_never_trusts() {
        let mut s = TrustStore::default();
        s.seen(&id(4), "x", T);
        assert!(s.is_empty());
    }

    #[test]
    fn full_store_forgets_the_oldest() {
        let mut s = TrustStore::default();
        for i in 0..MAX_DEVICES {
            let mut b = [0u8; 32];
            b[..8].copy_from_slice(&(i as u64).to_le_bytes());
            s.trust(EndpointId(b), "d", None, PairMethod::Ticket, T + i as u64);
        }
        s.trust(id(0xff), "new", None, PairMethod::Ticket, T + 10_000);
        assert_eq!(s.len(), MAX_DEVICES);
        assert!(!s.is_trusted(&EndpointId([0; 32])));
        assert!(s.is_trusted(&id(0xff)));
    }

    #[test]
    fn migrate_drops_bad_keys() {
        let mut s = TrustStore::default();
        s.trust(id(1), "ok", None, PairMethod::Ticket, T);
        let bad = TrustedDevice {
            name: "x".into(),
            platform: None,
            method: PairMethod::Ticket,
            paired_at: 0,
            last_seen: 0,
            ble_key: None,
        };
        s.devices.insert("nothex".into(), bad.clone());
        s.devices.insert("AB".repeat(32), bad);
        s.version = 0;
        let s = s.migrate();
        assert_eq!(s.len(), 1);
        assert_eq!(s.version, TRUST_VERSION);
        assert_eq!(s.iter().count(), 1);
    }

    #[test]
    fn ble_keys_only_for_trusted_devices_and_kept_in_json() {
        let mut s = TrustStore::default();
        let key = crate::ble::BleKey([4; 16]);
        assert!(
            !s.set_ble_key(&id(1), key),
            "seeing a key never makes a device trusted"
        );
        s.trust(id(1), "Pixel", None, PairMethod::Ticket, T);
        s.trust(id(2), "Ofis", None, PairMethod::Code, T);
        assert!(s.set_ble_key(&id(1), key));
        assert_eq!(s.ble_keys().collect::<Vec<_>>(), vec![(id(1), &key)]);
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(&"04".repeat(16)), "{json}");
        assert!(
            !json.contains("\"ble_key\":null"),
            "absent keys are not written"
        );
        assert_eq!(serde_json::from_str::<TrustStore>(&json).unwrap(), s);
    }
}
