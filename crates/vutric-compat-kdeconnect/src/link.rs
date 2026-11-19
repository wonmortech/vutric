//! One KDE Connect connection, set up in either role (spec, "TLS"):
//!
//! - **We dialled** (we heard the other device's UDP identity, or the user picked it): we send the plaintext
//!   first identity naming the device we expect, then act as the **TLS server**.
//! - **We were dialled**: we read that first line (at most 4 KiB), check it names us and version 8, then act as
//!   the **TLS client**.
//!
//! Both sides then send the full identity inside TLS; a device id or version that changed aborts the connection.

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use tokio::io::{
    AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader, ReadHalf, WriteHalf,
};
use tokio::net::TcpStream;
use tokio_rustls::TlsStream;

use crate::proto::{self, DeviceType, Identity, Packet};
use crate::tls;

/// How long the plaintext part and the TLS handshake may take.
pub const HANDSHAKE: Duration = Duration::from_secs(10);

/// Who we are, as the links need it.
#[derive(Debug, Clone)]
pub struct Me {
    pub tls: tls::Identity,
    pub name: String,
    pub kind: DeviceType,
}

#[derive(Debug)]
pub struct Link {
    reader: BufReader<ReadHalf<TlsStream<TcpStream>>>,
    writer: WriteHalf<TlsStream<TcpStream>>,
    pub peer: Identity,
    /// The certificate the peer presented (DER).
    pub peer_cert: Vec<u8>,
    pub addr: SocketAddr,
}

/// Reads one line of at most `max` bytes; `Ok(None)` at a clean end of input.
pub async fn read_line<R: tokio::io::AsyncBufRead + Unpin>(
    r: &mut R,
    max: usize,
) -> Result<Option<Vec<u8>>> {
    let mut buf = Vec::new();
    let n = (&mut *r)
        .take(max as u64 + 1)
        .read_until(b'\n', &mut buf)
        .await?;
    if n == 0 {
        return Ok(None);
    }
    if buf.last() != Some(&b'\n') {
        bail!("line too long or cut");
    }
    buf.pop();
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    Ok(Some(buf))
}

fn peer_cert(s: &TlsStream<TcpStream>) -> Result<Vec<u8>> {
    let certs = match s {
        TlsStream::Client(c) => c.get_ref().1.peer_certificates(),
        TlsStream::Server(c) => c.get_ref().1.peer_certificates(),
    };
    Ok(certs
        .and_then(|c| c.first())
        .context("no peer certificate")?
        .to_vec())
}

impl Link {
    /// We open the connection to `addr`, expecting the device `target`.
    pub async fn dial(me: &Me, addr: SocketAddr, target: &str) -> Result<Self> {
        let go = async {
            let mut tcp = TcpStream::connect(addr).await?;
            let _ = tcp.set_nodelay(true);
            tcp.write_all(&Identity::first(&me.tls.device_id, target))
                .await?;
            let tls = tokio_rustls::TlsAcceptor::from(me.tls.server_config()?)
                .accept(tcp)
                .await
                .context("tls (server role)")?;
            Self::finish(me, TlsStream::Server(tls), addr, target).await
        };
        tokio::time::timeout(HANDSHAKE, go)
            .await
            .context("handshake timeout")?
    }

    /// Someone opened a connection to us.
    pub async fn answer(me: &Me, tcp: TcpStream, addr: SocketAddr) -> Result<Self> {
        let go = async {
            let _ = tcp.set_nodelay(true);
            let mut r = BufReader::new(tcp);
            let line = read_line(&mut r, proto::MAX_PREAUTH_LINE)
                .await?
                .context("closed")?;
            let first = Packet::parse(&line, proto::MAX_PREAUTH_LINE)
                .as_ref()
                .and_then(Identity::from_packet)
                .context("first line is not an identity")?;
            if first.protocol_version != proto::PROTOCOL_VERSION
                || first.target_protocol_version != Some(proto::PROTOCOL_VERSION)
                || first.target_device_id.as_deref() != Some(me.tls.device_id.as_str())
            {
                bail!("identity is not for us or not version 8");
            }
            if !r.buffer().is_empty() {
                bail!("bytes before TLS");
            }
            let tcp = r.into_inner();
            let name = rustls::pki_types::ServerName::try_from("kdeconnect")?;
            let tls = tokio_rustls::TlsConnector::from(me.tls.client_config()?)
                .connect(name, tcp)
                .await
                .context("tls (client role)")?;
            Self::finish(me, TlsStream::Client(tls), addr, &first.device_id).await
        };
        tokio::time::timeout(HANDSHAKE, go)
            .await
            .context("handshake timeout")?
    }

    async fn finish(
        me: &Me,
        tls: TlsStream<TcpStream>,
        addr: SocketAddr,
        expect: &str,
    ) -> Result<Self> {
        let cert = peer_cert(&tls)?;
        let (r, mut w) = tokio::io::split(tls);
        w.write_all(&Identity::full(&me.tls.device_id, &me.name, me.kind))
            .await?;
        let mut reader = BufReader::new(r);
        let line = read_line(&mut reader, proto::MAX_PREAUTH_LINE)
            .await?
            .context("closed")?;
        let peer = Packet::parse(&line, proto::MAX_PREAUTH_LINE)
            .as_ref()
            .and_then(Identity::from_packet)
            .context("no identity after TLS")?;
        if peer.device_id != expect || peer.protocol_version != proto::PROTOCOL_VERSION {
            bail!("identity changed after TLS");
        }
        Ok(Self {
            reader,
            writer: w,
            peer,
            peer_cert: cert,
            addr,
        })
    }

    /// The next packet; `Ok(None)` when the peer closed. Lines that are not packets are skipped.
    pub async fn next(&mut self) -> Result<Option<Packet>> {
        loop {
            let Some(line) = read_line(&mut self.reader, proto::MAX_LINE).await? else {
                return Ok(None);
            };
            if line.is_empty() {
                continue;
            }
            if let Some(p) = Packet::parse(&line, proto::MAX_LINE) {
                return Ok(Some(p));
            }
        }
    }

    pub async fn send(&mut self, line: &[u8]) -> Result<()> {
        self.writer.write_all(line).await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// Asks the peer to pair and waits up to `wait` for its answer. Other packets meanwhile are dropped.
    pub async fn request_pair(&mut self, timestamp: u64, wait: Duration) -> Result<bool> {
        self.send(&proto::Pair::request(timestamp)).await?;
        let answer = async {
            while let Some(p) = self.next().await? {
                if let Some(pair) = proto::Pair::from_packet(&p) {
                    return Ok(pair.pair);
                }
            }
            bail!("closed while pairing")
        };
        tokio::time::timeout(wait, answer)
            .await
            .context("no pairing answer")?
    }
}
