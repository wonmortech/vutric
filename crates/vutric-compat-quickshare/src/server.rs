//! The receiving side: Android's Quick Share sends to Vutric on the LAN (backlog C3, ADR-0056).
//!
//! Order is enforced: connection request, UKEY2, connection responses, then inside the secure channel the
//! paired-key frames, the introduction, the user's answer, and only then file chunks. Nothing is written before
//! the user accepts; only payload ids from the introduction are written, in order, never past their announced
//! size, as `.vutric-part` in the staging folder until complete.

use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use tokio::io::{AsyncWriteExt as _, BufWriter};
use tokio::net::TcpListener;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::{Semaphore, mpsc, oneshot};

use crate::conn::{self, Bytes, Piece};
use crate::endpoint::{self, Device};
use crate::msg::{self, Offline, Share};
use crate::ukey2::{self, Channel};

const MAX_CONNECTIONS: usize = 8;
const CONNECTIONS_PER_MINUTE: u32 = 10;

#[derive(Debug, Clone)]
pub struct Config {
    /// Shown on the sender's screen.
    pub name: String,
    pub device: Device,
    /// 0 picks a free port; the port is advertised over mDNS, so any works.
    pub port: u16,
    pub bind: IpAddr,
    pub staging: PathBuf,
    pub decide_timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferFile {
    pub name: String,
    pub size: u64,
    pub mime: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferText {
    pub title: String,
    /// 1 text, 2 URL, 3 address, 4 phone number.
    pub kind: i64,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct Offer {
    pub session_id: String,
    pub peer: IpAddr,
    /// `None` when the sender is hidden.
    pub from: Option<String>,
    pub device: Device,
    /// The 4-digit code the sender shows too.
    pub pin: String,
    pub files: Vec<OfferFile>,
    pub texts: Vec<OfferText>,
}

/// Quick Share takes or refuses a transfer as a whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Accept,
    Reject,
}

#[derive(Debug)]
pub enum Event {
    Offer {
        offer: Offer,
        reply: oneshot::Sender<Decision>,
    },
    Received {
        session_id: String,
        name: String,
        path: PathBuf,
        size: u64,
    },
    Text {
        session_id: String,
        kind: i64,
        text: String,
    },
    Finished {
        session_id: String,
    },
    Cancelled {
        session_id: String,
    },
}

#[derive(Debug)]
struct State {
    cfg: Config,
    events: mpsc::Sender<Event>,
    rate: Mutex<HashMap<IpAddr, (Instant, u32)>>,
}

/// A running receiver; dropping it stops it.
#[derive(Debug)]
pub struct Receiver {
    addr: SocketAddr,
    endpoint_id: String,
    info: Vec<u8>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Receiver {
    pub async fn start(cfg: Config) -> Result<(Self, mpsc::Receiver<Event>)> {
        tokio::fs::create_dir_all(&cfg.staging)
            .await
            .with_context(|| format!("staging folder {}", cfg.staging.display()))?;
        let listener = TcpListener::bind((cfg.bind, cfg.port))
            .await
            .with_context(|| format!("bind Quick Share port {}", cfg.port))?;
        let addr = listener.local_addr()?;
        let info = endpoint::info(cfg.device, &cfg.name);
        let (tx, rx) = mpsc::channel(32);
        let st = Arc::new(State {
            cfg,
            events: tx,
            rate: Mutex::new(HashMap::new()),
        });
        let task = tokio::spawn(serve(listener, st));
        Ok((
            Self {
                addr,
                endpoint_id: endpoint::random_endpoint_id(),
                info,
                task,
            },
            rx,
        ))
    }

    #[must_use]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The 4-character id in our mDNS name.
    #[must_use]
    pub fn endpoint_id(&self) -> &str {
        &self.endpoint_id
    }

    /// Our endpoint info (the TXT `n` record, before base64).
    #[must_use]
    pub fn endpoint_info(&self) -> &[u8] {
        &self.info
    }
}

async fn serve(listener: TcpListener, st: Arc<State>) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let Ok((tcp, peer)) = listener.accept().await else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let ip = peer.ip().to_canonical();
        let Ok(permit) = Arc::clone(&slots).try_acquire_owned() else {
            continue;
        };
        if over_rate(&st, ip) {
            continue;
        }
        let _ = tcp.set_nodelay(true);
        let st = Arc::clone(&st);
        tokio::spawn(async move {
            let _permit = permit;
            let _ = connection(tcp, ip, &st).await;
        });
    }
}

fn over_rate(st: &State, ip: IpAddr) -> bool {
    let now = Instant::now();
    let mut m = st.rate.lock().expect("rate lock");
    if m.len() > 4096 {
        m.retain(|_, (t, _)| now.duration_since(*t) < Duration::from_secs(60));
    }
    let e = m.entry(ip).or_insert((now, 0));
    if now.duration_since(e.0) >= Duration::from_secs(60) {
        *e = (now, 0);
    }
    e.1 += 1;
    e.1 > CONNECTIONS_PER_MINUTE
}

fn random_hex() -> String {
    use ring::rand::SecureRandom as _;
    let mut b = [0u8; 16];
    ring::rand::SystemRandom::new()
        .fill(&mut b)
        .expect("system random");
    b.iter().fold(String::new(), |mut s, x| {
        use std::fmt::Write as _;
        let _ = write!(s, "{x:02x}");
        s
    })
}

/// Connection request, UKEY2 and the plaintext connection responses; returns the peer and the secure channel.
async fn handshake(
    rx: &mut mpsc::Receiver<Vec<u8>>,
    gate: &conn::Gate,
    w: &mut OwnedWriteHalf,
) -> Result<(endpoint::Peer, String, Channel)> {
    let f = conn::next(rx).await.context("no connection request")?;
    let Some(Offline::ConnectionRequest { endpoint_info, .. }) = msg::parse_offline(&f) else {
        bail!("expected a connection request");
    };
    let peer = endpoint::parse(endpoint_info).context("endpoint info")?;
    let init = conn::next(rx).await.context("no client init")?;
    let (hs, server_init) = match ukey2::Server::start(&init) {
        Ok(v) => v,
        Err(e) => {
            let _ = conn::write_frame(w, &msg::alert(e as i64)).await;
            return Err(e.into());
        }
    };
    conn::write_frame(w, &server_init).await?;
    let fin = conn::next(rx).await.context("no client finished")?;
    let keys = hs.finish(&fin)?;
    gate.authenticated();
    let f = conn::next(rx).await.context("no connection response")?;
    if msg::parse_offline(&f) != Some(Offline::ConnectionResponse { accepted: true }) {
        bail!("sender did not confirm the connection");
    }
    conn::write_frame(w, &msg::connection_response(true)).await?;
    Ok((peer, keys.pin(), Channel::new(&keys, true)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    PairedKey,
    PairedResult,
    Introduction,
    Deciding,
    Receiving,
}

#[derive(Debug)]
struct Incoming {
    name: String,
    size: u64,
    written: u64,
    part: PathBuf,
    file: Option<BufWriter<tokio::fs::File>>,
    done: bool,
}

struct Session<'a> {
    st: &'a State,
    ip: IpAddr,
    id: String,
    peer: endpoint::Peer,
    pin: String,
    w: OwnedWriteHalf,
    ch: Channel,
    step: Step,
    bytes: Bytes,
    files: HashMap<i64, Incoming>,
    texts: HashMap<i64, (i64, bool)>,
    offered: bool,
    decision: Option<Pin<Box<dyn Future<Output = Decision> + Send>>>,
}

/// What the loop does next.
enum Flow {
    Go,
    Done,
    Abort,
}

async fn connection(tcp: tokio::net::TcpStream, ip: IpAddr, st: &State) -> Result<()> {
    let (r, mut w) = tcp.into_split();
    let (mut rx, gate) = conn::gated_reader(r);
    let (peer, pin, ch) = handshake(&mut rx, &gate, &mut w).await?;
    let mut s = Session {
        st,
        ip,
        id: random_hex(),
        peer,
        pin,
        w,
        ch,
        step: Step::PairedKey,
        bytes: Bytes::default(),
        files: HashMap::new(),
        texts: HashMap::new(),
        offered: false,
        decision: None,
    };
    let mut secret = [0u8; 78];
    {
        use ring::rand::SecureRandom as _;
        ring::rand::SystemRandom::new()
            .fill(&mut secret)
            .expect("system random");
    }
    let (hash, signed) = secret.split_at(6);
    s.share(&msg::paired_key_encryption(
        hash.try_into().expect("6"),
        signed.try_into().expect("72"),
    ))
    .await?;
    let mut tick = tokio::time::interval_at(
        tokio::time::Instant::now() + conn::KEEP_ALIVE,
        conn::KEEP_ALIVE,
    );
    let flow = loop {
        let flow = tokio::select! {
            f = conn::next(&mut rx) => match f {
                Some(f) => s.frame(&f).await,
                None => Flow::Abort,
            },
            d = async { s.decision.as_mut().expect("guarded").await }, if s.decision.is_some() => {
                s.decision = None;
                s.decided(d).await
            }
            _ = tick.tick() => match conn::send(&mut s.w, &mut s.ch, &msg::keep_alive(false)).await {
                Ok(()) => Flow::Go,
                Err(_) => Flow::Abort,
            },
        };
        match flow {
            Flow::Go => {}
            other => break other,
        }
    };
    if matches!(flow, Flow::Abort) {
        s.abort().await;
    }
    let _ = conn::send(&mut s.w, &mut s.ch, &msg::disconnection()).await;
    let _ = s.w.shutdown().await;
    Ok(())
}

impl Session<'_> {
    async fn share(&mut self, frame: &[u8]) -> std::io::Result<()> {
        conn::send_share(&mut self.w, &mut self.ch, frame).await
    }

    async fn frame(&mut self, wire: &[u8]) -> Flow {
        let Ok(plain) = self.ch.open(wire) else {
            return Flow::Abort;
        };
        let Some(frame) = msg::parse_offline(&plain) else {
            return Flow::Abort;
        };
        match &frame {
            Offline::KeepAlive { ack: false } => {
                if conn::send(&mut self.w, &mut self.ch, &msg::keep_alive(true))
                    .await
                    .is_err()
                {
                    return Flow::Abort;
                }
                Flow::Go
            }
            Offline::Disconnection => {
                if self.complete() {
                    Flow::Done
                } else {
                    Flow::Abort
                }
            }
            Offline::Payload {
                control: Some(_), ..
            } => Flow::Abort,
            Offline::Payload { header, chunk, .. } => match self.bytes.feed(&frame) {
                Piece::Message(id, b) => self.message(id, b).await,
                Piece::File => match chunk {
                    Some(c) => self.chunk(header.id, header.total_size, c).await,
                    None => Flow::Go,
                },
                Piece::Nothing => Flow::Go,
                Piece::Bad => Flow::Abort,
            },
            _ => Flow::Go,
        }
    }

    async fn message(&mut self, id: i64, b: Vec<u8>) -> Flow {
        if self.step == Step::Receiving
            && let Some((kind, done)) = self.texts.get_mut(&id)
        {
            if *done {
                return Flow::Abort;
            }
            *done = true;
            let kind = *kind;
            let text = String::from_utf8_lossy(&b).into_owned();
            let _ = self
                .st
                .events
                .send(Event::Text {
                    session_id: self.id.clone(),
                    kind,
                    text,
                })
                .await;
            return self.after_piece().await;
        }
        match (self.step, msg::parse_share(&b)) {
            (Step::PairedKey, Some(Share::PairedKeyEncryption)) => {
                self.step = Step::PairedResult;
                match self.share(&msg::paired_key_result()).await {
                    Ok(()) => Flow::Go,
                    Err(_) => Flow::Abort,
                }
            }
            (Step::PairedResult, Some(Share::PairedKeyResult)) => {
                self.step = Step::Introduction;
                Flow::Go
            }
            (Step::Introduction, Some(Share::Introduction { files, texts })) => {
                self.introduction(files, texts).await
            }
            (_, Some(Share::Cancel) | None) => Flow::Abort,
            _ => Flow::Go,
        }
    }

    async fn introduction(&mut self, files: Vec<msg::FileMeta>, texts: Vec<msg::TextMeta>) -> Flow {
        if files.is_empty() && texts.is_empty() {
            let _ = self.share(&msg::response(msg::UNSUPPORTED)).await;
            return Flow::Done;
        }
        let mut offer_files = Vec::new();
        for (n, f) in files.iter().enumerate() {
            if self.files.contains_key(&f.payload_id) {
                return Flow::Abort;
            }
            let raw = if f.name.trim().is_empty() {
                "file"
            } else {
                &f.name
            };
            let name = vutric_core::filename::sanitize(raw);
            let size = f.size.unsigned_abs();
            self.files.insert(
                f.payload_id,
                Incoming {
                    name: name.clone(),
                    size,
                    written: 0,
                    part: self
                        .st
                        .cfg
                        .staging
                        .join(format!("{}-{n}.vutric-part", &self.id[..8])),
                    file: None,
                    done: false,
                },
            );
            offer_files.push(OfferFile {
                name,
                size,
                mime: f.mime.clone(),
            });
        }
        let mut offer_texts = Vec::new();
        for t in &texts {
            if self.texts.contains_key(&t.payload_id) || self.files.contains_key(&t.payload_id) {
                return Flow::Abort;
            }
            self.texts.insert(t.payload_id, (t.kind, false));
            offer_texts.push(OfferText {
                title: t.title.clone(),
                kind: t.kind,
                size: t.size.unsigned_abs(),
            });
        }
        let (tx, rx) = oneshot::channel();
        let offer = Offer {
            session_id: self.id.clone(),
            peer: self.ip,
            from: self.peer.name.clone(),
            device: self.peer.device,
            pin: self.pin.clone(),
            files: offer_files,
            texts: offer_texts,
        };
        if self
            .st
            .events
            .send(Event::Offer { offer, reply: tx })
            .await
            .is_err()
        {
            return Flow::Abort;
        }
        self.offered = true;
        let wait = self.st.cfg.decide_timeout;
        self.decision = Some(Box::pin(async move {
            tokio::time::timeout(wait, rx)
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or(Decision::Reject)
        }));
        self.step = Step::Deciding;
        Flow::Go
    }

    async fn decided(&mut self, d: Decision) -> Flow {
        if d == Decision::Accept {
            self.step = Step::Receiving;
            match self.share(&msg::response(msg::ACCEPT)).await {
                Ok(()) => Flow::Go,
                Err(_) => Flow::Abort,
            }
        } else {
            let _ = self.share(&msg::response(msg::REJECT)).await;
            self.files.clear();
            self.texts.clear();
            self.offered = false;
            Flow::Done
        }
    }

    async fn chunk(&mut self, id: i64, total: i64, c: &msg::Chunk<'_>) -> Flow {
        if self.step != Step::Receiving {
            return Flow::Abort;
        }
        let Some(f) = self.files.get_mut(&id) else {
            return Flow::Abort;
        };
        let len = c.body.len() as u64;
        if f.done
            || total.unsigned_abs() != f.size
            || c.offset.unsigned_abs() != f.written
            || f.written + len > f.size
        {
            return Flow::Abort;
        }
        if f.file.is_none() {
            match tokio::fs::File::create(&f.part).await {
                Ok(h) => f.file = Some(BufWriter::with_capacity(256 * 1024, h)),
                Err(_) => return Flow::Abort,
            }
        }
        let out = f.file.as_mut().expect("opened");
        if out.write_all(c.body).await.is_err() {
            return Flow::Abort;
        }
        f.written += len;
        if c.flags & msg::LAST_CHUNK == 0 {
            return Flow::Go;
        }
        if f.written != f.size || out.flush().await.is_err() {
            return Flow::Abort;
        }
        f.file = None;
        let staging = self.st.cfg.staging.clone();
        let name = vutric_core::filename::unique(&f.name, |n| staging.join(n).exists());
        let path = staging.join(&name);
        if tokio::fs::rename(&f.part, &path).await.is_err() {
            return Flow::Abort;
        }
        f.done = true;
        let size = f.size;
        let _ = self
            .st
            .events
            .send(Event::Received {
                session_id: self.id.clone(),
                name,
                path,
                size,
            })
            .await;
        self.after_piece().await
    }

    fn complete(&self) -> bool {
        self.files.values().all(|f| f.done) && self.texts.values().all(|t| t.1)
    }

    async fn after_piece(&mut self) -> Flow {
        if !self.complete() {
            return Flow::Go;
        }
        let _ = self
            .st
            .events
            .send(Event::Finished {
                session_id: self.id.clone(),
            })
            .await;
        self.offered = false;
        Flow::Done
    }

    /// Deletes unfinished parts and tells the daemon, if it had seen an offer.
    async fn abort(&mut self) {
        for f in self.files.values_mut().filter(|f| !f.done) {
            f.file = None;
            let _ = tokio::fs::remove_file(&f.part).await;
        }
        if self.offered {
            let _ = self
                .st
                .events
                .send(Event::Cancelled {
                    session_id: self.id.clone(),
                })
                .await;
        }
    }
}
