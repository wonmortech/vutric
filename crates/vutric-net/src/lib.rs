//! Vutric transport (ADR-0005): every device is a peer, and the local network is enough.
//!
//! - [`Peer`] owns one iroh endpoint with relays and DNS lookups disabled, so nothing leaves the LAN.
//!   Other peers are found through mDNS (the device name rides along as mDNS user data) or through the IP hints
//!   of a [`PairingTicket`].
//! - [`Session`] is one control stream to one peer: `Hello` both ways, then framed [`Message`]s in either
//!   direction. "Sender" and "receiver" are per transfer, never per platform.
//! - [`Session::check_code`] is the pairing check shown on both screens. It is derived from this connection's TLS
//!   keying material, so an attacker in the middle would make the two screens disagree.

pub mod probe;

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use iroh::address_lookup::UserData;
use iroh::endpoint::{Connection, RecvStream, SendStream, presets};
use iroh::{Endpoint, EndpointAddr, RelayMode, SecretKey};
use iroh_mdns_address_lookup::{DiscoveryEvent, MdnsAddressLookup};
use n0_future::StreamExt as _;
use vutric_core::frame::{self, Decoder};
use vutric_core::{ALPN, EndpointId, Message, PROTOCOL_VERSION, PairingTicket, Platform};

/// mDNS service name; becomes `_vutric._udp.local.` on the wire (also what iOS will browse with `NWBrowser`).
pub const MDNS_SERVICE: &str = "vutric";

/// A device on the local network that can both send and receive.
#[derive(Debug, Clone)]
pub struct Peer {
    endpoint: Endpoint,
    /// `None` where the OS refused multicast (iOS without the multicast entitlement): link/QR pairing still works.
    mdns: Option<MdnsAddressLookup>,
    name: String,
    platform: Platform,
}

/// Another device seen on the network through mDNS.
#[derive(Debug, Clone)]
pub struct Nearby {
    pub id: EndpointId,
    pub name: Option<String>,
    pub addrs: Vec<SocketAddr>,
}

impl Peer {
    /// Binds a LAN-only endpoint. `secret` is the device's persistent identity (32 random bytes kept by the app).
    pub async fn bind(secret: [u8; 32], name: &str, platform: Platform) -> Result<Self> {
        let user_data: UserData = clip(name, UserData::MAX_LENGTH)
            .parse()
            .map_err(|e| anyhow!("device name for mDNS: {e:?}"))?;
        let endpoint = Endpoint::builder(presets::Minimal)
            .secret_key(SecretKey::from_bytes(&secret))
            .alpns(vec![ALPN.to_vec()])
            .relay_mode(RelayMode::Disabled)
            .user_data_for_address_lookup(user_data)
            .bind()
            .await
            .context("bind endpoint")?;
        let mdns = match MdnsAddressLookup::builder()
            .service_name(MDNS_SERVICE)
            .build(endpoint.id())
        {
            Ok(mdns) => {
                endpoint
                    .address_lookup()
                    .context("address lookup")?
                    .add(mdns.clone());
                Some(mdns)
            }
            Err(e) => {
                eprintln!("vutric-net: mDNS unavailable, pairing link only: {e:#}");
                None
            }
        };
        Ok(Self {
            endpoint,
            mdns,
            name: name.to_owned(),
            platform,
        })
    }

    #[must_use]
    pub fn id(&self) -> EndpointId {
        EndpointId(*self.endpoint.id().as_bytes())
    }

    /// The QR payload: our key plus every LAN address we listen on (for networks that drop multicast).
    #[must_use]
    pub fn ticket(&self) -> PairingTicket {
        let mut addrs: Vec<SocketAddr> = self
            .endpoint
            .addr()
            .ip_addrs()
            .copied()
            .filter(|a| is_lan(a.ip()))
            .collect();
        addrs.sort_by_key(|a| (a.is_ipv6(), *a));
        addrs.dedup();
        addrs.truncate(8);
        PairingTicket {
            endpoint: self.id(),
            device_name: self.name.clone(),
            addrs,
        }
    }

    /// Whether this peer can find and be found over mDNS (false: pairing link only).
    #[must_use]
    pub fn has_mdns(&self) -> bool {
        self.mdns.is_some()
    }

    /// Waits for the first other Vutric device announced over mDNS.
    pub async fn discover(&self, timeout: Duration) -> Result<Nearby> {
        let mdns = self
            .mdns
            .as_ref()
            .context("mDNS is unavailable on this device; pair with the QR code or link")?;
        let mut events = mdns.subscribe().await;
        let me = self.endpoint.id();
        tokio::time::timeout(timeout, async {
            while let Some(event) = events.next().await {
                if let DiscoveryEvent::Discovered { endpoint_info, .. } = event {
                    if endpoint_info.endpoint_id == me {
                        continue;
                    }
                    return Ok(Nearby {
                        id: EndpointId(*endpoint_info.endpoint_id.as_bytes()),
                        name: endpoint_info.user_data().map(|u| u.as_ref().to_owned()),
                        addrs: endpoint_info.ip_addrs().copied().collect(),
                    });
                }
            }
            bail!("mDNS stream ended")
        })
        .await
        .map_err(|_| anyhow!("no Vutric device found over mDNS in {timeout:?}"))?
    }

    /// Opens a session to a peer by key; `addrs` are optional hints (ticket or mDNS), mDNS fills in the rest.
    pub async fn connect(&self, id: EndpointId, addrs: &[SocketAddr]) -> Result<Session> {
        let key = iroh::PublicKey::from_bytes(&id.0).context("peer key")?;
        let mut target = EndpointAddr::new(key);
        for a in addrs {
            target = target.with_ip_addr(*a);
        }
        let conn = self
            .endpoint
            .connect(target, ALPN)
            .await
            .context("connect")?;
        let (send, recv) = conn.open_bi().await.context("open control stream")?;
        Session::handshake(conn, send, recv, &self.name, self.platform).await
    }

    /// Waits for the next peer to connect to us.
    pub async fn accept(&self) -> Result<Session> {
        let incoming = self
            .endpoint
            .accept()
            .await
            .ok_or_else(|| anyhow!("endpoint closed"))?;
        let conn = incoming.await.context("incoming connection")?;
        let (send, recv) = conn.accept_bi().await.context("accept control stream")?;
        Session::handshake(conn, send, recv, &self.name, self.platform).await
    }

    pub async fn close(&self) {
        self.endpoint.close().await;
    }
}

/// One control stream with one peer.
#[derive(Debug)]
pub struct Session {
    conn: Connection,
    send: SendStream,
    recv: RecvStream,
    decoder: Decoder,
    /// Read buffer, on the heap so session futures stay small.
    buf: Box<[u8]>,
    pub remote_name: String,
    pub remote_platform: Platform,
}

impl Session {
    async fn handshake(
        conn: Connection,
        send: SendStream,
        recv: RecvStream,
        name: &str,
        platform: Platform,
    ) -> Result<Self> {
        let mut s = Self {
            conn,
            send,
            recv,
            decoder: Decoder::new(),
            buf: vec![0u8; 16 * 1024].into_boxed_slice(),
            remote_name: String::new(),
            remote_platform: platform,
        };
        s.send(&Message::hello(clip(name, 64), platform)).await?;
        match s.recv().await? {
            Message::Hello {
                protocol,
                device_name,
                platform,
            } => {
                ensure!(
                    protocol == PROTOCOL_VERSION,
                    "peer speaks protocol {protocol}, we speak {PROTOCOL_VERSION}"
                );
                s.remote_name = device_name;
                s.remote_platform = platform;
                Ok(s)
            }
            other => bail!("expected Hello, got {other:?}"),
        }
    }

    #[must_use]
    pub fn remote_id(&self) -> EndpointId {
        EndpointId(*self.conn.remote_id().as_bytes())
    }

    pub async fn send(&mut self, msg: &Message) -> Result<()> {
        let bytes = frame::encode(msg)?;
        self.send.write_all(&bytes).await.context("write frame")?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<Message> {
        loop {
            if let Some(msg) = self.decoder.next_message()? {
                return Ok(msg);
            }
            let n = self
                .recv
                .read(&mut self.buf)
                .await
                .context("read frame")?
                .ok_or_else(|| anyhow!("peer closed the control stream"))?;
            self.decoder.push(&self.buf[..n]);
        }
    }

    /// Six digits both screens show while pairing (Bluetooth-style numeric comparison).
    pub fn check_code(&self) -> Result<String> {
        let mut out = [0u8; 8];
        self.conn
            .export_keying_material(&mut out, b"EXPORTER-vutric/1 pairing check", &[])
            .map_err(|e| anyhow!("keying material: {e:?}"))?;
        Ok(format!("{:06}", u64::from_le_bytes(out) % 1_000_000))
    }

    pub fn close(&self) {
        self.conn.close(0u32.into(), b"bye");
    }
}

/// A fresh device identity: 32 random bytes from the OS generator. The app stores it and reuses it forever.
#[must_use]
pub fn new_identity() -> [u8; 32] {
    SecretKey::generate().to_bytes()
}

/// Monotonic microseconds since `start`, the clock in `EffectStarted::receiver_micros`.
#[must_use]
pub fn micros_since(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
}

fn is_lan(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => !v6.is_loopback() && !v6.is_multicast() && !v6.is_unspecified(),
    }
}

fn clip(s: &str, max_bytes: usize) -> String {
    let mut end = s.len().min(max_bytes);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}
