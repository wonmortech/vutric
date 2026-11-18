//! Transit (transit.md): a direct TCP connection or one through a transit relay, picked by handshake, then an
//! encrypted record pipe.
//!
//! What the peer controls is bounded: at most [`MAX_HINTS`] direct and [`MAX_RELAYS`] relay hints are tried, a
//! handshake must match byte for byte, a record is refused above [`MAX_RECORD`] before it is buffered, and a record
//! whose nonce is not the next one closes the pipe.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};

use crate::crypto::{self, Key32, NONCE_BYTES};
use crate::mailbox::hex;

/// Most direct hints tried from the peer.
pub const MAX_HINTS: usize = 16;
/// Most relays tried from the peer.
pub const MAX_RELAYS: usize = 4;
/// Largest record accepted (transit.md: a bound, 64 MiB by default; our senders use 256 KiB records).
pub const MAX_RECORD: usize = 4 << 20;
/// Bytes of file data in each record we send.
pub const CHUNK: usize = 256 * 1024;
/// How long direct hints get before the relay is tried.
pub const RELAY_DELAY: Duration = Duration::from_secs(2);
/// Longest wait for any connection to win.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Which end of the transit: the file's sender leads ("Sender"), the receiver follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Sender,
    Receiver,
}

/// Where the peer may be reached.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hints {
    pub direct: Vec<(String, u16)>,
    pub relays: Vec<(String, u16)>,
}

/// `tcp:host:port`, the form the reference tools use for a relay address.
#[must_use]
pub fn parse_relay(s: &str) -> Option<(String, u16)> {
    let rest = s.strip_prefix("tcp:")?;
    let (host, port) = rest.rsplit_once(':')?;
    let port: u16 = port.parse().ok().filter(|p| *p != 0)?;
    (!host.is_empty() && host.len() <= 253).then(|| (host.to_owned(), port))
}

fn host_ok(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-:".contains(&b))
}

fn tcp_hint(v: &Value) -> Option<(String, u16)> {
    if v.get("type")?.as_str()? != "direct-tcp-v1" {
        return None;
    }
    let host = v.get("hostname")?.as_str()?;
    let port = u16::try_from(v.get("port")?.as_u64()?)
        .ok()
        .filter(|p| *p != 0)?;
    host_ok(host).then(|| (host.to_owned(), port))
}

impl Hints {
    /// The peer's `transit` value (`{"abilities-v1": [...], "hints-v1": [...]}`), bounded; unknown hint types are
    /// skipped as the spec asks. `None` if it is not an object with a hint list.
    #[must_use]
    pub fn parse(transit: &Value) -> Option<Self> {
        let list = transit.get("hints-v1")?.as_array()?;
        let mut h = Self::default();
        for v in list.iter().take(64) {
            match v.get("type").and_then(Value::as_str) {
                Some("direct-tcp-v1") => {
                    if let Some(t) = tcp_hint(v)
                        && h.direct.len() < MAX_HINTS
                    {
                        h.direct.push(t);
                    }
                }
                Some("relay-v1") => {
                    let first = v
                        .get("hints")
                        .and_then(Value::as_array)
                        .and_then(|hs| hs.iter().take(8).find_map(tcp_hint));
                    if let Some(t) = first
                        && h.relays.len() < MAX_RELAYS
                    {
                        h.relays.push(t);
                    }
                }
                _ => {}
            }
        }
        Some(h)
    }

    /// Our `transit` value for the peer.
    #[must_use]
    pub fn to_json(&self) -> Value {
        let mut hints: Vec<Value> = self
            .direct
            .iter()
            .map(|(h, p)| json!({"type": "direct-tcp-v1", "hostname": h, "port": p, "priority": 0.0}))
            .collect();
        for (h, p) in &self.relays {
            hints.push(json!({"type": "relay-v1", "hints": [
                {"type": "direct-tcp-v1", "hostname": h, "port": p, "priority": 0.0}
            ]}));
        }
        json!({
            "abilities-v1": [{"type": "direct-tcp-v1"}, {"type": "relay-v1"}],
            "hints-v1": hints,
        })
    }
}

/// This device's LAN IPv4 address: the source address the OS would use towards a public address (no packet is
/// sent). Loopback if there is no network.
#[must_use]
pub fn local_ip() -> IpAddr {
    std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .and_then(|s| {
            s.connect((Ipv4Addr::new(192, 0, 2, 1), 9))?;
            s.local_addr()
        })
        .map_or(IpAddr::V4(Ipv4Addr::LOCALHOST), |a| a.ip())
}

/// A fresh transit side: 8 random bytes as 16 hex digits. The transit relay refuses any other shape, and the
/// reference tool picks it apart from the 5-byte mailbox side (transit.md, "please relay").
#[must_use]
pub fn transit_side() -> String {
    use ring::rand::SecureRandom as _;
    let mut b = [0u8; 8];
    ring::rand::SystemRandom::new()
        .fill(&mut b)
        .expect("OS random generator");
    hex(&b)
}

/// Our side of a transit: a listening socket plus the relays we know.
#[derive(Debug)]
pub struct Transit {
    listener: TcpListener,
    pub ours: Hints,
    key: Key32,
    role: Role,
    side: String,
}

impl Transit {
    /// Listens on `bind` (port 0 = any) and builds our hints: that address (or the LAN address for an unspecified
    /// bind) and `relay`. `side` is what we tell the relay, from [`transit_side`] (16 hex digits).
    pub async fn new(
        key: Key32,
        role: Role,
        side: &str,
        bind: IpAddr,
        relay: Option<(String, u16)>,
    ) -> Result<Self> {
        if side.len() != 16 || !side.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("a transit side is 16 hex digits");
        }
        let listener = TcpListener::bind((bind, 0))
            .await
            .context("transit listen")?;
        let port = listener.local_addr()?.port();
        let ip = if bind.is_unspecified() {
            local_ip()
        } else {
            bind
        };
        Ok(Self {
            listener,
            ours: Hints {
                direct: vec![(ip.to_string(), port)],
                relays: relay.into_iter().collect(),
            },
            key,
            role,
            side: side.to_owned(),
        })
    }

    fn handshake(&self, role: Role) -> Vec<u8> {
        let (word, purpose): (&str, &[u8]) = match role {
            Role::Sender => ("sender", b"transit_sender"),
            Role::Receiver => ("receiver", b"transit_receiver"),
        };
        format!(
            "transit {word} {} ready\n\n",
            hex(&crypto::derive(&self.key, purpose))
        )
        .into_bytes()
    }

    fn relay_line(&self) -> Vec<u8> {
        format!(
            "please relay {} for side {}\n",
            hex(&crypto::derive(&self.key, b"transit_relay_token")),
            self.side
        )
        .into_bytes()
    }

    /// Races every way to the peer and returns the first that passes the handshake (the sender decides with
    /// `go`).
    pub async fn connect(self, peer: &Hints) -> Result<Records> {
        let ours = self.handshake(self.role);
        let theirs = self.handshake(match self.role {
            Role::Sender => Role::Receiver,
            Role::Receiver => Role::Sender,
        });
        let relay_line = self.relay_line();
        let role = self.role;
        let mut tries = tokio::task::JoinSet::new();
        let attempt = move |s: TcpStream, relay: bool| {
            let (ours, theirs, line) = (ours.clone(), theirs.clone(), relay_line.clone());
            async move { negotiate(s, relay.then_some(line), &ours, &theirs, role).await }
        };
        for (h, p) in peer.direct.iter().take(MAX_HINTS).cloned() {
            let a = attempt.clone();
            tries.spawn(async move {
                let s = tokio::time::timeout(
                    Duration::from_secs(5),
                    TcpStream::connect((h.as_str(), p)),
                )
                .await
                .map_err(|_| anyhow!("direct hint timed out"))??;
                a(s, false).await
            });
        }
        let mut relays = self.ours.relays.clone();
        for r in peer.relays.iter().take(MAX_RELAYS) {
            if !relays.contains(r) {
                relays.push(r.clone());
            }
        }
        let delay = if peer.direct.is_empty() {
            Duration::ZERO
        } else {
            RELAY_DELAY
        };
        for (h, p) in relays {
            let a = attempt.clone();
            tries.spawn(async move {
                tokio::time::sleep(delay).await;
                let s = tokio::time::timeout(
                    Duration::from_secs(10),
                    TcpStream::connect((h.as_str(), p)),
                )
                .await
                .map_err(|_| anyhow!("relay timed out"))??;
                a(s, true).await
            });
        }
        let deadline = tokio::time::sleep(CONNECT_TIMEOUT);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                () = &mut deadline => bail!("could not reach the other side, directly or through a relay"),
                inbound = self.listener.accept() => {
                    if let Ok((s, _)) = inbound {
                        let a = attempt.clone();
                        tries.spawn(async move { a(s, false).await });
                    }
                }
                Some(done) = tries.join_next(), if !tries.is_empty() => {
                    if let Ok(Ok(mut s)) = done {
                        if role == Role::Sender {
                            s.write_all(b"go\n").await?;
                        }
                        tries.abort_all();
                        return Ok(Records::new(s, &self.key, role));
                    }
                }
            }
        }
    }
}

/// One candidate connection: relay handshake if through a relay, then both transit handshakes; the receiver also
/// waits for `go`.
async fn negotiate(
    mut s: TcpStream,
    relay: Option<Vec<u8>>,
    ours: &[u8],
    theirs: &[u8],
    role: Role,
) -> Result<TcpStream> {
    s.set_nodelay(true)?;
    tokio::time::timeout(CONNECT_TIMEOUT, async {
        if let Some(line) = relay {
            s.write_all(&line).await?;
            let mut ok = [0u8; 3];
            s.read_exact(&mut ok).await?;
            if &ok != b"ok\n" {
                bail!("relay refused");
            }
        }
        s.write_all(ours).await?;
        let mut got = vec![0u8; theirs.len()];
        s.read_exact(&mut got).await?;
        if got != theirs {
            bail!("not the other side's transit handshake");
        }
        if role == Role::Receiver {
            let mut go = [0u8; 3];
            s.read_exact(&mut go).await?;
            if &go != b"go\n" {
                bail!("the sender chose another connection");
            }
        }
        Ok(s)
    })
    .await
    .map_err(|_| anyhow!("transit handshake timed out"))?
}

/// The encrypted record pipe: 4-byte big-endian length, then `nonce ‖ secretbox`; nonces count up from zero.
#[derive(Debug)]
pub struct Records {
    s: TcpStream,
    send_key: Key32,
    recv_key: Key32,
    sent: u128,
    received: u128,
}

fn nonce(n: u128) -> [u8; NONCE_BYTES] {
    let mut out = [0u8; NONCE_BYTES];
    out[NONCE_BYTES - 16..].copy_from_slice(&n.to_be_bytes());
    out
}

impl Records {
    fn new(s: TcpStream, key: &Key32, role: Role) -> Self {
        let s2r = crypto::derive(key, b"transit_record_sender_key");
        let r2s = crypto::derive(key, b"transit_record_receiver_key");
        let (send_key, recv_key) = match role {
            Role::Sender => (s2r, r2s),
            Role::Receiver => (r2s, s2r),
        };
        Self {
            s,
            send_key,
            recv_key,
            sent: 0,
            received: 0,
        }
    }

    pub async fn send(&mut self, plain: &[u8]) -> Result<()> {
        let boxed = crypto::seal_with(&self.send_key, &nonce(self.sent), plain);
        self.sent += 1;
        let len = u32::try_from(boxed.len()).context("record too large")?;
        self.s.write_all(&len.to_be_bytes()).await?;
        self.s.write_all(&boxed).await?;
        Ok(())
    }

    /// The next record. Errors on a record over [`MAX_RECORD`] (before reading it), a bad tag, or a nonce that is
    /// not the next one.
    pub async fn recv(&mut self) -> Result<Vec<u8>> {
        let mut len = [0u8; 4];
        self.s
            .read_exact(&mut len)
            .await
            .context("transit closed")?;
        let len = u32::from_be_bytes(len) as usize;
        if !(crypto::OVERHEAD..=MAX_RECORD + crypto::OVERHEAD).contains(&len) {
            bail!("transit record of {len} bytes refused");
        }
        let mut buf = vec![0u8; len];
        self.s
            .read_exact(&mut buf)
            .await
            .context("transit closed")?;
        let (n, plain) = crypto::open(&self.recv_key, &buf)
            .ok_or_else(|| anyhow!("transit record did not decrypt"))?;
        if n != nonce(self.received) {
            bail!("transit record out of order");
        }
        self.received += 1;
        Ok(plain)
    }

    /// The peer's address (for the route shown to the user).
    pub fn peer(&self) -> Option<SocketAddr> {
        self.s.peer_addr().ok()
    }

    pub async fn close(mut self) {
        let _ = self.s.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_round_trip_and_are_bounded() {
        let h = Hints {
            direct: vec![("192.168.1.5".into(), 4000)],
            relays: vec![("transit.example".into(), 4001)],
        };
        assert_eq!(Hints::parse(&h.to_json()), Some(h));
        let many: Vec<Value> = (0..100)
            .map(|i| json!({"type": "direct-tcp-v1", "hostname": format!("10.0.0.{i}"), "port": 1}))
            .collect();
        let p = Hints::parse(&json!({"hints-v1": many})).unwrap();
        assert_eq!(p.direct.len(), MAX_HINTS);
        let odd = json!({"hints-v1": [
            {"type": "direct-tcp-v1", "hostname": "a b", "port": 1},
            {"type": "direct-tcp-v1", "hostname": "x", "port": 0},
            {"type": "direct-tcp-v1", "hostname": "x", "port": 70000},
            {"type": "tor-tcp-v1", "hostname": "x.onion", "port": 1},
            {"type": "relay-v1", "hints": [{"type": "websocket-v1", "url": "wss://r"}]}
        ]});
        assert_eq!(Hints::parse(&odd), Some(Hints::default()));
        assert_eq!(Hints::parse(&json!({})), None);
    }

    #[test]
    fn relay_addresses() {
        assert_eq!(
            parse_relay("tcp:transit.magic-wormhole.io:4001"),
            Some(("transit.magic-wormhole.io".into(), 4001))
        );
        assert_eq!(parse_relay("tcp:[::1]:4001"), Some(("[::1]".into(), 4001)));
        for bad in ["transit:4001", "tcp::4001", "tcp:x:0", "tcp:x:y"] {
            assert_eq!(parse_relay(bad), None, "{bad}");
        }
    }

    #[test]
    fn nonces_count_up() {
        assert_eq!(nonce(0), [0u8; 24]);
        assert_eq!(nonce(1)[23], 1);
        assert_eq!(nonce(256)[22], 1);
    }

    #[tokio::test]
    async fn direct_pipe_both_ways_and_refuses_strangers() {
        let key = [5u8; 32];
        let lo = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let a = Transit::new(key, Role::Sender, &transit_side(), lo, None)
            .await
            .unwrap();
        let b = Transit::new(key, Role::Receiver, &transit_side(), lo, None)
            .await
            .unwrap();
        let (ha, hb) = (a.ours.clone(), b.ours.clone());
        // A stranger with another key dials the sender too; it must not win.
        let wrong = Transit::new([6u8; 32], Role::Receiver, &transit_side(), lo, None)
            .await
            .unwrap();
        let stranger = tokio::spawn(async move { wrong.connect(&ha.clone()).await });
        let tb = tokio::spawn(async move {
            b.connect(&Hints {
                direct: vec![],
                relays: vec![],
            })
            .await
        });
        let mut ra = a.connect(&hb).await.unwrap();
        let mut rb = tb.await.unwrap().unwrap();
        ra.send(b"one").await.unwrap();
        ra.send(&vec![9u8; CHUNK]).await.unwrap();
        assert_eq!(rb.recv().await.unwrap(), b"one");
        assert_eq!(rb.recv().await.unwrap().len(), CHUNK);
        rb.send(b"ack").await.unwrap();
        assert_eq!(ra.recv().await.unwrap(), b"ack");
        stranger.abort();
    }
}
