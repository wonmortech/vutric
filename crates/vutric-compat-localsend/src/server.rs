//! The receiving side (spec §4, "Upload API"): an HTTPS server that `LocalSend` apps send to.
//!
//! Nothing is written before the user (through [`Event::Offer`]) accepts. Accepted files go to the staging
//! folder only, as `.vutric-part` until complete and checked; the daemon then moves them with the storage rules
//! (ADR-0007 part 4). One session at a time (`409` otherwise), at most 10 offers per minute per address (`429`),
//! an upload must come from the address that prepared it with that file's token (`403`), never more bytes than
//! announced, and a `sha256` mismatch is refused with `422` (v2.2).

use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use bytes::Bytes;
use http::{Method, Request, Response, StatusCode};
use http_body_util::{BodyExt as _, Full, Limited};
use hyper::body::Incoming;
use hyper_util::rt::{TokioIo, TokioTimer};
use sha2::{Digest as _, Sha256};
use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, mpsc, oneshot};

use crate::proto::{self, DeviceInfo, DeviceType, FileMeta, Scheme};
use crate::tls::{self, Identity};

const API: &str = "/api/localsend/v2/";
/// Most connections served at once.
const MAX_CONNECTIONS: usize = 64;
/// Offers per address per minute.
const OFFERS_PER_MINUTE: u32 = 10;
/// A stalled upload is dropped after this long without data.
const IDLE: Duration = Duration::from_secs(30);

/// What the receiver needs from the daemon.
#[derive(Debug, Clone)]
pub struct Config {
    pub alias: String,
    pub device_type: DeviceType,
    /// 0 picks a free port (tests); production uses [`proto::PORT`].
    pub port: u16,
    pub bind: IpAddr,
    /// Where accepted files are written; the daemon moves them from here.
    pub staging: PathBuf,
    /// Optional PIN the sender must give (`?pin=`).
    pub pin: Option<String>,
    /// How long an offer waits for the user before it counts as rejected.
    pub decide_timeout: Duration,
}

/// An incoming offer, waiting for the user.
#[derive(Debug, Clone)]
pub struct Offer {
    pub session_id: String,
    pub peer: IpAddr,
    /// SHA-256 of the sender's client certificate, when it sent one: what the user can pin.
    pub peer_cert: Option<String>,
    pub from: DeviceInfo,
    pub files: Vec<FileMeta>,
}

/// The user's answer to an [`Offer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// These file ids (a subset is fine).
    Accept(Vec<String>),
    Reject,
}

#[derive(Debug)]
pub enum Event {
    /// A device introduced itself (`register`).
    Seen { info: DeviceInfo, ip: IpAddr },
    Offer {
        offer: Offer,
        reply: oneshot::Sender<Decision>,
    },
    /// One file is complete and checked, at `path` in the staging folder.
    Received {
        session_id: String,
        file_id: String,
        name: String,
        path: PathBuf,
        size: u64,
    },
    /// Every accepted file of the session arrived.
    Finished { session_id: String },
    /// The sender cancelled; its partial files are deleted.
    Cancelled { session_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Waiting,
    Busy,
    Done,
}

#[derive(Debug)]
struct File {
    meta: FileMeta,
    token: String,
    slot: Slot,
    part: PathBuf,
}

#[derive(Debug)]
struct Session {
    id: String,
    peer: IpAddr,
    files: BTreeMap<String, File>,
}

#[derive(Debug)]
struct State {
    cfg: Config,
    info: DeviceInfo,
    session: Mutex<Option<Session>>,
    preparing: AtomicBool,
    offers: Mutex<HashMap<IpAddr, (Instant, u32)>>,
    events: mpsc::Sender<Event>,
}

/// A running receiver. Dropping it stops it.
#[derive(Debug)]
pub struct Receiver {
    addr: SocketAddr,
    fingerprint: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// 128 random bits as hex.
#[must_use]
pub fn random_id() -> String {
    use ring::rand::SecureRandom as _;
    let mut b = [0u8; 16];
    ring::rand::SystemRandom::new()
        .fill(&mut b)
        .expect("system random");
    b.iter().fold(String::with_capacity(32), |mut s, x| {
        use std::fmt::Write as _;
        let _ = write!(s, "{x:02x}");
        s
    })
}

/// Our own description as a receiver (HTTPS, upload API only).
#[must_use]
pub fn our_info(alias: &str, device_type: DeviceType, fingerprint: &str, port: u16) -> DeviceInfo {
    DeviceInfo {
        alias: proto::clean(alias, 64),
        version: proto::VERSION.to_owned(),
        device_model: Some("Vutric".to_owned()),
        device_type: Some(device_type),
        fingerprint: fingerprint.to_owned(),
        port: Some(port),
        protocol: Some(Scheme::Https),
        download: false,
        announce: None,
    }
}

impl Receiver {
    /// Binds and serves. Events (offers above all) arrive on the returned channel; an offer nobody answers
    /// within `decide_timeout` is rejected.
    pub async fn start(cfg: Config, identity: &Identity) -> Result<(Self, mpsc::Receiver<Event>)> {
        tokio::fs::create_dir_all(&cfg.staging)
            .await
            .with_context(|| format!("staging folder {}", cfg.staging.display()))?;
        let listener = TcpListener::bind((cfg.bind, cfg.port))
            .await
            .with_context(|| format!("bind LocalSend port {}", cfg.port))?;
        let addr = listener.local_addr()?;
        let fingerprint = identity.fingerprint();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(identity.server_config()?));
        let (tx, rx) = mpsc::channel(32);
        let st = Arc::new(State {
            info: our_info(&cfg.alias, cfg.device_type, &fingerprint, addr.port()),
            cfg,
            session: Mutex::new(None),
            preparing: AtomicBool::new(false),
            offers: Mutex::new(HashMap::new()),
            events: tx,
        });
        let task = tokio::spawn(serve(listener, acceptor, st));
        Ok((
            Self {
                addr,
                fingerprint,
                task,
            },
            rx,
        ))
    }

    #[must_use]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

async fn serve(listener: TcpListener, acceptor: tokio_rustls::TlsAcceptor, st: Arc<State>) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let Ok((tcp, peer)) = listener.accept().await else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let Ok(permit) = Arc::clone(&slots).try_acquire_owned() else {
            continue; // full: drop the connection
        };
        let acceptor = acceptor.clone();
        let st = Arc::clone(&st);
        tokio::spawn(async move {
            let _permit = permit;
            let Ok(Ok(tls)) =
                tokio::time::timeout(Duration::from_secs(10), acceptor.accept(tcp)).await
            else {
                return;
            };
            let cert = tls
                .get_ref()
                .1
                .peer_certificates()
                .and_then(|c| c.first())
                .map(|c| tls::fingerprint(c));
            let ip = peer.ip().to_canonical();
            let svc = hyper::service::service_fn(move |req| {
                let st = Arc::clone(&st);
                let cert = cert.clone();
                async move { Ok::<_, std::convert::Infallible>(handle(req, &st, ip, cert).await) }
            });
            let _ = hyper::server::conn::http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(Duration::from_secs(10))
                .serve_connection(TokioIo::new(tls), svc)
                .await;
        });
    }
}

fn reply(status: StatusCode, body: impl Into<Bytes>) -> Response<Full<Bytes>> {
    let mut r = Response::new(Full::new(body.into()));
    *r.status_mut() = status;
    r
}

fn json(value: &impl serde::Serialize) -> Response<Full<Bytes>> {
    let mut r = reply(
        StatusCode::OK,
        serde_json::to_vec(value).unwrap_or_default(),
    );
    r.headers_mut().insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/json"),
    );
    r
}

fn status(code: u16) -> Response<Full<Bytes>> {
    reply(
        StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_REQUEST),
        Bytes::new(),
    )
}

async fn read_body(req: Request<Incoming>, max: usize) -> Result<Bytes, u16> {
    match Limited::new(req.into_body(), max).collect().await {
        Ok(b) => Ok(b.to_bytes()),
        Err(e)
            if e.downcast_ref::<http_body_util::LengthLimitError>()
                .is_some() =>
        {
            Err(413)
        }
        Err(_) => Err(400),
    }
}

async fn handle(
    req: Request<Incoming>,
    st: &Arc<State>,
    ip: IpAddr,
    cert: Option<String>,
) -> Response<Full<Bytes>> {
    let Some(route) = req.uri().path().strip_prefix(API).map(str::to_owned) else {
        return status(404);
    };
    let q = proto::query(req.uri().query().unwrap_or_default());
    match (req.method().clone(), route.as_str()) {
        (Method::POST, "register") => match read_body(req, 16 * 1024).await {
            Ok(b) => match proto::parse_register(&b) {
                Ok(info) => {
                    let _ = st.events.try_send(Event::Seen { info, ip });
                    json(&st.info)
                }
                Err(e) => status(e.status()),
            },
            Err(code) => status(code),
        },
        (Method::GET, "info") => json(&st.info),
        (Method::POST, "prepare-upload") => prepare(req, st, ip, cert, &q).await,
        (Method::POST, "upload") => upload(req, st, ip, &q).await,
        (Method::POST, "cancel") => {
            cancel(st, ip, q.get("sessionId").map(String::as_str)).await;
            status(200)
        }
        _ => status(404),
    }
}

fn over_rate(st: &State, ip: IpAddr) -> bool {
    let now = Instant::now();
    let mut m = st.offers.lock().expect("rate lock");
    if m.len() > 4096 {
        m.retain(|_, (t, _)| now.duration_since(*t) < Duration::from_secs(60));
    }
    let e = m.entry(ip).or_insert((now, 0));
    if now.duration_since(e.0) >= Duration::from_secs(60) {
        *e = (now, 0);
    }
    e.1 += 1;
    e.1 > OFFERS_PER_MINUTE
}

struct Preparing<'a>(&'a AtomicBool);

impl Drop for Preparing<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

async fn prepare(
    req: Request<Incoming>,
    st: &Arc<State>,
    ip: IpAddr,
    cert: Option<String>,
    q: &BTreeMap<String, String>,
) -> Response<Full<Bytes>> {
    if over_rate(st, ip) {
        return status(429);
    }
    if let Some(pin) = &st.cfg.pin
        && q.get("pin") != Some(pin)
    {
        return status(401);
    }
    if st.session.lock().expect("session lock").is_some()
        || st.preparing.swap(true, Ordering::AcqRel)
    {
        return status(409);
    }
    let _guard = Preparing(&st.preparing);
    let body = match read_body(req, proto::MAX_BODY).await {
        Ok(b) => b,
        Err(code) => return status(code),
    };
    let p = match proto::parse_prepare_upload(&body) {
        Ok(p) => p,
        Err(e) => return status(e.status()),
    };
    let session_id = random_id();
    let (tx, rx) = oneshot::channel();
    let offer = Offer {
        session_id: session_id.clone(),
        peer: ip,
        peer_cert: cert,
        from: p.info,
        files: p.files.values().cloned().collect(),
    };
    if st
        .events
        .send(Event::Offer { offer, reply: tx })
        .await
        .is_err()
    {
        return status(500);
    }
    let decision = tokio::time::timeout(st.cfg.decide_timeout, rx)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or(Decision::Reject);
    let Decision::Accept(ids) = decision else {
        return status(403);
    };
    let mut files = BTreeMap::new();
    let mut tokens = BTreeMap::new();
    for (n, id) in ids.iter().enumerate() {
        let Some(meta) = p.files.get(id) else {
            continue;
        };
        let token = random_id();
        tokens.insert(id.clone(), token.clone());
        files.insert(
            id.clone(),
            File {
                meta: meta.clone(),
                token,
                slot: Slot::Waiting,
                part: st
                    .cfg
                    .staging
                    .join(format!("{}-{n}.vutric-part", &session_id[..8])),
            },
        );
    }
    if files.is_empty() {
        return status(403);
    }
    *st.session.lock().expect("session lock") = Some(Session {
        id: session_id.clone(),
        peer: ip,
        files,
    });
    json(&proto::PrepareResponse {
        session_id,
        files: tokens,
    })
}

enum Claim {
    Ok(Box<FileMeta>, PathBuf),
    Refuse(u16),
}

fn claim(st: &State, ip: IpAddr, q: &BTreeMap<String, String>) -> Claim {
    let (Some(sid), Some(fid), Some(token)) = (q.get("sessionId"), q.get("fileId"), q.get("token"))
    else {
        return Claim::Refuse(400);
    };
    let mut guard = st.session.lock().expect("session lock");
    let Some(s) = guard.as_mut() else {
        return Claim::Refuse(403);
    };
    if &s.id != sid || s.peer != ip {
        return Claim::Refuse(if &s.id == sid { 403 } else { 409 });
    }
    let Some(f) = s.files.get_mut(fid) else {
        return Claim::Refuse(403);
    };
    if &f.token != token {
        return Claim::Refuse(403);
    }
    if f.slot != Slot::Waiting {
        return Claim::Refuse(409);
    }
    f.slot = Slot::Busy;
    Claim::Ok(Box::new(f.meta.clone()), f.part.clone())
}

fn release(st: &State, sid: &str, fid: &str, done: bool) -> bool {
    let mut guard = st.session.lock().expect("session lock");
    let Some(s) = guard.as_mut().filter(|s| s.id == sid) else {
        return false;
    };
    if let Some(f) = s.files.get_mut(fid) {
        f.slot = if done { Slot::Done } else { Slot::Waiting };
    }
    let all = s.files.values().all(|f| f.slot == Slot::Done);
    if all {
        *guard = None;
    }
    all
}

/// Streams the body into `part`, never past `meta.size`. Returns the SHA-256 when the size matched.
async fn receive_body(body: Incoming, part: &PathBuf, size: u64) -> Result<[u8; 32], u16> {
    let file = tokio::fs::File::create(part).await.map_err(|_| 500u16)?;
    let mut out = tokio::io::BufWriter::with_capacity(256 * 1024, file);
    let mut hasher = Sha256::new();
    let mut got: u64 = 0;
    let mut body = body;
    loop {
        let frame = match tokio::time::timeout(IDLE, body.frame()).await {
            Err(_) | Ok(Some(Err(_))) => return Err(400),
            Ok(None) => break,
            Ok(Some(Ok(f))) => f,
        };
        let Ok(data) = frame.into_data() else {
            continue;
        };
        got += data.len() as u64;
        if got > size {
            return Err(400);
        }
        hasher.update(&data);
        out.write_all(&data).await.map_err(|_| 500u16)?;
    }
    out.flush().await.map_err(|_| 500u16)?;
    if got != size {
        return Err(400);
    }
    Ok(hasher.finalize().into())
}

fn hex(b: &[u8]) -> String {
    b.iter().fold(String::with_capacity(64), |mut s, x| {
        use std::fmt::Write as _;
        let _ = write!(s, "{x:02x}");
        s
    })
}

async fn upload(
    req: Request<Incoming>,
    st: &Arc<State>,
    ip: IpAddr,
    q: &BTreeMap<String, String>,
) -> Response<Full<Bytes>> {
    let (meta, part) = match claim(st, ip, q) {
        Claim::Ok(m, p) => (*m, p),
        Claim::Refuse(code) => return status(code),
    };
    let (sid, fid) = (q["sessionId"].clone(), q["fileId"].clone());
    let hash = match receive_body(req.into_body(), &part, meta.size).await {
        Ok(h) => h,
        Err(code) => {
            let _ = tokio::fs::remove_file(&part).await;
            release(st, &sid, &fid, false);
            return status(code);
        }
    };
    if meta
        .sha256
        .as_deref()
        .is_some_and(|want| want != hex(&hash))
    {
        let _ = tokio::fs::remove_file(&part).await;
        release(st, &sid, &fid, false);
        return status(422);
    }
    let staging = st.cfg.staging.clone();
    let name = vutric_core::filename::unique(&meta.file_name, |n| staging.join(n).exists());
    let path = staging.join(&name);
    if tokio::fs::rename(&part, &path).await.is_err() {
        let _ = tokio::fs::remove_file(&part).await;
        release(st, &sid, &fid, false);
        return status(500);
    }
    let _ = st
        .events
        .send(Event::Received {
            session_id: sid.clone(),
            file_id: fid.clone(),
            name,
            path,
            size: meta.size,
        })
        .await;
    if release(st, &sid, &fid, true) {
        let _ = st.events.send(Event::Finished { session_id: sid }).await;
    }
    status(200)
}

async fn cancel(st: &Arc<State>, ip: IpAddr, sid: Option<&str>) {
    let gone = {
        let mut guard = st.session.lock().expect("session lock");
        match guard.as_ref() {
            Some(s) if Some(s.id.as_str()) == sid && s.peer == ip => guard.take(),
            _ => None,
        }
    };
    if let Some(s) = gone {
        for f in s.files.values().filter(|f| f.slot != Slot::Done) {
            let _ = tokio::fs::remove_file(&f.part).await;
        }
        let _ = st.events.send(Event::Cancelled { session_id: s.id }).await;
    }
}
