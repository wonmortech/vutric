//! Multicast discovery (spec §3.1): announce ourselves on `224.0.0.167:53317`, hear others, and answer an
//! announcement first with `register` over HTTPS, then (if that fails) with a multicast answer.
//!
//! Visible only while receive mode is on (ADR-0010 §3.3): the caller starts this with the receiver and drops it
//! when receive mode ends.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::client::{self, Target};
use crate::proto::{self, DeviceInfo, Scheme};
use crate::tls::Identity;

/// A `LocalSend` device heard on the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub info: DeviceInfo,
    pub target: Target,
}

/// Running discovery. Dropping it stops it.
#[derive(Debug)]
pub struct Discovery {
    sock: Arc<UdpSocket>,
    us: DeviceInfo,
    group: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Discovery {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn bind(port: u16) -> Result<std::net::UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    s.set_reuse_address(true)?;
    s.set_nonblocking(true)?;
    s.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)).into())
        .with_context(|| format!("bind LocalSend multicast port {port}"))?;
    Ok(s.into())
}

impl Discovery {
    /// Joins the group on `port` (production: [`proto::PORT`]) on each of `ifaces` (empty: the default
    /// interface). `us` is our receiver description (its fingerprint filters out our own echoes).
    pub fn start(
        port: u16,
        ifaces: &[Ipv4Addr],
        us: DeviceInfo,
        identity: Identity,
    ) -> Result<(Self, mpsc::Receiver<Found>)> {
        let sock = UdpSocket::from_std(bind(port)?)?;
        let port = sock.local_addr()?.port();
        if ifaces.is_empty() {
            sock.join_multicast_v4(proto::MULTICAST, Ipv4Addr::UNSPECIFIED)
                .context("join LocalSend group")?;
        }
        for i in ifaces {
            let _ = sock.join_multicast_v4(proto::MULTICAST, *i);
        }
        let sock = Arc::new(sock);
        let (tx, rx) = mpsc::channel(32);
        let task = tokio::spawn(listen(Arc::clone(&sock), us.clone(), identity, tx));
        Ok((
            Self {
                sock,
                us,
                group: SocketAddr::from((proto::MULTICAST, port)),
                task,
            },
            rx,
        ))
    }

    /// "I am here, answer me" to the group.
    pub async fn announce(&self) -> Result<()> {
        let mut a = self.us.clone();
        a.announce = Some(true);
        self.sock
            .send_to(&serde_json::to_vec(&a)?, self.group)
            .await?;
        Ok(())
    }
}

async fn listen(sock: Arc<UdpSocket>, us: DeviceInfo, id: Identity, tx: mpsc::Sender<Found>) {
    let mut buf = vec![0u8; proto::MAX_DATAGRAM + 1];
    loop {
        let Ok((n, from)) = sock.recv_from(&mut buf).await else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let Ok(info) = proto::parse_announcement(&buf[..n]) else {
            continue;
        };
        if info.fingerprint == us.fingerprint {
            continue; // our own echo
        }
        let target = Target {
            addr: SocketAddr::new(from.ip(), info.port.unwrap_or(proto::PORT)),
            https: info.protocol != Some(Scheme::Http),
        };
        let answer_wanted = info.announce == Some(true);
        let _ = tx.try_send(Found {
            info: info.clone(),
            target,
        });
        if answer_wanted {
            let (sock, us, id) = (Arc::clone(&sock), us.clone(), id.clone());
            tokio::spawn(async move {
                if client::register(&id, target, &us).await.is_err() {
                    let mut a = us;
                    a.announce = Some(false);
                    if let Ok(b) = serde_json::to_vec(&a) {
                        let group = SocketAddr::new(IpAddr::V4(proto::MULTICAST), from.port());
                        let _ = sock.send_to(&b, group).await;
                    }
                }
            });
        }
    }
}
