//! File bytes travel on a second TCP connection (spec, `payloadTransferInfo`): the sender listens on `port`, the
//! receiver connects. It is TLS with the natural roles (the receiver is the TLS client), and the certificate must
//! be the one of the main connection, so a third device cannot slip bytes in. The receiver reads exactly
//! `payloadSize` bytes.
//!
//! The TLS roles of the payload connection are not written in the spec; they follow what the KDE Connect clients
//! do and are re-checked by the real-app test (backlog M5).

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};

use crate::link::{Link, Me};
use crate::proto;

/// Ports a sender offers payloads on (the range KDE Connect firewall rules open).
pub const PORTS: std::ops::RangeInclusive<u16> = 1739..=1764;
/// How long a sender waits for the receiver to fetch one file.
pub const FETCH_WAIT: Duration = Duration::from_secs(60);
const CHUNK: usize = 256 * 1024;

/// Fetches `size` bytes from the sender at `ip:port` into `staged`. The certificate must be `cert`.
pub async fn fetch(
    me: &Me,
    ip: IpAddr,
    port: u16,
    cert: &[u8],
    size: u64,
    staged: &Path,
) -> Result<()> {
    let tcp = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect((ip, port)))
        .await
        .context("payload connect timeout")??;
    let _ = tcp.set_nodelay(true);
    let name = rustls::pki_types::ServerName::try_from("kdeconnect")?;
    let mut tls = tokio_rustls::TlsConnector::from(me.tls.client_config()?)
        .connect(name, tcp)
        .await
        .context("payload tls")?;
    let seen = tls
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|c| c.first())
        .context("no certificate")?;
    if seen.as_ref() != cert {
        bail!("payload from another certificate");
    }
    let mut file = tokio::fs::File::create(staged)
        .await
        .with_context(|| staged.display().to_string())?;
    let mut left = size;
    let mut buf = vec![0u8; CHUNK];
    while left > 0 {
        let want = usize::try_from(left.min(CHUNK as u64)).expect("bounded by CHUNK");
        let n = tokio::time::timeout(Duration::from_secs(45), tls.read(&mut buf[..want]))
            .await
            .context("payload stalled")??;
        if n == 0 {
            bail!("payload ended {left} bytes early");
        }
        file.write_all(&buf[..n]).await?;
        left -= n as u64;
    }
    file.flush().await?;
    file.sync_all().await?;
    Ok(())
}

/// Binds the first free port of `ports` (`0..=0` for any, in tests).
pub async fn listen(bind: IpAddr, ports: std::ops::RangeInclusive<u16>) -> Result<TcpListener> {
    for p in ports {
        if let Ok(l) = TcpListener::bind(SocketAddr::new(bind, p)).await {
            return Ok(l);
        }
    }
    bail!("no free payload port")
}

/// Waits for the paired receiver (`ip`, `cert`) to fetch `path` from `listener`, and streams the file to it.
async fn serve(
    me: &Me,
    listener: &TcpListener,
    ip: IpAddr,
    cert: &[u8],
    path: &Path,
) -> Result<()> {
    let go = async {
        loop {
            let (tcp, from) = listener.accept().await?;
            if from.ip() != ip {
                continue;
            }
            let _ = tcp.set_nodelay(true);
            let mut tls = tokio_rustls::TlsAcceptor::from(me.tls.server_config()?)
                .accept(tcp)
                .await?;
            let seen = tls
                .get_ref()
                .1
                .peer_certificates()
                .and_then(|c| c.first())
                .context("no certificate")?;
            if seen.as_ref() != cert {
                bail!("payload fetched by another certificate");
            }
            let mut file = tokio::fs::File::open(path).await?;
            tokio::io::copy(&mut file, &mut tls).await?;
            tls.shutdown().await?;
            return Ok(());
        }
    };
    tokio::time::timeout(FETCH_WAIT, go)
        .await
        .context("receiver did not fetch the file")?
}

impl Link {
    /// Sends files to a paired peer, one after another, each on a payload port from `ports`.
    pub async fn send_files(
        &mut self,
        me: &Me,
        paths: &[PathBuf],
        bind: IpAddr,
        ports: std::ops::RangeInclusive<u16>,
    ) -> Result<()> {
        let mut sizes = Vec::with_capacity(paths.len());
        for p in paths {
            let m = tokio::fs::metadata(p)
                .await
                .with_context(|| p.display().to_string())?;
            if !m.is_file() {
                bail!("{} is not a file", p.display());
            }
            sizes.push(m.len());
        }
        let total: u64 = sizes.iter().sum();
        let count = paths.len() as u64;
        self.send(&proto::Share::update(count, total)).await?;
        for (p, size) in paths.iter().zip(sizes) {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("file");
            let listener = listen(bind, ports.clone()).await?;
            let port = listener.local_addr()?.port();
            self.send(&proto::Share::file(name, size, port, count, total))
                .await?;
            let (ip, cert) = (self.addr.ip(), self.peer_cert.clone());
            serve(me, &listener, ip, &cert, p).await?;
        }
        Ok(())
    }
}
