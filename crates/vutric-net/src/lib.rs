//! Vutric transport (ADR-0005, ADR-0006): every device is a peer; the local network comes first, the internet
//! is the fallback.
//!
//! - [`Peer`] owns one iroh endpoint. With [`Reach::Lan`] relays and lookup servers are off and nothing leaves
//!   the LAN. With [`Reach::Anywhere`] the same LAN paths are tried first, and a device on another network is
//!   reached by its key alone: a direct path by hole punching, else through an end-to-end encrypted relay.
//!   Peers are found through mDNS (the device name rides along as mDNS user data, never to a public server),
//!   the IP hints and relay of a [`PairingTicket`], or the n0 lookup server (by key, relay address only).
//! - [`Session`] is one control stream to one peer: `Hello` both ways, then framed [`Message`]s in either
//!   direction. "Sender" and "receiver" are per transfer, never per platform.
//! - [`Session::check_code`] is the pairing check shown on both screens, derived from this connection's TLS
//!   keying material. It catches a passive mix-up (wrong device), but six digits without a commitment can be
//!   ground by an active attacker who answers mDNS; only QR/link pairing (which pins the peer's key) is strong.
//!   Phase 1 adds a trust store of paired keys and commit-then-reveal numeric comparison (see the 0.2 report).
//!
//! Every network wait is bounded ([`HANDSHAKE_TIMEOUT`], [`probe::ROUND_TRIP_TIMEOUT`]), so a silent peer can
//! stall a caller for seconds, never forever.

pub mod beacon;
pub mod disk;
pub mod lan;
pub mod preview;
pub mod probe;
pub mod settings;
pub mod transfer;
pub mod trust;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use iroh::address_lookup::{
    AddressLookup, AddressLookupBuilder, AddressLookupBuilderError, DnsAddressLookup, EndpointData,
    Error as LookupError, Item, PkarrPublisher, PkarrResolver, UserData,
};
use iroh::endpoint::{Connection, RecvStream, SendStream, presets};
use iroh::{Endpoint, EndpointAddr, RelayMode, RelayUrl, SecretKey};
use iroh_mdns_address_lookup::{DiscoveryEvent, MdnsAddressLookup};
use n0_future::StreamExt as _;
use n0_future::boxed::BoxStream;
use vutric_core::frame::{self, Decoder};
pub use vutric_core::trust::Admission;
use vutric_core::{ALPN, EndpointId, Message, PROTOCOL_VERSION, PairingTicket, Platform};

/// Prints a line without ever blocking the caller: a Windows console in selection mode (`QuickEdit`) freezes
/// every write, which must not freeze the network. Lines go to one printer thread; if it is stuck and 1024
/// lines are queued, newer lines are dropped.
pub fn say(line: String) {
    static TX: std::sync::OnceLock<std::sync::mpsc::SyncSender<String>> =
        std::sync::OnceLock::new();
    let tx = TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::sync_channel::<String>(1024);
        let _ = std::thread::Builder::new()
            .name("vutric-console".into())
            .spawn(move || {
                for l in rx {
                    println!("{l}");
                }
            });
        tx
    });
    let _ = tx.try_send(line);
}

/// mDNS service name; becomes `_vutric._udp.local.` on the wire (also what iOS will browse with `NWBrowser`).
pub const MDNS_SERVICE: &str = "vutric";

/// Longest wait for QUIC, the control stream and `Hello` when a session opens, either direction.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(8);

/// Longest wait for each machine step of [`Session::pair_code`].
pub const PAIR_STEP_TIMEOUT: Duration = Duration::from_secs(8);

/// Longest wait for the other user to compare the digits in [`Session::pair_finish`].
pub const PAIR_CONFIRM_TIMEOUT: Duration = Duration::from_secs(120);

/// How long a closing side waits for the peer to take its last bytes before it closes anyway.
const LINGER: Duration = Duration::from_secs(2);

/// How far a [`Peer`] reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// The local network only: no relay, no lookup server, nothing leaves the LAN (the "local only" setting,
    /// and tests).
    Lan,
    /// The local network first, then the internet: a direct path by hole punching, else an end-to-end
    /// encrypted relay. The relay and the lookup server see keys and encrypted packets, never content or names.
    Anywhere,
}

/// How a session's packets travel right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// Straight to the other device (same network, or a hole-punched internet path).
    Direct,
    /// Through a relay server (end-to-end encrypted); iroh keeps trying to upgrade to a direct path.
    Relay,
    /// No path yet.
    Unknown,
}

/// Publishes through `B` without the device name: the name is for nearby devices (mDNS), not for a public
/// lookup server.
#[derive(Debug)]
struct Nameless<B>(B);

#[derive(Debug)]
struct NamelessLookup<L>(L);

impl<B: AddressLookupBuilder> AddressLookupBuilder for Nameless<B> {
    fn into_address_lookup(
        self,
        endpoint: &Endpoint,
    ) -> Result<impl AddressLookup, AddressLookupBuilderError> {
        Ok(NamelessLookup(self.0.into_address_lookup(endpoint)?))
    }
}

impl<L: AddressLookup> AddressLookup for NamelessLookup<L> {
    fn publish(&self, data: &EndpointData) {
        let mut data = data.clone();
        data.set_user_data(None);
        self.0.publish(&data);
    }

    fn resolve(
        &self,
        endpoint_id: iroh::EndpointId,
    ) -> Option<BoxStream<std::result::Result<Item, LookupError>>> {
        self.0.resolve(endpoint_id)
    }
}

/// A device that can both send and receive.
#[derive(Debug, Clone)]
pub struct Peer {
    endpoint: Endpoint,
    reach: Reach,
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
    /// Binds an endpoint. `secret` is the device's persistent identity (32 random bytes kept by the app).
    pub async fn bind(
        secret: [u8; 32],
        name: &str,
        platform: Platform,
        reach: Reach,
    ) -> Result<Self> {
        let user_data: UserData = clip(name, UserData::MAX_LENGTH)
            .parse()
            .map_err(|e| anyhow!("device name for mDNS: {e:?}"))?;
        let builder = match reach {
            Reach::Lan => Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Disabled),
            // presets::N0 minus its publisher, which would put the device name on a public server.
            Reach::Anywhere => Endpoint::builder(presets::Minimal)
                .relay_mode(RelayMode::Default)
                .address_lookup(Nameless(PkarrPublisher::n0_dns()))
                .address_lookup(PkarrResolver::n0_dns())
                .address_lookup(DnsAddressLookup::n0_dns()),
        };
        let endpoint = builder
            .secret_key(SecretKey::from_bytes(&secret))
            .alpns(vec![ALPN.to_vec()])
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
                say(format!(
                    "vutric-net: mDNS unavailable, pairing link only: {e:#}"
                ));
                None
            }
        };
        Ok(Self {
            endpoint,
            reach,
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
        // Ranked by interface (wired, Wi-Fi, tethering, virtual last); link-local IPv6 goes in without its zone and
        // the dialer expands it to its own interfaces (`lan::expand_link_local`).
        let addrs = lan::ticket_addrs(
            self.endpoint.addr().ip_addrs().copied(),
            &lan::local_addrs(),
        );
        PairingTicket {
            endpoint: self.id(),
            device_name: self.name.clone(),
            addrs,
            relay: self
                .endpoint
                .addr()
                .relay_urls()
                .next()
                .map(ToString::to_string),
        }
    }

    #[must_use]
    pub fn reach(&self) -> Reach {
        self.reach
    }

    /// [`Reach::Anywhere`]: waits up to `max` for a home relay, so the next [`Self::ticket`] carries it. Returns
    /// whether we are online; without internet it returns `false` after `max` and the LAN still works.
    pub async fn wait_online(&self, max: Duration) -> bool {
        self.reach == Reach::Anywhere
            && tokio::time::timeout(max, self.endpoint.online())
                .await
                .is_ok()
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
        self.dial(id, addrs, None).await
    }

    /// Opens a session from a pairing link: its LAN hints first, its relay (and, with [`Reach::Anywhere`], the
    /// lookup server) when the device is on another network.
    pub async fn connect_ticket(&self, t: &PairingTicket) -> Result<Session> {
        self.dial(t.endpoint, &t.addrs, t.relay.as_deref()).await
    }

    async fn dial(
        &self,
        id: EndpointId,
        addrs: &[SocketAddr],
        relay: Option<&str>,
    ) -> Result<Session> {
        let key = iroh::PublicKey::from_bytes(&id.0).context("peer key")?;
        let mut target = EndpointAddr::new(key);
        for a in lan::expand_link_local(addrs, &lan::local_addrs()) {
            target = target.with_ip_addr(a);
        }
        if self.reach == Reach::Anywhere
            && let Some(url) = relay
        {
            target = target.with_relay_url(url.parse::<RelayUrl>().context("relay url")?);
        }
        tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
            let conn = self
                .endpoint
                .connect(target, ALPN)
                .await
                .context("connect")?;
            let opened = async {
                let (send, recv) = conn.open_bi().await.context("open control stream")?;
                Session::handshake(conn.clone(), send, recv, &self.name, self.platform, true).await
            }
            .await;
            match opened {
                Err(_) if refused(&conn) => Err(NotAdmitted.into()),
                other => other,
            }
        })
        .await
        .map_err(|_| anyhow!("no answer from the device within {HANDSHAKE_TIMEOUT:?}"))?
    }

    /// Like [`Self::accept`], but asks `admit` about each peer's key as soon as QUIC has authenticated it, before
    /// a single byte from the peer is parsed (ADR-0007 part 2). A refused peer's connection is closed with
    /// [`REFUSED_CODE`], and its dialer gets [`NotAdmitted`]. Returns the session and the admission it got.
    ///
    /// Hosts build `admit` from [`vutric_core::trust::admit`] with their trust store, visibility and clock.
    pub async fn accept_with<F>(&self, admit: F) -> Result<(Session, Admission)>
    where
        F: Fn(&EndpointId) -> Admission + Send + Sync + 'static,
    {
        let admit = std::sync::Arc::new(admit);
        let mut pending = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                incoming = self.endpoint.accept() => {
                    let incoming = incoming.ok_or_else(|| anyhow!("endpoint closed"))?;
                    let (name, platform, admit) = (self.name.clone(), self.platform, admit.clone());
                    pending.spawn(async move {
                        tokio::time::timeout(HANDSHAKE_TIMEOUT, async move {
                            let conn = incoming.await.context("incoming connection")?;
                            let peer = EndpointId(*conn.remote_id().as_bytes());
                            let admission = admit(&peer);
                            if admission == Admission::Refuse {
                                conn.close(REFUSED_CODE.into(), b"not trusted");
                                bail!("refused {}: not a trusted device", short_id(&peer));
                            }
                            let (send, recv) = conn.accept_bi().await.context("accept control stream")?;
                            let s = Session::handshake(conn, send, recv, &name, platform, false).await?;
                            Ok((s, admission))
                        })
                        .await
                    });
                }
                Some(done) = pending.join_next(), if !pending.is_empty() => match done {
                    Ok(Ok(Ok(s))) => return Ok(s),
                    Ok(Ok(Err(e))) => say(format!("vutric-net: incoming connection dropped: {e:#}")),
                    Ok(Err(_)) => say("vutric-net: incoming connection stalled its handshake, dropped".into()),
                    Err(e) => say(format!("vutric-net: handshake task failed: {e}")),
                },
            }
        }
    }

    /// Waits for the next peer to connect to us and returns the first one whose handshake completes.
    /// Handshakes run side by side, each bounded by [`HANDSHAKE_TIMEOUT`]: a client that fails or stalls is
    /// logged and dropped without delaying anyone else. Handshakes still running when one wins are dropped.
    pub async fn accept(&self) -> Result<Session> {
        let mut pending = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                incoming = self.endpoint.accept() => {
                    let incoming = incoming.ok_or_else(|| anyhow!("endpoint closed"))?;
                    let (name, platform) = (self.name.clone(), self.platform);
                    pending.spawn(async move {
                        tokio::time::timeout(HANDSHAKE_TIMEOUT, async move {
                            let conn = incoming.await.context("incoming connection")?;
                            let (send, recv) = conn.accept_bi().await.context("accept control stream")?;
                            Session::handshake(conn, send, recv, &name, platform, false).await
                        })
                        .await
                    });
                }
                Some(done) = pending.join_next(), if !pending.is_empty() => match done {
                    Ok(Ok(Ok(s))) => return Ok(s),
                    Ok(Ok(Err(e))) => say(format!("vutric-net: incoming connection dropped: {e:#}")),
                    Ok(Err(_)) => say("vutric-net: incoming connection stalled its handshake, dropped".into()),
                    Err(e) => say(format!("vutric-net: handshake task failed: {e}")),
                },
            }
        }
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
    /// We opened the connection (the dialer); decides the role in [`Session::pair_code`].
    dialer: bool,
}

/// QUIC application close code for "you are not a trusted device, and this device is not visible to everyone".
pub const REFUSED_CODE: u32 = 403;

/// The other device refused us before the handshake: we are not in its trust list and it is not in "everyone"
/// or pairing mode. Find it with `err.downcast_ref::<NotAdmitted>()`. The UI says: pair first, or ask the other
/// side to open "Hamma" for 10 minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotAdmitted;

impl std::fmt::Display for NotAdmitted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the other device accepts only its trusted devices right now")
    }
}

impl std::error::Error for NotAdmitted {}

/// The six digits of [`Session::pair_code`] did not come out of an honest exchange: the reveal did not open the
/// commitment, or the peer broke the order. Treat as an attack; never offer to trust this peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairingTampered;

impl std::fmt::Display for PairingTampered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("pairing check failed: the other device did not follow the protocol")
    }
}

impl std::error::Error for PairingTampered {}

fn refused(conn: &Connection) -> bool {
    matches!(
        conn.close_reason(),
        Some(iroh::endpoint::ConnectionError::ApplicationClosed(c))
            if c.error_code.into_inner() == u64::from(REFUSED_CODE)
    )
}

fn short_id(id: &EndpointId) -> String {
    id.to_string()[..8].to_owned()
}

impl Session {
    async fn handshake(
        conn: Connection,
        send: SendStream,
        recv: RecvStream,
        name: &str,
        platform: Platform,
        dialer: bool,
    ) -> Result<Self> {
        let mut s = Self {
            conn,
            send,
            recv,
            decoder: Decoder::new(),
            buf: vec![0u8; 16 * 1024].into_boxed_slice(),
            remote_name: String::new(),
            remote_platform: platform,
            dialer,
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
                s.remote_name = display_name(&device_name);
                s.remote_platform = platform;
                Ok(s)
            }
            other => bail!("expected Hello, got {}", other.kind()),
        }
    }

    #[must_use]
    pub fn remote_id(&self) -> EndpointId {
        EndpointId(*self.conn.remote_id().as_bytes())
    }

    /// Sends one message. It is validated first, so an oversized preview fails here, on the sender, instead of
    /// killing the session on the receiver.
    pub async fn send(&mut self, msg: &Message) -> Result<()> {
        msg.validate()?;
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

    /// How this session's packets travel right now, and the round trip of that path.
    #[must_use]
    pub fn route(&self) -> (Route, Option<Duration>) {
        let paths = self.conn.paths();
        let Some(p) = paths.iter().find(iroh::endpoint::Path::is_selected) else {
            return (Route::Unknown, None);
        };
        let route = if p.is_relay() {
            Route::Relay
        } else {
            Route::Direct
        };
        (route, Some(p.rtt()))
    }

    /// Six digits both screens show while pairing (Bluetooth-style numeric comparison).
    pub fn check_code(&self) -> Result<String> {
        let mut out = [0u8; 8];
        self.conn
            .export_keying_material(&mut out, b"EXPORTER-vutric/1 pairing check", &[])
            .map_err(|e| anyhow!("keying material: {e:?}"))?;
        Ok(format!("{:06}", u64::from_le_bytes(out) % 1_000_000))
    }

    /// Whether we opened this connection.
    #[must_use]
    pub fn is_dialer(&self) -> bool {
        self.dialer
    }

    /// Pairing without a QR code (backlog S2): the commit-then-reveal exchange of [`vutric_core::sas`], run on
    /// this session by both sides at once (each side's role follows who dialed). Returns the six digits to show.
    /// The users compare them, and each side then calls [`Self::pair_finish`]. Errors with [`PairingTampered`]
    /// when the peer's reveal does not match its commitment.
    pub async fn pair_code(&mut self) -> Result<String> {
        use vutric_core::sas;
        let mut x = [0u8; sas::EXPORTER_BYTES];
        self.conn
            .export_keying_material(&mut x, sas::EXPORTER_LABEL, &[])
            .map_err(|e| anyhow!("keying material: {e:?}"))?;
        let mine: [u8; sas::NONCE_BYTES] = new_identity();
        if self.dialer {
            self.send(&Message::PairCommit {
                commit: sas::commitment(&x, &mine),
            })
            .await?;
            let Message::PairNonce { nonce: theirs } = self.recv_step().await? else {
                return Err(PairingTampered.into());
            };
            self.send(&Message::PairReveal { nonce: mine }).await?;
            Ok(sas::code(&x, &mine, &theirs))
        } else {
            let Message::PairCommit { commit } = self.recv_step().await? else {
                return Err(PairingTampered.into());
            };
            self.send(&Message::PairNonce { nonce: mine }).await?;
            let theirs = match self.recv_step().await? {
                Message::PairReveal { nonce } if sas::opens(&commit, &x, &nonce) => nonce,
                _ => return Err(PairingTampered.into()),
            };
            Ok(sas::code(&x, &theirs, &mine))
        }
    }

    async fn recv_step(&mut self) -> Result<Message> {
        tokio::time::timeout(PAIR_STEP_TIMEOUT, self.recv())
            .await
            .map_err(|_| anyhow!("the other device stopped answering the pairing check"))?
    }

    /// Sends this user's answer ("the digits match") and waits for the other user's. Returns true only when
    /// both confirmed; only then may the host store the peer in its trust list with
    /// [`vutric_core::trust::PairMethod::Code`]. Waits up to [`PAIR_CONFIRM_TIMEOUT`] for the other user.
    /// To end the session after a "no", call [`Self::finish`], not [`Self::close`], so the answer reaches the
    /// other side before the connection goes.
    pub async fn pair_finish(&mut self, confirmed: bool) -> Result<bool> {
        self.send(&Message::PairResult { confirmed }).await?;
        let theirs = tokio::time::timeout(PAIR_CONFIRM_TIMEOUT, self.recv())
            .await
            .map_err(|_| anyhow!("the other user did not answer the pairing check"))??;
        match theirs {
            Message::PairResult { confirmed: other } => Ok(confirmed && other),
            _ => Err(PairingTampered.into()),
        }
    }

    /// Closes at once. Bytes still in flight may be lost; after a final send use [`Session::finish`].
    pub fn close(&self) {
        self.conn.close(0u32.into(), b"bye");
    }

    /// Closes after the peer has taken everything we sent (or after a short linger), so a last answer is not
    /// dropped by the close.
    pub async fn finish(&mut self) {
        if self.send.finish().is_ok() {
            let _ = tokio::time::timeout(LINGER, self.send.stopped()).await;
        }
        self.close();
    }
}

/// A peer-supplied device name made safe to print and show: no control characters, at most 64 bytes.
fn display_name(raw: &str) -> String {
    // Control characters and bidi overrides (a name must not reorder the text shown around it).
    let clean: String = raw
        .chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(c, '\u{200E}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        })
        .collect();
    let clean = clip(clean.trim(), 64);
    if clean.is_empty() {
        "unnamed device".into()
    } else {
        clean
    }
}

/// A fresh device identity: 32 random bytes from the OS generator. The app stores it and reuses it forever.
#[must_use]
pub fn new_identity() -> [u8; 32] {
    SecretKey::generate().to_bytes()
}

/// The device identity kept at `<data dir>/vutric/<file>` (`%LOCALAPPDATA%`, `$XDG_DATA_HOME` or
/// `~/.local/share`), created on first use. Desktop tools only; phone apps keep it in app storage.
pub fn identity_file(file: &str) -> Result<[u8; 32]> {
    let dir = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".local/share")))
        .context("no data directory")?
        .join("vutric");
    let path = dir.join(file);
    match std::fs::read(&path) {
        Ok(bytes) => {
            // Never silently replace an existing identity: that would break every pairing made with it.
            return <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| {
                anyhow!(
                    "{} is not a 32-byte identity; move it away to create a new one",
                    path.display()
                )
            });
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
    }
    std::fs::create_dir_all(&dir)?;
    let key = new_identity();
    // Owner-only, written beside and renamed into place so a crash never leaves half a key.
    let tmp = dir.join(format!("{file}.tmp"));
    {
        use std::io::Write as _;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        let mut f = opts
            .open(&tmp)
            .with_context(|| format!("create {}", tmp.display()))?;
        f.write_all(&key)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(key)
}

/// A fresh transfer id (16 random bytes).
#[must_use]
pub fn new_fling_id() -> vutric_core::FlingId {
    let mut id = [0u8; 16];
    id.copy_from_slice(&new_identity()[..16]);
    vutric_core::FlingId(id)
}

/// Monotonic microseconds since `start`, the clock in `EffectStarted::receiver_micros`.
#[must_use]
pub fn micros_since(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
}

fn clip(s: &str, max_bytes: usize) -> String {
    let mut end = s.len().min(max_bytes);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}
