//! The sending side (spec §4 as the client, and §3 `register`): Vutric sends files to a `LocalSend` app.
//!
//! The receiver's certificate is self-signed; we report its fingerprint instead of trusting a CA, so the user can
//! pin it. Every answer is size-limited and parsed by [`crate::proto`].

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use bytes::Bytes;
use http::{Method, Request, StatusCode};
use http_body_util::{BodyExt as _, Full, Limited};
use hyper_util::rt::TokioIo;
use tokio::io::AsyncReadExt as _;
use tokio::net::TcpStream;

use crate::proto::{self, DeviceInfo, FileMeta, PrepareUpload};
use crate::tls::{self, Identity};

const CONNECT: Duration = Duration::from_secs(5);
/// How long we wait for the person on the other side to accept.
pub const ANSWER: Duration = Duration::from_secs(120);
const CHUNK: usize = 256 * 1024;

/// Where a `LocalSend` receiver listens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub addr: SocketAddr,
    pub https: bool,
}

/// Why a send did not happen, in the receiver's words (spec §4 status codes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    PinRequired,
    Rejected,
    Busy,
    TooManyRequests,
    /// `204`: nothing to transfer.
    NothingNeeded,
    ChecksumMismatch,
    Other(u16),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Refusal {}

fn refusal(code: StatusCode) -> Refusal {
    match code.as_u16() {
        401 => Refusal::PinRequired,
        403 => Refusal::Rejected,
        409 => Refusal::Busy,
        429 => Refusal::TooManyRequests,
        204 => Refusal::NothingNeeded,
        422 => Refusal::ChecksumMismatch,
        c => Refusal::Other(c),
    }
}

trait Io: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Io for T {}

/// One request on a fresh connection; returns the status, the body (at most `max` bytes) and the receiver's
/// certificate fingerprint.
async fn call<B>(
    id: &Identity,
    t: Target,
    method: Method,
    path: &str,
    body: B,
    max: usize,
    wait: Duration,
) -> Result<(StatusCode, Bytes, Option<String>)>
where
    B: hyper::body::Body<Data = Bytes> + Send + 'static,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let tcp = tokio::time::timeout(CONNECT, TcpStream::connect(t.addr))
        .await
        .context("connect timeout")?
        .context("connect")?;
    let _ = tcp.set_nodelay(true);
    let (io, cert): (Box<dyn Io>, Option<String>) = if t.https {
        let conn = tokio_rustls::TlsConnector::from(Arc::new(id.client_config()?));
        let name = rustls::pki_types::ServerName::try_from("localsend").expect("valid name");
        let s = tokio::time::timeout(CONNECT, conn.connect(name, tcp))
            .await
            .context("TLS timeout")?
            .context("TLS")?;
        let fp = s
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|c| c.first())
            .map(|c| tls::fingerprint(c));
        (Box::new(s), fp)
    } else {
        (Box::new(tcp), None)
    };
    let (mut tx, conn) = hyper::client::conn::http1::handshake(TokioIo::new(io))
        .await
        .context("HTTP handshake")?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header(http::header::HOST, t.addr.to_string())
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(body)?;
    let resp = tokio::time::timeout(wait, tx.send_request(req))
        .await
        .context("no answer")?
        .context("request")?;
    let code = resp.status();
    let bytes = Limited::new(resp.into_body(), max)
        .collect()
        .await
        .map_err(|e| anyhow!("answer: {e}"))?
        .to_bytes();
    Ok((code, bytes, cert))
}

/// `register`: introduces us and returns the receiver's description and certificate hash.
pub async fn register(
    id: &Identity,
    t: Target,
    us: &DeviceInfo,
) -> Result<(DeviceInfo, Option<String>)> {
    let body = Full::new(Bytes::from(serde_json::to_vec(us)?));
    let (code, b, cert) = call(
        id,
        t,
        Method::POST,
        "/api/localsend/v2/register",
        body,
        16 * 1024,
        CONNECT,
    )
    .await?;
    if !code.is_success() {
        bail!(refusal(code));
    }
    Ok((proto::parse_register(&b)?, cert))
}

fn mime(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("heic") => "image/heic",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("pdf") => "application/pdf",
        Some("txt") => "text/plain",
        Some("zip") => "application/zip",
        Some("apk") => "application/vnd.android.package-archive",
        _ => "application/octet-stream",
    }
}

/// What a finished send did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    /// Files the receiver accepted and got.
    pub files: usize,
    pub bytes: u64,
    /// The receiver's certificate hash (to pin).
    pub peer_cert: Option<String>,
}

/// Sends `paths` to a `LocalSend` receiver: `prepare-upload`, wait for its user, then upload what was accepted.
/// A refusal comes back as a [`Refusal`] error.
pub async fn send(
    id: &Identity,
    t: Target,
    us: &DeviceInfo,
    paths: &[PathBuf],
    pin: Option<&str>,
) -> Result<Sent> {
    let mut files = std::collections::BTreeMap::new();
    for (n, p) in paths.iter().enumerate() {
        let meta = tokio::fs::metadata(p)
            .await
            .with_context(|| p.display().to_string())?;
        if !meta.is_file() {
            bail!("{} is not a file", p.display());
        }
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_owned();
        let fid = format!("f{n}");
        files.insert(
            fid.clone(),
            FileMeta {
                id: fid,
                file_type: mime(&name).to_owned(),
                file_name: name,
                size: meta.len(),
                sha256: None,
                preview: None,
                metadata: None,
            },
        );
    }
    let prep = PrepareUpload {
        info: us.clone(),
        files,
    };
    let path = match pin {
        Some(p) => format!(
            "/api/localsend/v2/prepare-upload?pin={}",
            proto::pct_encode(p)
        ),
        None => "/api/localsend/v2/prepare-upload".to_owned(),
    };
    let (code, body, cert) = call(
        id,
        t,
        Method::POST,
        &path,
        Full::new(Bytes::from(serde_json::to_vec(&prep)?)),
        proto::MAX_BODY,
        ANSWER,
    )
    .await?;
    if code != StatusCode::OK {
        bail!(refusal(code));
    }
    let answer = proto::parse_prepare_response(&body)?;
    let mut sent = Sent {
        files: 0,
        bytes: 0,
        peer_cert: cert,
    };
    for (n, p) in paths.iter().enumerate() {
        let fid = format!("f{n}");
        let Some(token) = answer.files.get(&fid) else {
            continue; // not accepted
        };
        let size = prep.files[&fid].size;
        let q = format!(
            "/api/localsend/v2/upload?sessionId={}&fileId={}&token={}",
            proto::pct_encode(&answer.session_id),
            proto::pct_encode(&fid),
            proto::pct_encode(token)
        );
        let (code, _, _) = call(
            id,
            t,
            Method::POST,
            &q,
            file_body(p.clone()),
            4096,
            Duration::from_secs(24 * 3600),
        )
        .await?;
        if !code.is_success() {
            bail!(refusal(code));
        }
        sent.files += 1;
        sent.bytes += size;
    }
    Ok(sent)
}

/// Streams a file as a request body, 256 KiB at a time.
pub(crate) fn file_body(path: PathBuf) -> http_body_util::channel::Channel<Bytes, std::io::Error> {
    let (mut tx, body) = http_body_util::channel::Channel::new(4);
    tokio::spawn(async move {
        let mut f = match tokio::fs::File::open(&path).await {
            Ok(f) => f,
            Err(e) => {
                tx.abort(e);
                return;
            }
        };
        loop {
            let mut buf = vec![0u8; CHUNK];
            match f.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    buf.truncate(n);
                    if tx.send_data(Bytes::from(buf)).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    tx.abort(e);
                    break;
                }
            }
        }
    });
    body
}

/// Cancels a session we started (spec §4.3).
pub async fn cancel(id: &Identity, t: Target, session_id: &str) -> Result<()> {
    let path = format!(
        "/api/localsend/v2/cancel?sessionId={}",
        proto::pct_encode(session_id)
    );
    call(
        id,
        t,
        Method::POST,
        &path,
        Full::new(Bytes::new()),
        4096,
        CONNECT,
    )
    .await?;
    Ok(())
}
