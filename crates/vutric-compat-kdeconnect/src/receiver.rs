//! Receive mode: listen for KDE Connect devices, pair when the user says yes, and take files, texts and links
//! from paired devices only (spec: "all packets except pairing packets should be discarded" when unpaired).
//!
//! - TCP listener on the first free port of 1716–1764; UDP identity on 1716 when discovery is on.
//! - A UDP identity from a version-8 device makes us dial it (the spec's discovery order).
//! - Pairing: a request whose timestamp is within 30 minutes of our clock becomes [`Event::PairRequest`] with the
//!   verification key both screens show; no answer in time is a "no".
//! - A paired device whose certificate changed is dropped before any packet is read.
//! - Files are fetched into the staging folder as `.vutric-part` and renamed to a free, cleaned name when every
//!   byte has arrived.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::{Semaphore, mpsc, oneshot};

use crate::link::{Link, Me};
use crate::proto::{self, Identity, Packet, Pair, Share};
use crate::trust::{Trust, Verdict};
use crate::{payload, tls};

/// Standard UDP discovery port.
pub const UDP_PORT: u16 = 1716;
/// TCP ports tried for the listener.
pub const TCP_PORTS: std::ops::RangeInclusive<u16> = 1716..=1764;
/// Most connections at once.
pub const MAX_CONNECTIONS: usize = 16;
/// Pairing timestamps further than this from our clock are discarded (spec).
pub const PAIR_SKEW_SECS: u64 = 30 * 60;
/// A device heard over UDP is dialled at most this often.
const DIAL_EVERY: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct Config {
    pub me: Me,
    pub bind: IpAddr,
    pub tcp_ports: std::ops::RangeInclusive<u16>,
    /// `None`: no UDP discovery (tests, or the user turned it off).
    pub udp_port: Option<u16>,
    pub staging: PathBuf,
    pub trust: Trust,
    pub decide_timeout: Duration,
    /// Ports our payloads are offered on when we send ([`payload::PORTS`]; `0..=0` in tests).
    pub payload_ports: std::ops::RangeInclusive<u16>,
}

#[derive(Debug)]
pub enum Event {
    /// A device announced itself over UDP (for the device list); we dial it next.
    Seen {
        device_id: String,
        name: String,
        addr: SocketAddr,
    },
    /// Show both names and `key`; send `true` to pair.
    PairRequest {
        device_id: String,
        name: String,
        key: String,
        answer: oneshot::Sender<bool>,
    },
    Paired {
        device_id: String,
        name: String,
    },
    Unpaired {
        device_id: String,
    },
    File {
        device_id: String,
        name: String,
        path: PathBuf,
        size: u64,
    },
    Text {
        device_id: String,
        name: String,
        text: String,
    },
    Url {
        device_id: String,
        name: String,
        url: String,
    },
}

#[derive(Debug)]
struct Shared {
    me: Me,
    bind: IpAddr,
    payload_ports: std::ops::RangeInclusive<u16>,
    staging: PathBuf,
    trust: Mutex<Trust>,
    decide_timeout: Duration,
    events: mpsc::Sender<Event>,
    linked: Mutex<HashSet<String>>,
    dialled: Mutex<HashMap<String, Instant>>,
    slots: Arc<Semaphore>,
}

/// The running receiver. Dropping it stops listening (open transfers end with their connections).
#[derive(Debug)]
pub struct Receiver {
    shared: Arc<Shared>,
    addr: SocketAddr,
    udp: Option<Arc<UdpSocket>>,
    udp_port: u16,
    hello: Vec<u8>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for Receiver {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Receiver {
    pub async fn start(cfg: Config) -> Result<(Self, mpsc::Receiver<Event>)> {
        std::fs::create_dir_all(&cfg.staging).with_context(|| cfg.staging.display().to_string())?;
        let listener = payload::listen(cfg.bind, cfg.tcp_ports.clone())
            .await
            .context("tcp listener")?;
        let addr = listener.local_addr()?;
        let (events, rx) = mpsc::channel(16);
        let hello = Identity::udp(
            &cfg.me.tls.device_id,
            &cfg.me.name,
            cfg.me.kind,
            addr.port(),
        );
        let sh = Arc::new(Shared {
            me: cfg.me,
            bind: cfg.bind,
            payload_ports: cfg.payload_ports,
            staging: cfg.staging,
            trust: Mutex::new(cfg.trust),
            decide_timeout: cfg.decide_timeout,
            events,
            linked: Mutex::new(HashSet::new()),
            dialled: Mutex::new(HashMap::new()),
            slots: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
        });
        let mut tasks = vec![tokio::spawn(accept_loop(listener, Arc::clone(&sh)))];
        let (udp, udp_port) = match cfg.udp_port {
            None => (None, 0),
            Some(port) => {
                let sock = Arc::new(udp_socket(cfg.bind, port)?);
                let port = sock.local_addr()?.port();
                tasks.push(tokio::spawn(udp_loop(Arc::clone(&sock), Arc::clone(&sh))));
                (Some(sock), port)
            }
        };
        Ok((
            Self {
                shared: sh,
                addr,
                udp,
                udp_port,
                hello,
                tasks,
            },
            rx,
        ))
    }

    /// The TCP address devices connect to.
    #[must_use]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The UDP port we listen on (0 without discovery).
    #[must_use]
    pub fn udp_port(&self) -> u16 {
        self.udp_port
    }

    /// Sends files to a paired device at `to` (its TCP address, from [`Event::Seen`] or a stored address).
    /// Refuses a device that is not paired, or whose certificate changed. Returns the bytes sent.
    pub async fn send(&self, to: SocketAddr, device_id: &str, paths: &[PathBuf]) -> Result<u64> {
        let sh = &self.shared;
        let mut link = Link::dial(&sh.me, to, device_id).await?;
        let verdict = sh
            .trust
            .lock()
            .expect("lock")
            .check(device_id, &link.peer_cert);
        if verdict != Verdict::Paired {
            anyhow::bail!("device is not paired");
        }
        link.send_files(&sh.me, paths, sh.bind, sh.payload_ports.clone())
            .await?;
        let mut bytes = 0;
        for p in paths {
            bytes += tokio::fs::metadata(p).await?.len();
        }
        Ok(bytes)
    }

    /// Paired devices: id to name.
    #[must_use]
    pub fn paired(&self) -> Vec<(String, String)> {
        let t = self.shared.trust.lock().expect("lock");
        t.devices()
            .iter()
            .map(|(id, p)| (id.clone(), p.name.clone()))
            .collect()
    }

    /// Broadcasts our identity once on the LAN, so KDE Connect devices dial us.
    pub async fn announce(&self) {
        self.announce_to(SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), UDP_PORT))
            .await;
    }

    /// Sends our identity to one address (a known device, or a test).
    pub async fn announce_to(&self, to: SocketAddr) {
        if let Some(s) = &self.udp {
            let _ = s.send_to(&self.hello, to).await;
        }
    }
}

fn udp_socket(bind: IpAddr, port: u16) -> Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let s = Socket::new(
        Domain::for_address(SocketAddr::new(bind, port)),
        Type::DGRAM,
        Some(Protocol::UDP),
    )?;
    s.set_reuse_address(true)?;
    s.set_broadcast(true)?;
    s.set_nonblocking(true)?;
    s.bind(&SocketAddr::new(bind, port).into())
        .with_context(|| format!("bind udp {port}"))?;
    Ok(UdpSocket::from_std(s.into())?)
}

async fn accept_loop(listener: TcpListener, sh: Arc<Shared>) {
    loop {
        let Ok((tcp, from)) = listener.accept().await else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let Ok(permit) = Arc::clone(&sh.slots).try_acquire_owned() else {
            continue;
        };
        let sh = Arc::clone(&sh);
        tokio::spawn(async move {
            let _permit = permit;
            if let Ok(link) = Link::answer(&sh.me, tcp, from).await {
                serve(link, &sh).await;
            }
        });
    }
}

async fn udp_loop(sock: Arc<UdpSocket>, sh: Arc<Shared>) {
    let mut buf = vec![0u8; proto::MAX_PREAUTH_LINE + 1];
    loop {
        let Ok((n, from)) = sock.recv_from(&mut buf).await else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let line = buf[..n].strip_suffix(b"\n").unwrap_or(&buf[..n]);
        let Some(id) = Packet::parse(line, proto::MAX_PREAUTH_LINE)
            .as_ref()
            .and_then(Identity::from_packet)
        else {
            continue;
        };
        let Some(port) = id.tcp_port else { continue };
        if id.device_id == sh.me.tls.device_id || id.protocol_version != proto::PROTOCOL_VERSION {
            continue;
        }
        let seen = Event::Seen {
            device_id: id.device_id.clone(),
            name: id.device_name.clone(),
            addr: SocketAddr::new(from.ip(), port),
        };
        let _ = sh.events.try_send(seen);
        if sh.linked.lock().expect("lock").contains(&id.device_id) || !may_dial(&sh, &id.device_id)
        {
            continue;
        }
        let Ok(permit) = Arc::clone(&sh.slots).try_acquire_owned() else {
            continue;
        };
        let sh = Arc::clone(&sh);
        tokio::spawn(async move {
            let _permit = permit;
            if let Ok(link) =
                Link::dial(&sh.me, SocketAddr::new(from.ip(), port), &id.device_id).await
            {
                serve(link, &sh).await;
            }
        });
    }
}

fn may_dial(sh: &Shared, id: &str) -> bool {
    let now = Instant::now();
    let mut m = sh.dialled.lock().expect("lock");
    if m.len() > 1024 {
        m.retain(|_, t| now.duration_since(*t) < DIAL_EVERY);
    }
    match m.get(id) {
        Some(t) if now.duration_since(*t) < DIAL_EVERY => false,
        _ => {
            m.insert(id.to_owned(), now);
            true
        }
    }
}

/// Removes the device from the linked set when the connection ends, however it ends.
struct Linked<'a>(&'a Shared, String);

impl Drop for Linked<'_> {
    fn drop(&mut self) {
        self.0.linked.lock().expect("lock").remove(&self.1);
    }
}

async fn serve(mut link: Link, sh: &Shared) {
    let id = link.peer.device_id.clone();
    let name = link.peer.device_name.clone();
    if !sh.linked.lock().expect("lock").insert(id.clone()) {
        return;
    }
    let _linked = Linked(sh, id.clone());
    let verdict = sh.trust.lock().expect("lock").check(&id, &link.peer_cert);
    let mut paired = match verdict {
        Verdict::Paired => true,
        Verdict::Unknown => false,
        Verdict::Mismatch => return,
    };
    while let Ok(Some(p)) = link.next().await {
        if let Some(pair) = Pair::from_packet(&p) {
            if !on_pair(&mut link, sh, pair, &mut paired).await {
                return;
            }
            continue;
        }
        if !paired {
            continue;
        }
        let ev = match Share::from_packet(&p) {
            Some(Share::File {
                name: file,
                size,
                port,
                ..
            }) => match receive_file(&link, sh, &file, size, port).await {
                Ok(path) => Event::File {
                    device_id: id.clone(),
                    name: name.clone(),
                    path,
                    size,
                },
                Err(_) => continue,
            },
            Some(Share::Text(text)) => Event::Text {
                device_id: id.clone(),
                name: name.clone(),
                text,
            },
            Some(Share::Url(url)) => Event::Url {
                device_id: id.clone(),
                name: name.clone(),
                url,
            },
            None => continue,
        };
        if sh.events.send(ev).await.is_err() {
            return;
        }
    }
}

/// Handles one pairing packet; `false` ends the connection.
async fn on_pair(link: &mut Link, sh: &Shared, pair: Pair, paired: &mut bool) -> bool {
    let id = link.peer.device_id.clone();
    if !pair.pair {
        if *paired {
            *paired = false;
            let _ = sh.trust.lock().expect("lock").forget(&id);
            return sh
                .events
                .send(Event::Unpaired { device_id: id })
                .await
                .is_ok();
        }
        return true;
    }
    if *paired {
        return link.send(&Pair::answer(true)).await.is_ok();
    }
    let Some(ts) = pair.timestamp else {
        return link.send(&Pair::answer(false)).await.is_ok();
    };
    if now_secs().abs_diff(ts) > PAIR_SKEW_SECS {
        return true;
    }
    let Ok(key) = tls::verification_key(sh.me.tls.cert(), &link.peer_cert, ts) else {
        return false;
    };
    let (tx, rx) = oneshot::channel();
    let ask = Event::PairRequest {
        device_id: id.clone(),
        name: link.peer.device_name.clone(),
        key,
        answer: tx,
    };
    if sh.events.send(ask).await.is_err() {
        return false;
    }
    let yes = tokio::time::timeout(sh.decide_timeout, rx)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or(false);
    if link.send(&Pair::answer(yes)).await.is_err() {
        return false;
    }
    if yes {
        let pinned =
            sh.trust
                .lock()
                .expect("lock")
                .pin(&id, &link.peer.device_name, &link.peer_cert);
        if pinned.is_err() {
            return false;
        }
        *paired = true;
        let ev = Event::Paired {
            device_id: id,
            name: link.peer.device_name.clone(),
        };
        return sh.events.send(ev).await.is_ok();
    }
    true
}

async fn receive_file(
    link: &Link,
    sh: &Shared,
    name: &str,
    size: u64,
    port: u16,
) -> Result<PathBuf> {
    let staged = sh
        .staging
        .join(format!("{}.vutric-part", proto::new_device_id()));
    let got = payload::fetch(&sh.me, link.addr.ip(), port, &link.peer_cert, size, &staged).await;
    if let Err(e) = got {
        let _ = tokio::fs::remove_file(&staged).await;
        return Err(e);
    }
    let dest = free_name(&sh.staging, name);
    tokio::fs::rename(&staged, &dest).await?;
    Ok(dest)
}

/// `name` in `dir`, or `name (2)`, `name (3)`… when taken.
fn free_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    (2..10_000)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join(format!("{}{ext}", proto::new_device_id())))
}
