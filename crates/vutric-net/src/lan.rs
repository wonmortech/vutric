//! Stronger LAN finding (backlog R1, ADR-0007 route R1): which local addresses go into the QR and in what order,
//! link-local IPv6 that works across machines, and a plain-language verdict when the other device cannot be
//! reached, so the UI can say why and offer the next route.
//!
//! - [`local_addrs`]: every usable interface address, ranked: wired, Wi-Fi, tethering, other, virtual adapters
//!   (Hyper-V, VPNs, Docker) last, so the 8 QR slots go to addresses a phone can actually reach.
//! - [`ticket_addrs`]: the QR hints. Private IPv4, `169.254/16`, carrier-grade NAT `100.64/10` (Tailscale and
//!   some tethering), unique-local and link-local IPv6. A link-local IPv6 hint is written without its zone, which
//!   only means something on the machine that wrote it.
//! - [`expand_link_local`]: on the dialing side, a zone-less link-local hint becomes one candidate per local
//!   interface, so two computers on one cable (R5a) or one switch without DHCP still connect.
//! - [`diagnose`]: local network denied (iOS), AP isolation, no shared network, or no network at all.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6};

use vutric_core::pairing::PairingTicket;

/// What kind of link an address sits on; lower ranks first in the QR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Medium {
    Wired,
    WiFi,
    /// A phone hotspot or USB tethering link (ours or the OS's).
    Tether,
    Other,
    /// Hyper-V, WSL, Docker, `VMware`, `VirtualBox`, VPN and overlay adapters: rarely reachable from a phone.
    Virtual,
}

/// One address of this device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalAddr {
    pub ip: IpAddr,
    pub prefix: u8,
    /// OS interface index (the zone of link-local IPv6).
    pub iface: u32,
    pub iface_name: String,
    pub medium: Medium,
}

const VIRTUAL_HINTS: [&str; 18] = [
    "vethernet",
    "hyper-v",
    "virtual",
    "vmware",
    "vmnet",
    "vbox",
    "wsl",
    "docker",
    "veth",
    "virbr",
    "tailscale",
    "zerotier",
    "wireguard",
    "wg",
    "utun",
    "tun",
    "tap",
    "vpn",
];

const TETHER_HINTS: [&str; 7] = [
    "rndis",
    "ap0",
    "swlan",
    "softap",
    "usb",
    "iphone",
    "apple mobile",
];

/// Classifies an interface by its names and type. Pure, so it is tested without the OS.
#[must_use]
pub fn classify(names: &[&str], wired: bool, wifi: bool, ip: IpAddr) -> Medium {
    let lower: Vec<String> = names.iter().map(|n| n.to_ascii_lowercase()).collect();
    let has = |hints: &[&str]| {
        lower.iter().any(|n| {
            hints.iter().any(|h| {
                if h.len() <= 3 {
                    // Short hints (`wg`, `tun`, `tap`, `en`) only as a name prefix, so "Ethernet 2" is not "tun".
                    n.starts_with(h)
                } else {
                    n.contains(h)
                }
            })
        })
    };
    if has(&VIRTUAL_HINTS) {
        return Medium::Virtual;
    }
    let tether_subnet = match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            matches!(
                (o[0], o[1], o[2]),
                (172, 20, 10) | (192, 168, 42 | 43 | 137)
            )
        }
        IpAddr::V6(_) => false,
    };
    if has(&TETHER_HINTS) || tether_subnet {
        return Medium::Tether;
    }
    if wifi || has(&["wi-fi", "wifi", "wlan", "wireless", "airport"]) {
        return Medium::WiFi;
    }
    if wired || has(&["ethernet", "eth", "en"]) {
        return Medium::Wired;
    }
    Medium::Other
}

/// Whether an address can be reached by a device on the same network (the only kind the QR carries).
#[must_use]
pub fn is_lan_scope(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || is_cgnat(v4),
        IpAddr::V6(v6) => is_ula(v6) || is_link_local_v6(v6),
    }
}

fn is_cgnat(v4: Ipv4Addr) -> bool {
    let o = v4.octets();
    o[0] == 100 && (o[1] & 0xc0) == 64
}

fn is_ula(v6: Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xfe00) == 0xfc00
}

#[must_use]
pub fn is_link_local_v6(v6: Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xffc0) == 0xfe80
}

/// Every up, non-loopback interface address in LAN scope, best first.
#[must_use]
pub fn local_addrs() -> Vec<LocalAddr> {
    use netdev::interface::types::InterfaceType as T;
    let mut out = Vec::new();
    for i in netdev::get_interfaces() {
        if !i.is_up() || i.is_loopback() {
            continue;
        }
        let wired = matches!(
            i.if_type,
            T::Ethernet | T::GigabitEthernet | T::FastEthernetT | T::FastEthernetFx
        );
        let wifi = i.if_type == T::Wireless80211;
        let names: Vec<&str> = [
            Some(i.name.as_str()),
            i.friendly_name.as_deref(),
            i.description.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect();
        let shown = i.friendly_name.clone().unwrap_or_else(|| i.name.clone());
        let v4 = i
            .ipv4
            .iter()
            .map(|n| (IpAddr::V4(n.addr()), n.prefix_len()));
        let v6 = i
            .ipv6
            .iter()
            .map(|n| (IpAddr::V6(n.addr()), n.prefix_len()));
        for (ip, prefix) in v4.chain(v6) {
            if is_lan_scope(ip) {
                out.push(LocalAddr {
                    ip,
                    prefix,
                    iface: i.index,
                    iface_name: shown.clone(),
                    medium: classify(&names, wired, wifi, ip),
                });
            }
        }
    }
    out.sort_by_key(|a| (a.medium, addr_class(a.ip)));
    out
}

/// Inside one medium: private IPv4, IPv4 link-local, unique-local IPv6, link-local IPv6, CGNAT.
fn addr_class(ip: IpAddr) -> u8 {
    match ip {
        IpAddr::V4(v4) if v4.is_private() => 0,
        IpAddr::V4(v4) if v4.is_link_local() => 1,
        IpAddr::V6(v6) if is_ula(v6) => 2,
        IpAddr::V6(_) => 3,
        IpAddr::V4(_) => 4,
    }
}

/// Most address hints in a pairing ticket (the QR stays scannable).
pub const MAX_HINTS: usize = 8;

/// The QR hints: `bound` (the endpoint's listening addresses, with ports) in LAN scope, ranked by the interface
/// they sit on, zones stripped, at most [`MAX_HINTS`].
#[must_use]
pub fn ticket_addrs(
    bound: impl IntoIterator<Item = SocketAddr>,
    locals: &[LocalAddr],
) -> Vec<SocketAddr> {
    let medium = |ip: IpAddr| {
        locals
            .iter()
            .find(|l| l.ip == ip)
            .map_or(Medium::Other, |l| l.medium)
    };
    let mut addrs: Vec<SocketAddr> = bound
        .into_iter()
        .filter(|a| is_lan_scope(a.ip()))
        .map(|a| match a {
            SocketAddr::V6(v6) => SocketAddr::V6(SocketAddrV6::new(*v6.ip(), v6.port(), 0, 0)),
            v4 @ SocketAddr::V4(_) => v4,
        })
        .collect();
    addrs.sort_by_key(|a| (medium(a.ip()), addr_class(a.ip()), *a));
    addrs.dedup();
    addrs.truncate(MAX_HINTS);
    addrs
}

/// Most dial candidates after link-local expansion.
pub const MAX_CANDIDATES: usize = 32;

/// Gives every zone-less link-local IPv6 hint one candidate per local interface that has a link-local address,
/// since the hint's own zone belonged to the other machine. Other hints are kept as they are.
#[must_use]
pub fn expand_link_local(hints: &[SocketAddr], locals: &[LocalAddr]) -> Vec<SocketAddr> {
    let mut zones: Vec<u32> = locals
        .iter()
        .filter(|l| matches!(l.ip, IpAddr::V6(v6) if is_link_local_v6(v6)))
        .map(|l| l.iface)
        .collect();
    zones.sort_unstable();
    zones.dedup();
    let mut out = Vec::new();
    for h in hints {
        match h {
            SocketAddr::V6(v6) if is_link_local_v6(*v6.ip()) && v6.scope_id() == 0 => {
                for z in &zones {
                    out.push(SocketAddr::V6(SocketAddrV6::new(
                        *v6.ip(),
                        v6.port(),
                        0,
                        *z,
                    )));
                }
            }
            other => out.push(*other),
        }
    }
    out.dedup();
    out.truncate(MAX_CANDIDATES);
    out
}

/// Whether `ip` is on one of our IPv4 subnets (by prefix), or link-local / unique-local on a link we have.
#[must_use]
pub fn same_network(ip: IpAddr, locals: &[LocalAddr]) -> bool {
    locals.iter().any(|l| match (l.ip, ip) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            let bits = u32::from(l.prefix.min(32));
            if bits == 0 {
                return false;
            }
            let mask = u32::MAX << (32 - bits);
            (u32::from(a) & mask) == (u32::from(b) & mask)
        }
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            (is_link_local_v6(a) && is_link_local_v6(b))
                || (is_ula(a) && a.segments()[..4] == b.segments()[..4])
        }
        _ => false,
    })
}

/// Why the other device could not be reached, for the screen that explains it (ADR-0007 R1, backlog R1a).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreachable {
    /// The OS keeps this app off the local network: iOS "Local Network" permission off, or a firewall rule.
    /// Screen: how to turn it on (Settings, Privacy, Local Network, Vutric).
    LocalNetworkDenied,
    /// We share a subnet with the other device, but nothing it sends arrives: client isolation on guest, hotel or
    /// office Wi-Fi, or a firewall. "This network keeps devices apart." Offer the relay (if `internet`) or hotspot.
    Isolated { internet: bool },
    /// The other device is on another network. Reached over the internet when `internet`, else offer hotspot.
    OtherNetwork { internet: bool },
    /// This device has no network address at all: offer hotspot or cable.
    NoNetwork,
}

impl Unreachable {
    /// Stable i18n key for the explanation screen (`lan.unreachable.<key>.title` / `.body`).
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::LocalNetworkDenied => "local_network_denied",
            Self::Isolated { .. } => "isolated",
            Self::OtherNetwork { .. } => "other_network",
            Self::NoNetwork => "no_network",
        }
    }

    /// The routes to offer next, best first (ADR-0007 "Avtomatik" order).
    #[must_use]
    pub fn next_routes(self) -> &'static [NextRoute] {
        match self {
            Self::LocalNetworkDenied => &[NextRoute::FixPermission, NextRoute::Relay],
            Self::Isolated { internet: true } | Self::OtherNetwork { internet: true } => {
                &[NextRoute::Relay, NextRoute::Hotspot, NextRoute::Cable]
            }
            Self::Isolated { internet: false }
            | Self::OtherNetwork { internet: false }
            | Self::NoNetwork => &[NextRoute::Hotspot, NextRoute::Cable],
        }
    }
}

/// A route the UI can offer after a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextRoute {
    /// Open the OS setting that gives Vutric the local network.
    FixPermission,
    /// Connect through the end-to-end encrypted relay (R2).
    Relay,
    /// One device opens a hotspot, the other joins from the QR (R3).
    Hotspot,
    /// USB or Ethernet cable (R5).
    Cable,
}

/// What a host saw while trying to reach a device on the LAN.
#[derive(Debug, Clone, Default)]
pub struct Evidence {
    pub locals: Vec<LocalAddr>,
    /// The other device's LAN hints (ticket or beacon).
    pub hints: Vec<SocketAddr>,
    /// The OS refused a send to a local address (see [`is_denied_error`]), or the host's own check (`NWBrowser`
    /// policy denied on iOS) said so.
    pub local_network_denied: bool,
    /// A beacon probe to one of the hints was answered: the LAN path works, the problem is elsewhere.
    pub probe_answered: bool,
    /// We have a home relay (internet works).
    pub internet: bool,
}

/// The verdict for a failed or relay-only connection.
#[must_use]
pub fn diagnose(e: &Evidence) -> Unreachable {
    if e.local_network_denied {
        return Unreachable::LocalNetworkDenied;
    }
    if e.locals.is_empty() {
        return Unreachable::NoNetwork;
    }
    let shared = e.hints.iter().any(|h| same_network(h.ip(), &e.locals));
    if shared && !e.probe_answered {
        Unreachable::Isolated {
            internet: e.internet,
        }
    } else {
        Unreachable::OtherNetwork {
            internet: e.internet,
        }
    }
}

/// [`diagnose`] for a pairing ticket on this machine right now.
#[must_use]
pub fn diagnose_ticket(t: &PairingTicket, probe_answered: bool, internet: bool) -> Unreachable {
    diagnose(&Evidence {
        locals: local_addrs(),
        hints: t.addrs.clone(),
        local_network_denied: false,
        probe_answered,
        internet,
    })
}

/// Whether an I/O error on a send to a LAN address means the OS denied local network access. Only iOS gives
/// apps this meaning for `EHOSTUNREACH` (elsewhere no route means no route).
#[must_use]
pub fn is_denied_error(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::PermissionDenied
        || (cfg!(target_os = "ios") && matches!(e.raw_os_error(), Some(1 | 13 | 65)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(ip: &str, prefix: u8, iface: u32, medium: Medium) -> LocalAddr {
        LocalAddr {
            ip: ip.parse().unwrap(),
            prefix,
            iface,
            iface_name: format!("if{iface}"),
            medium,
        }
    }

    #[test]
    fn classifies_common_adapters() {
        let v4: IpAddr = "192.168.1.5".parse().unwrap();
        assert_eq!(
            classify(&["vEthernet (WSL)"], true, false, v4),
            Medium::Virtual
        );
        assert_eq!(classify(&["Tailscale"], false, false, v4), Medium::Virtual);
        assert_eq!(classify(&["wg0"], false, false, v4), Medium::Virtual);
        assert_eq!(classify(&["Wi-Fi"], false, true, v4), Medium::WiFi);
        assert_eq!(classify(&["wlan0"], false, false, v4), Medium::WiFi);
        assert_eq!(classify(&["Ethernet 2"], true, false, v4), Medium::Wired);
        assert_eq!(classify(&["en0"], false, false, v4), Medium::Wired);
        assert_eq!(
            classify(&["Wi-Fi"], false, true, "172.20.10.3".parse().unwrap()),
            Medium::Tether
        );
        assert_eq!(classify(&["rndis0"], false, false, v4), Medium::Tether);
        assert_eq!(classify(&["foo"], false, false, v4), Medium::Other);
    }

    #[test]
    fn lan_scope() {
        for ok in [
            "10.0.0.1",
            "172.16.4.4",
            "192.168.0.9",
            "169.254.3.3",
            "100.64.0.1",
            "100.127.255.1",
            "fd12::1",
            "fe80::1",
        ] {
            assert!(is_lan_scope(ok.parse().unwrap()), "{ok}");
        }
        for no in ["8.8.8.8", "100.128.0.1", "2001:db8::1", "127.0.0.1", "::1"] {
            assert!(!is_lan_scope(no.parse().unwrap()), "{no}");
        }
    }

    #[test]
    fn ticket_ranks_physical_first_and_strips_zones() {
        let locals = vec![
            l("172.28.0.1", 20, 9, Medium::Virtual),
            l("192.168.1.20", 24, 3, Medium::WiFi),
            l("fe80::abcd", 64, 3, Medium::WiFi),
            l("10.0.0.5", 8, 2, Medium::Wired),
        ];
        let bound = [
            "172.28.0.1:5000",
            "8.8.8.8:5000",
            "[fe80::abcd%3]:5000",
            "192.168.1.20:5000",
            "10.0.0.5:5000",
            "127.0.0.1:5000",
        ]
        .map(|s| s.parse::<SocketAddr>().unwrap());
        let got = ticket_addrs(bound, &locals);
        let want: Vec<SocketAddr> = [
            "10.0.0.5:5000",
            "192.168.1.20:5000",
            "[fe80::abcd]:5000",
            "172.28.0.1:5000",
        ]
        .map(|s| s.parse().unwrap())
        .to_vec();
        assert_eq!(got, want);
        let t = PairingTicket {
            endpoint: vutric_core::EndpointId([1; 32]),
            device_name: "x".into(),
            addrs: got,
            relay: None,
        };
        assert_eq!(
            PairingTicket::parse(&t.to_uri()).unwrap(),
            t,
            "zone-less IPv6 fits the QR format"
        );
    }

    #[test]
    fn ticket_caps_at_eight() {
        let bound = (1..=20).map(|i| SocketAddr::from(([10, 0, 0, i], 1)));
        assert_eq!(ticket_addrs(bound, &[]).len(), MAX_HINTS);
    }

    #[test]
    fn link_local_gets_every_local_zone() {
        let locals = vec![
            l("fe80::1", 64, 4, Medium::Wired),
            l("fe80::2", 64, 7, Medium::WiFi),
            l("fe80::3", 64, 7, Medium::WiFi),
            l("192.168.1.2", 24, 7, Medium::WiFi),
        ];
        let hints =
            ["[fe80::99]:4000", "192.168.1.9:4000"].map(|s| s.parse::<SocketAddr>().unwrap());
        let got = expand_link_local(&hints, &locals);
        assert_eq!(got.len(), 3);
        assert!(
            got.iter()
                .any(|a| matches!(a, SocketAddr::V6(v) if v.scope_id() == 4))
        );
        assert!(
            got.iter()
                .any(|a| matches!(a, SocketAddr::V6(v) if v.scope_id() == 7))
        );
        assert!(got.contains(&"192.168.1.9:4000".parse().unwrap()));
    }

    #[test]
    fn same_network_by_prefix() {
        let locals = vec![
            l("192.168.1.20", 24, 1, Medium::WiFi),
            l("fe80::1", 64, 1, Medium::WiFi),
        ];
        assert!(same_network("192.168.1.200".parse().unwrap(), &locals));
        assert!(!same_network("192.168.2.1".parse().unwrap(), &locals));
        assert!(same_network("fe80::77".parse().unwrap(), &locals));
        assert!(!same_network("fd00::1".parse().unwrap(), &locals));
    }

    #[test]
    fn verdicts() {
        let locals = vec![l("192.168.1.20", 24, 1, Medium::WiFi)];
        let near: Vec<SocketAddr> = vec!["192.168.1.30:1".parse().unwrap()];
        let far: Vec<SocketAddr> = vec!["10.9.9.9:1".parse().unwrap()];
        let e = |hints: &Vec<SocketAddr>, probe, internet, denied| Evidence {
            locals: locals.clone(),
            hints: hints.clone(),
            local_network_denied: denied,
            probe_answered: probe,
            internet,
        };
        assert_eq!(
            diagnose(&e(&near, false, true, true)),
            Unreachable::LocalNetworkDenied
        );
        assert_eq!(
            diagnose(&e(&near, false, true, false)),
            Unreachable::Isolated { internet: true }
        );
        assert_eq!(
            diagnose(&e(&near, true, false, false)),
            Unreachable::OtherNetwork { internet: false }
        );
        assert_eq!(
            diagnose(&e(&far, false, true, false)),
            Unreachable::OtherNetwork { internet: true }
        );
        assert_eq!(diagnose(&Evidence::default()), Unreachable::NoNetwork);
        assert_eq!(
            Unreachable::Isolated { internet: false }.next_routes()[0],
            NextRoute::Hotspot
        );
        assert_eq!(
            Unreachable::Isolated { internet: true }.next_routes()[0],
            NextRoute::Relay
        );
        assert_eq!(
            Unreachable::LocalNetworkDenied.key(),
            "local_network_denied"
        );
    }

    #[test]
    fn this_machine_is_ranked() {
        // No assertion on content (CI machines differ): it must not panic and must come out ranked.
        let a = local_addrs();
        assert!(
            a.windows(2)
                .all(|w| (w[0].medium, addr_class(w[0].ip)) <= (w[1].medium, addr_class(w[1].ip)))
        );
    }
}
