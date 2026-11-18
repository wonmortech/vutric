//! The sending side: Vutric sends files to a Quick Share receiver (used by the tests against our own receiver;
//! sending to Android also needs its BLE wake-up, which is left open in ADR-0056).

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context as _, Result, bail};
use tokio::io::AsyncReadExt as _;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::mpsc;

use crate::conn::{self, Bytes, Piece};
use crate::endpoint::{self, Device};
use crate::msg::{self, Offline, PayloadHeader, Share};
use crate::ukey2::{self, Channel};

/// What a finished send did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    /// The 4-digit code, as the receiver shows it.
    pub pin: String,
    pub files: usize,
    pub bytes: u64,
}

/// The receiver said no (its response status: 2 reject, 3 no space, 4 unsupported, 5 timed out).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused(pub i64);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "receiver refused ({})", self.0)
    }
}

impl std::error::Error for Refused {}

struct Link {
    rx: mpsc::Receiver<Vec<u8>>,
    w: OwnedWriteHalf,
    ch: Channel,
    bytes: Bytes,
}

impl Link {
    /// The next sharing frame, answering keep-alives on the way.
    async fn share(&mut self) -> Result<Share> {
        loop {
            let f = conn::next(&mut self.rx)
                .await
                .context("receiver went quiet")?;
            let plain = self.ch.open(&f)?;
            let frame = msg::parse_offline(&plain).context("bad frame")?;
            match &frame {
                Offline::KeepAlive { ack: false } => {
                    conn::send(&mut self.w, &mut self.ch, &msg::keep_alive(true)).await?;
                }
                Offline::Disconnection => bail!("receiver disconnected"),
                Offline::Payload { .. } => match self.bytes.feed(&frame) {
                    Piece::Message(_, b) => {
                        return msg::parse_share(&b).context("bad sharing frame");
                    }
                    Piece::Bad => bail!("bad payload"),
                    _ => {}
                },
                _ => {}
            }
        }
    }
}

fn random_id() -> i64 {
    use ring::rand::SecureRandom as _;
    let mut b = [0u8; 8];
    ring::rand::SystemRandom::new()
        .fill(&mut b)
        .expect("system random");
    i64::from_le_bytes(b) & i64::MAX
}

async fn connect(addr: SocketAddr, name: &str) -> Result<(Link, String)> {
    let tcp = tokio::time::timeout(conn::KEEP_ALIVE, tokio::net::TcpStream::connect(addr))
        .await
        .context("connect timeout")??;
    let _ = tcp.set_nodelay(true);
    let (r, mut w) = tcp.into_split();
    let mut rx = conn::reader(r);
    conn::write_frame(
        &mut w,
        &msg::connection_request(
            &endpoint::random_endpoint_id(),
            &endpoint::info(Device::Laptop, name),
        ),
    )
    .await?;
    let (hs, init) = ukey2::Client::start();
    conn::write_frame(&mut w, &init).await?;
    let server_init = conn::next(&mut rx).await.context("no server init")?;
    let (keys, finished) = hs.finish(&server_init)?;
    conn::write_frame(&mut w, &finished).await?;
    conn::write_frame(&mut w, &msg::connection_response(true)).await?;
    let f = conn::next(&mut rx)
        .await
        .context("no connection response")?;
    if msg::parse_offline(&f) != Some(Offline::ConnectionResponse { accepted: true }) {
        bail!("receiver refused the connection");
    }
    let link = Link {
        rx,
        w,
        ch: Channel::new(&keys, false),
        bytes: Bytes::default(),
    };
    Ok((link, keys.pin()))
}

/// Sends `paths` to the Quick Share receiver at `addr`, introducing ourselves as `name`.
pub async fn send(addr: SocketAddr, name: &str, paths: &[PathBuf]) -> Result<Sent> {
    let mut files = Vec::new();
    for p in paths {
        let meta = tokio::fs::metadata(p)
            .await
            .with_context(|| p.display().to_string())?;
        if !meta.is_file() {
            bail!("{} is not a file", p.display());
        }
        files.push(msg::FileMeta {
            name: p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_owned(),
            payload_id: random_id(),
            size: i64::try_from(meta.len())?,
            mime: "application/octet-stream".to_owned(),
        });
    }
    let (mut link, pin) = connect(addr, name).await?;
    let mut secret = [0u8; 78];
    {
        use ring::rand::SecureRandom as _;
        ring::rand::SystemRandom::new()
            .fill(&mut secret)
            .expect("system random");
    }
    let (hash, signed) = secret.split_at(6);
    conn::send_share(
        &mut link.w,
        &mut link.ch,
        &msg::paired_key_encryption(hash.try_into()?, signed.try_into()?),
    )
    .await?;
    loop {
        match link.share().await? {
            Share::PairedKeyResult => break,
            Share::PairedKeyEncryption | Share::Other(_) => {}
            other => bail!("unexpected {other:?}"),
        }
    }
    conn::send_share(&mut link.w, &mut link.ch, &msg::paired_key_result()).await?;
    conn::send_share(&mut link.w, &mut link.ch, &msg::introduction(&files)).await?;
    match link.share().await? {
        Share::Response(msg::ACCEPT) => {}
        Share::Response(s) => return Err(Refused(s).into()),
        other => bail!("unexpected {other:?}"),
    }
    let mut total = 0u64;
    for (meta, path) in files.iter().zip(paths) {
        let header = PayloadHeader {
            id: meta.payload_id,
            kind: 2,
            total_size: meta.size,
        };
        let mut f = tokio::fs::File::open(path).await?;
        let mut offset = 0i64;
        let mut buf = vec![0u8; conn::CHUNK];
        loop {
            let n = f.read(&mut buf).await?;
            let last = i64::try_from(n)? + offset >= meta.size;
            conn::send(
                &mut link.w,
                &mut link.ch,
                &msg::payload(&header, offset, &buf[..n], last),
            )
            .await?;
            offset += i64::try_from(n)?;
            if last {
                break;
            }
            if n == 0 {
                bail!("{} shrank while sending", path.display());
            }
        }
        total += meta.size.unsigned_abs();
    }
    // Wait for the receiver to finish and say goodbye.
    while let Some(f) = conn::next(&mut link.rx).await {
        let Ok(plain) = link.ch.open(&f) else { break };
        match msg::parse_offline(&plain) {
            Some(Offline::Disconnection) | None => break,
            Some(Offline::KeepAlive { ack: false }) => {
                conn::send(&mut link.w, &mut link.ch, &msg::keep_alive(true)).await?;
            }
            _ => {}
        }
    }
    let _ = conn::send(&mut link.w, &mut link.ch, &msg::disconnection()).await;
    Ok(Sent {
        pin,
        files: files.len(),
        bytes: total,
    })
}
