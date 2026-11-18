//! The beacon service (backlog R1): finds Vutric devices where mDNS does not get through, and probes whether a
//! LAN path to a device exists at all. Wire format: [`vutric_core::beacon`].
//!
//! - **Multicast** announce and query to `224.0.0.167:53319` on every IPv4 interface. Many access points and
//!   enterprise networks filter mDNS (5353) but still pass link-local multicast.
//! - **Probe**: a unicast query to the hint addresses of a ticket. An answer proves the LAN path works; silence
//!   with internet working is the AP-isolation signal of [`crate::lan::diagnose`].
//! - **Scan** (opt-in, only while the "add device" screen is open): a unicast query to every host of each local
//!   physical `/24`, paced, `LocalSend`'s HTTP scan fallback without HTTP.
//!
//! Every datagram that is not exactly one beacon is dropped unread. Answers are rate-limited per source.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use tokio::net::UdpSocket;
use tokio::sync::broadcast;
use vutric_core::EndpointId;
use vutric_core::beacon::{Beacon, GROUP, Kind, LEN, PORT};

use crate::lan::{self, Medium};

/// At most one answer per source address in this window.
const ANSWER_EVERY: Duration = Duration::from_millis(500);
/// Sources remembered for the rate limit.
const MAX_SOURCES: usize = 1024;
/// Pause between scan datagrams (254 hosts take about half a second).
const SCAN_PACE: Duration = Duration::from_millis(2);
/// Most hosts one scan touches.
pub const MAX_SCAN_HOSTS: usize = 1024;

/// A device heard on the LAN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Heard {
    pub id: EndpointId,
    /// Its QUIC address: the datagram's source IP and the announced port.
    pub addr: SocketAddr,
    pub phone: bool,
}

/// What the beacon learned about the network so far, for [`crate::lan::Evidence`] and the UI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Health {
    /// At least one multicast datagram left this device.
    pub multicast_sent: bool,
    /// Sends the OS refused with a "denied" error (iOS local network permission).
    pub denied_sends: u32,
    /// Datagrams from other Vutric devices received.
    pub heard: u32,
}

#[derive(Debug)]
struct Shared {
    me: EndpointId,
    quic_port: u16,
    phone: bool,
    answering: AtomicBool,
    multicast_sent: AtomicBool,
    denied: AtomicU32,
    heard: AtomicU32,
    answered: Mutex<HashMap<IpAddr, Instant>>,
    found: broadcast::Sender<Heard>,
}

/// A running beacon on one UDP port. Dropping it stops it.
#[derive(Debug)]
pub struct BeaconService {
    sock: Arc<UdpSocket>,
    shared: Arc<Shared>,
    port: u16,
    /// Interfaces the group was joined on (and multicasts go out of).
    ifaces: Vec<Ipv4Addr>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for BeaconService {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn bind(port: u16) -> Result<std::net::UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).context("beacon socket")?;
    s.set_reuse_address(true)?;
    s.set_nonblocking(true)?;
    s.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)).into())
        .with_context(|| format!("bind beacon port {port}"))?;
    Ok(s.into())
}

impl BeaconService {
    /// Starts on the standard port, inside a Tokio runtime. `quic_port` is the endpoint's port others should
    /// connect to.
    pub fn start(me: EndpointId, quic_port: u16, phone: bool) -> Result<Self> {
        Self::start_on(PORT, me, quic_port, phone)
    }

    /// Starts on `port` (tests use a free port; production uses [`PORT`]), inside a Tokio runtime.
    pub fn start_on(port: u16, me: EndpointId, quic_port: u16, phone: bool) -> Result<Self> {
        let sock = UdpSocket::from_std(bind(port)?)?;
        let port = sock.local_addr()?.port();
        let _ = sock.set_multicast_loop_v4(false);
        let mut ifaces = Vec::new();
        for a in lan::local_addrs() {
            if let IpAddr::V4(v4) = a.ip
                && !ifaces.contains(&v4)
                && sock.join_multicast_v4(GROUP, v4).is_ok()
            {
                ifaces.push(v4);
            }
        }
        let (found, _) = broadcast::channel(64);
        let shared = Arc::new(Shared {
            me,
            quic_port,
            phone,
            answering: AtomicBool::new(true),
            multicast_sent: AtomicBool::new(false),
            denied: AtomicU32::new(0),
            heard: AtomicU32::new(0),
            answered: Mutex::new(HashMap::new()),
            found,
        });
        let sock = Arc::new(sock);
        let task = tokio::spawn(listen(Arc::clone(&sock), Arc::clone(&shared)));
        Ok(Self {
            sock,
            shared,
            port,
            ifaces,
            task,
        })
    }

    /// The UDP port this beacon listens on.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Devices heard from now on.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Heard> {
        self.shared.found.subscribe()
    }

    /// Whether queries are answered. Off while the device is not visible (ADR-0050 `Visibility::Off`).
    pub fn set_answering(&self, on: bool) {
        self.shared.answering.store(on, Ordering::Relaxed);
    }

    #[must_use]
    pub fn health(&self) -> Health {
        Health {
            multicast_sent: self.shared.multicast_sent.load(Ordering::Relaxed),
            denied_sends: self.shared.denied.load(Ordering::Relaxed),
            heard: self.shared.heard.load(Ordering::Relaxed),
        }
    }

    fn beacon(&self, kind: Kind) -> [u8; LEN] {
        Beacon {
            kind,
            id: self.shared.me,
            port: self.shared.quic_port,
            flags: u8::from(self.shared.phone),
        }
        .encode()
    }

    async fn send(&self, bytes: &[u8], to: SocketAddr) -> bool {
        match self.sock.send_to(bytes, to).await {
            Ok(_) => true,
            Err(e) => {
                if lan::is_denied_error(&e) {
                    self.shared.denied.fetch_add(1, Ordering::Relaxed);
                }
                false
            }
        }
    }

    async fn multicast(&self, kind: Kind) {
        let bytes = self.beacon(kind);
        let to = SocketAddr::V4(SocketAddrV4::new(GROUP, self.port));
        let sref = socket2::SockRef::from(&*self.sock);
        for ifc in &self.ifaces {
            if sref.set_multicast_if_v4(ifc).is_ok() && self.send(&bytes, to).await {
                self.shared.multicast_sent.store(true, Ordering::Relaxed);
            }
        }
    }

    /// "I am here" to the group on every interface.
    pub async fn announce(&self) {
        self.multicast(Kind::Announce).await;
    }

    /// "Who is here?" to the group; answers arrive through [`Self::subscribe`].
    pub async fn query(&self) {
        self.multicast(Kind::Query).await;
    }

    /// Unicast query to each address (a ticket's hints, say): an answer means the LAN path to it works.
    pub async fn probe(&self, ips: &[IpAddr]) {
        let bytes = self.beacon(Kind::Query);
        for ip in ips.iter().filter(|ip| ip.is_ipv4()) {
            self.send(&bytes, SocketAddr::new(*ip, self.port)).await;
        }
    }

    /// Unicast query to one beacon address (any port): for a device that announced a non-standard beacon port,
    /// and for tests.
    pub async fn query_to(&self, to: SocketAddr) -> bool {
        self.send(&self.beacon(Kind::Query), to).await
    }

    /// Unicast query to every host of each local physical IPv4 `/24` (a wider subnet is cut to the `/24` around
    /// our address), at most [`MAX_SCAN_HOSTS`]. Returns how many datagrams went out. Opt-in only.
    pub async fn scan(&self) -> usize {
        let targets = scan_targets(&lan::local_addrs(), MAX_SCAN_HOSTS);
        let bytes = self.beacon(Kind::Query);
        let mut sent = 0;
        for ip in targets {
            if self
                .send(&bytes, SocketAddr::new(IpAddr::V4(ip), self.port))
                .await
            {
                sent += 1;
            }
            tokio::time::sleep(SCAN_PACE).await;
        }
        sent
    }
}

/// The hosts a scan queries: every other address of each physical IPv4 `/24` we are on.
#[must_use]
pub fn scan_targets(locals: &[lan::LocalAddr], max: usize) -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    for l in locals {
        let IpAddr::V4(me) = l.ip else { continue };
        if l.medium == Medium::Virtual || !me.is_private() || l.prefix > 30 {
            continue;
        }
        let o = me.octets();
        for host in 1..=254u8 {
            let ip = Ipv4Addr::new(o[0], o[1], o[2], host);
            if ip != me && !out.contains(&ip) {
                out.push(ip);
            }
            if out.len() >= max {
                return out;
            }
        }
    }
    out
}

async fn listen(sock: Arc<UdpSocket>, sh: Arc<Shared>) {
    let mut buf = [0u8; LEN + 1];
    loop {
        let Ok((n, from)) = sock.recv_from(&mut buf).await else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let Some(b) = Beacon::decode(&buf[..n]) else {
            continue;
        };
        if b.id == sh.me {
            continue;
        }
        sh.heard.fetch_add(1, Ordering::Relaxed);
        match b.kind {
            Kind::Announce => {
                let _ = sh.found.send(Heard {
                    id: b.id,
                    addr: SocketAddr::new(from.ip(), b.port),
                    phone: b.flags & 1 == 1,
                });
            }
            Kind::Query => {
                // The querier is a device too: remember it, then answer once per window.
                let _ = sh.found.send(Heard {
                    id: b.id,
                    addr: SocketAddr::new(from.ip(), b.port),
                    phone: b.flags & 1 == 1,
                });
                if !sh.answering.load(Ordering::Relaxed) || !may_answer(&sh, from.ip()) {
                    continue;
                }
                let reply = Beacon {
                    kind: Kind::Announce,
                    id: sh.me,
                    port: sh.quic_port,
                    flags: u8::from(sh.phone),
                }
                .encode();
                if let Err(e) = sock.send_to(&reply, from).await
                    && lan::is_denied_error(&e)
                {
                    sh.denied.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

fn may_answer(sh: &Shared, ip: IpAddr) -> bool {
    let now = Instant::now();
    let mut seen = sh.answered.lock().expect("beacon lock");
    if seen.len() >= MAX_SOURCES {
        seen.retain(|_, t| now.duration_since(*t) < ANSWER_EVERY);
        if seen.len() >= MAX_SOURCES {
            return false;
        }
    }
    match seen.get(&ip) {
        Some(t) if now.duration_since(*t) < ANSWER_EVERY => false,
        _ => {
            seen.insert(ip, now);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_covers_physical_slash_24_only() {
        let l = |ip: &str, prefix, medium| lan::LocalAddr {
            ip: ip.parse().unwrap(),
            prefix,
            iface: 1,
            iface_name: "x".into(),
            medium,
        };
        let t = scan_targets(
            &[
                l("192.168.1.20", 24, Medium::WiFi),
                l("10.0.5.9", 8, Medium::Wired),
                l("172.28.0.1", 20, Medium::Virtual),
                l("fe80::1", 64, Medium::WiFi),
            ],
            MAX_SCAN_HOSTS,
        );
        assert_eq!(t.len(), 253 * 2);
        assert!(!t.contains(&"192.168.1.20".parse().unwrap()));
        assert!(t.contains(&"10.0.5.1".parse().unwrap()));
        assert!(!t.iter().any(|ip| ip.octets()[0] == 172));
        assert_eq!(
            scan_targets(&[l("192.168.1.20", 24, Medium::WiFi)], 10).len(),
            10
        );
    }
}
