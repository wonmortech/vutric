//! Connect without the QR (issue #9, ADR-0059): the user types the desktop's address (`192.168.1.24`) or its
//! short code (`K7QD-2M9X`), and the beacon turns that into the endpoint id and QUIC address `Peer::connect`
//! needs.
//!
//! - An **address** gets a unicast beacon query; the answer from that IP carries the id and port.
//! - A **code** gets a multicast query and, if allowed, the opt-in subnet scan; the first answer whose id
//!   starts with the code wins.
//!
//! Neither proves who answered, so the result is [`Found::Unverified`] by construction: connect, then run the
//! 6-digit comparison (`Session::pair_code` / `pair_finish`) before trusting the device with
//! `PairMethod::Code`. The other side must have its pairing window open (ADR-0050), as for any new device.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use tokio::sync::broadcast::error::RecvError;
use tokio::time::Instant;
use vutric_core::EndpointId;
use vutric_core::code::ShortCode;

use crate::beacon::{BeaconService, Heard};

/// Gap between repeated queries: above the beacon's per-source answer limit (500 ms), and UDP may drop one.
const RETRY: Duration = Duration::from_millis(700);

/// What the user typed, understood.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// An IPv4 address, with or without a port. The beacon always answers on its own port, so a typed port is
    /// only a hint and the announced QUIC port wins.
    Address(IpAddr),
    Code(ShortCode),
}

impl Target {
    /// Reads `192.168.1.24`, `192.168.1.24:47110` or a short code. `None` for anything else (a full pairing
    /// link goes to `PairingTicket::parse` instead). IPv6 is refused: the beacon is IPv4 only, and a
    /// link-local IPv6 address cannot be typed usefully anyway.
    #[must_use]
    pub fn parse(typed: &str) -> Option<Self> {
        let t = typed.trim();
        let ip = t
            .parse::<IpAddr>()
            .ok()
            .or_else(|| t.parse::<SocketAddr>().ok().map(|a| a.ip()));
        match ip {
            Some(ip @ IpAddr::V4(v4))
                if !v4.is_unspecified() && !v4.is_broadcast() && !v4.is_multicast() =>
            {
                Some(Self::Address(ip))
            }
            Some(_) => None,
            None => ShortCode::parse(t).map(Self::Code),
        }
    }

    fn accepts(&self, h: &Heard) -> bool {
        match self {
            Self::Address(ip) => h.addr.ip() == *ip,
            Self::Code(code) => code.matches(&h.id),
        }
    }
}

/// A device that answered. Not authenticated: see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    pub id: EndpointId,
    /// Its QUIC address, for `Peer::connect(id, &[addr])`.
    pub addr: SocketAddr,
    pub phone: bool,
}

/// Why nothing was found, for the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotFound {
    /// No device answered in time: wrong address or code, the other device hidden or off this network, or the
    /// network keeps devices apart (offer the full link, the relay or a hotspot).
    Silent,
    /// The OS refused to send (iOS local network permission denied).
    Denied,
}

impl NotFound {
    /// Stable key for the UI's i18n table.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Silent => "find.silent",
            Self::Denied => "find.denied",
        }
    }
}

impl std::fmt::Display for NotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Silent => "no device answered",
            Self::Denied => "sending on the local network was refused",
        })
    }
}

impl std::error::Error for NotFound {}

/// Looks for `target` for at most `wait`. `scan` allows the subnet scan for a code (only while the "add
/// device" screen is open, as for [`BeaconService::scan`]). `beacon_port` is where the other side's beacon
/// listens: `vutric_core::beacon::PORT` in production.
pub async fn resolve(
    beacon: &BeaconService,
    target: Target,
    beacon_port: u16,
    wait: Duration,
    scan: bool,
) -> Result<Found, NotFound> {
    let mut rx = beacon.subscribe();
    let denied_before = beacon.health().denied_sends;
    let deadline = Instant::now() + wait;
    let mut next_query = Instant::now();
    let mut scanned = false;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(if beacon.health().denied_sends > denied_before {
                NotFound::Denied
            } else {
                NotFound::Silent
            });
        }
        if now >= next_query {
            match target {
                Target::Address(ip) => {
                    beacon.query_to(SocketAddr::new(ip, beacon_port)).await;
                }
                Target::Code(_) => {
                    beacon.query().await;
                    if scan && !scanned {
                        scanned = true;
                        beacon.scan().await;
                    }
                }
            }
            next_query = Instant::now() + RETRY;
        }
        let until = next_query.min(deadline);
        match tokio::time::timeout_at(until, rx.recv()).await {
            Ok(Ok(h)) if target.accepts(&h) => {
                return Ok(Found {
                    id: h.id,
                    addr: h.addr,
                    phone: h.phone,
                });
            }
            Ok(Ok(_) | Err(RecvError::Lagged(_))) | Err(_) => {}
            Ok(Err(RecvError::Closed)) => return Err(NotFound::Silent),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_people_type() {
        assert_eq!(
            Target::parse(" 192.168.1.24 "),
            Some(Target::Address("192.168.1.24".parse().unwrap()))
        );
        assert_eq!(
            Target::parse("192.168.1.24:47110"),
            Some(Target::Address("192.168.1.24".parse().unwrap()))
        );
        assert!(matches!(Target::parse("k7qd-2m9x"), Some(Target::Code(_))));
        for bad in [
            "",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.167",
            "fe80::1",
            "[fd00::1]:4000",
            "vutric://pair?v=2",
            "192.168.1",
        ] {
            assert_eq!(Target::parse(bad), None, "{bad}");
        }
    }
}
