//! The API the Flutter app calls (`flutter_rust_bridge` generates the Dart side).
//!
//! Every function here is blocking and runs on a bridge worker thread, never the UI thread. Inside, the peer
//! lives on one Tokio runtime owned by this crate, so iroh always has the runtime it needs.
//!
//! Two rules keep the screens honest:
//! - **Every wait can be cancelled.** [`disconnect`] (and [`start`], and opening a new session) moves the
//!   *epoch* on; every blocking call watches it and returns [`CANCELLED`] at once. A page that closes therefore
//!   never leaves a bridge thread parked, and a stale call can never touch a newer session.
//! - **The open session stays in its slot.** Calls borrow it through an async lock instead of taking it out, so
//!   [`disconnect`] always finds it. A call that fails on it (the peer vanished, an answer timed out) drops it,
//!   and the screen goes back to "connect".

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use tokio::runtime::Runtime;
use tokio::sync::{Mutex as AsyncMutex, Notify, watch};
use vutric_core::message::FailReason;
use vutric_core::{FlingId, Message, PairingTicket, Platform};
use vutric_net::{Peer, Reach, Route, Session, preview, probe, transfer};

use crate::frb_generated::StreamSink;

#[cfg(target_os = "ios")]
const PLATFORM: Platform = Platform::Ios;
#[cfg(not(target_os = "ios"))]
const PLATFORM: Platform = Platform::Android;

/// The error text of a call that [`disconnect`] (or a newer session) cancelled; the app ignores it.
pub const CANCELLED: &str = "vutric: cancelled";

/// How long "find a computer" looks over mDNS.
const DISCOVER_TIMEOUT: Duration = Duration::from_secs(15);

fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("vutric")
            .build()
            .expect("tokio runtime")
    })
}

// ---- Epoch: cancels every wait in flight ----

fn epoch() -> &'static watch::Sender<u64> {
    static EPOCH: OnceLock<watch::Sender<u64>> = OnceLock::new();
    EPOCH.get_or_init(|| watch::channel(0).0)
}

fn now_epoch() -> u64 {
    *epoch().borrow()
}

/// Moves the epoch on: every call that started before returns [`CANCELLED`].
fn bump() -> u64 {
    epoch().send_modify(|e| *e += 1);
    now_epoch()
}

/// Runs `fut` until it finishes or the epoch leaves `at`.
async fn cancellable<T>(at: u64, fut: impl Future<Output = Result<T>>) -> Result<T> {
    let mut rx = epoch().subscribe();
    tokio::select! {
        r = fut => r,
        _ = rx.wait_for(|e| *e != at) => bail!(CANCELLED),
    }
}

// ---- The peer (this device on the LAN) ----

fn peer_slot() -> &'static Mutex<Option<Peer>> {
    static PEER: Mutex<Option<Peer>> = Mutex::new(None);
    &PEER
}

fn peer() -> Result<Peer> {
    peer_slot()
        .lock()
        .expect("peer lock")
        .clone()
        .context("call start() first")
}

/// 32 random bytes to keep forever as this device's identity.
#[must_use]
pub fn new_identity() -> Vec<u8> {
    vutric_net::new_identity().to_vec()
}

/// Starts (or restarts, e.g. after a rename) this device's peer; returns its pairing link for the QR code.
/// `lan_only` keeps it off the internet; otherwise it is reachable on this network first and, failing that,
/// over the internet (direct through NAT where possible, else an encrypted relay). Cancels whatever was
/// running and closes the open session. Returns at once: the link carries this network's addresses, and
/// [`wait_online`] then [`current_link`] give the one that also carries the internet relay.
pub fn start(name: String, identity: Vec<u8>, lan_only: bool) -> Result<String> {
    static STARTING: Mutex<()> = Mutex::new(());
    let key: [u8; 32] = identity
        .as_slice()
        .try_into()
        .context("identity must be 32 bytes")?;
    // One restart at a time, so two quick renames cannot leave a second peer advertising unseen.
    let _one = STARTING.lock().expect("start lock");
    drop_session();
    let old = peer_slot().lock().expect("peer lock").take();
    runtime().block_on(async {
        if let Some(old) = old {
            old.close().await;
        }
        let reach = if lan_only {
            Reach::Lan
        } else {
            Reach::Anywhere
        };
        let p = Peer::bind(key, &name, PLATFORM, reach).await?;
        let link = p.ticket().to_uri();
        *peer_slot().lock().expect("peer lock") = Some(p);
        Ok(link)
    })
}

/// Waits up to `max_ms` for the internet relay (about 3 s on a good line). True: devices on other networks
/// can reach this one too. False at once in LAN-only mode.
pub fn wait_online(max_ms: u32) -> Result<bool> {
    let p = peer()?;
    Ok(runtime().block_on(p.wait_online(Duration::from_millis(max_ms.into()))))
}

/// This device's pairing link as of now; once online it also carries the relay.
pub fn current_link() -> Result<String> {
    Ok(peer()?.ticket().to_uri())
}

// ---- Measurement runs (spike 0.2) ----

/// What one spike run measured, both directions, for the screen and the report.
#[derive(Debug, Clone)]
pub struct RunReport {
    pub peer_name: String,
    pub peer_platform: String,
    pub check_code: String,
    pub found_ms: f64,
    pub connect_ms: f64,
    /// This phone → the peer, summary line.
    pub sent: String,
    /// One-way estimate (half the round trip), this phone → the peer.
    pub sent_p50_ms: f64,
    pub sent_p95_ms: f64,
    /// Probes of the peer → this phone that we answered; their numbers print on the peer's side.
    pub received_probes: u32,
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

struct Sent {
    summary: String,
    p50: f64,
    p95: f64,
}

fn sent_of(stats: &probe::Stats) -> Sent {
    let half = |q| stats.rtt_percentile(q).map_or(f64::NAN, ms) / 2.0;
    Sent {
        summary: stats.summary(),
        p50: half(50),
        p95: half(95),
    }
}

fn report(
    s: &Session,
    found: Duration,
    connect: Duration,
    sent: Sent,
    runs: u32,
) -> Result<RunReport> {
    Ok(RunReport {
        peer_name: s.remote_name.clone(),
        peer_platform: format!("{:?}", s.remote_platform),
        check_code: s.check_code()?,
        found_ms: ms(found),
        connect_ms: ms(connect),
        sent: sent.summary,
        sent_p50_ms: sent.p50,
        sent_p95_ms: sent.p95,
        received_probes: probe::WARMUP + runs,
    })
}

/// We connected: send our probes, answer the peer's, then close once the peer has our last answer.
async fn as_initiator(
    mut s: Session,
    found: Duration,
    connect: Duration,
    runs: u32,
) -> Result<RunReport> {
    let (_warm, stats) = probe::send_run(&mut s, runs).await?;
    probe::answer_run(&mut s, runs).await?;
    let r = report(&s, found, connect, sent_of(&stats), runs);
    s.finish().await;
    r
}

/// Finds a peer over mDNS, then measures both directions. Cancelled by [`disconnect`].
pub fn run_discovered(runs: u32) -> Result<RunReport> {
    let p = peer()?;
    let at = now_epoch();
    runtime().block_on(cancellable(at, async {
        let t0 = Instant::now();
        let near = p.discover(DISCOVER_TIMEOUT).await?;
        let found = t0.elapsed();
        let t1 = Instant::now();
        let s = p.connect(near.id, &near.addrs).await?;
        as_initiator(s, found, t1.elapsed(), runs).await
    }))
}

/// Connects with a pairing link (scanned or pasted), then measures both directions. Cancelled by [`disconnect`].
pub fn run_link(link: String, runs: u32) -> Result<RunReport> {
    let p = peer()?;
    let t = PairingTicket::parse(link.trim())?;
    let at = now_epoch();
    runtime().block_on(cancellable(at, async {
        let t1 = Instant::now();
        let s = p.connect_ticket(&t).await?;
        as_initiator(s, Duration::ZERO, t1.elapsed(), runs).await
    }))
}

/// Waits for a peer to connect (desktop: `vutric-net-spike send --discover`), answers its probes, then
/// measures this phone → that peer. Cancelled by [`disconnect`].
pub fn listen(runs: u32) -> Result<RunReport> {
    let p = peer()?;
    let at = now_epoch();
    runtime().block_on(cancellable(at, async {
        let t1 = Instant::now();
        let mut s = p.accept().await?;
        let waited = t1.elapsed();
        probe::answer_run(&mut s, runs).await?;
        let (_warm, stats) = probe::send_run(&mut s, runs).await?;
        let r = report(&s, waited, Duration::ZERO, sent_of(&stats), runs);
        s.close(); // our last act was a receive: nothing of ours is in flight
        r
    }))
}

// ---- The open session (fling and catch) ----

type Shared = Arc<AsyncMutex<Session>>;

fn session_slot() -> &'static Mutex<Option<Shared>> {
    static SESSION: Mutex<Option<Shared>> = Mutex::new(None);
    &SESSION
}

/// The open session, or an error the app shows as "connect first".
fn session() -> Result<(u64, Shared)> {
    // The epoch is read under the slot lock, like every bump that retires a session: a call can never hold a
    // retired session together with an epoch that will not move again.
    let slot = session_slot().lock().expect("session lock");
    let s = slot.clone().context("not connected to a device")?;
    Ok((now_epoch(), s))
}

/// Closes a session in the background once no call is using it, after its last bytes left.
fn retire(s: Shared) {
    runtime().spawn(async move {
        s.lock().await.finish().await;
    });
}

/// Cancels every waiting call and closes the open session (the epoch moves under the slot lock).
fn drop_session() {
    let mut slot = session_slot().lock().expect("session lock");
    bump();
    let old = slot.take();
    drop(slot);
    if let Some(s) = old {
        retire(s);
    }
}

/// After a call failed on `s`: forget it if it is still the open one. A photo the other device refused leaves
/// a healthy session, so that keeps it. (A cancelled call needs nothing: whatever cancelled it also retired
/// the session.)
fn forget_on_error<T>(s: &Shared, r: Result<T>) -> Result<T> {
    if let Err(e) = &r
        && e.to_string() != CANCELLED
        && e.downcast_ref::<probe::Refused>().is_none()
        && e.downcast_ref::<transfer::Declined>().is_none()
    {
        let mut slot = session_slot().lock().expect("session lock");
        if slot.as_ref().is_some_and(|cur| Arc::ptr_eq(cur, s))
            && let Some(dead) = slot.take()
        {
            retire(dead);
        }
    }
    r
}

/// The device a fling goes to (or comes from), shown with the pairing check.
#[derive(Debug, Clone)]
pub struct Connected {
    pub peer_name: String,
    pub peer_platform: String,
    pub check_code: String,
    pub connect_ms: f64,
    /// "direct" (same network, or through NAT), "relay" (encrypted, through the internet relay) or "" while
    /// the path is still being chosen.
    pub route: String,
}

fn route_name(s: &Session) -> String {
    match s.route().0 {
        Route::Direct => "direct".into(),
        Route::Relay => "relay".into(),
        Route::Unknown => String::new(),
    }
}

/// How the open session's packets travel right now (it can move from "relay" to "direct" a moment after
/// connecting); "" when not connected.
#[must_use]
pub fn current_route() -> String {
    let Ok((_, s)) = session() else {
        return String::new();
    };
    runtime().block_on(async { route_name(&*s.lock().await) })
}

/// Makes `s` the open session, unless the call that opened it was cancelled meanwhile (then `s` is closed).
fn keep(at: u64, s: Session, connect: Duration) -> Result<Connected> {
    let c = Connected {
        peer_name: s.remote_name.clone(),
        peer_platform: format!("{:?}", s.remote_platform),
        check_code: s.check_code()?,
        connect_ms: ms(connect),
        route: route_name(&s),
    };
    let mut slot = session_slot().lock().expect("session lock");
    if now_epoch() != at {
        drop(slot);
        retire(Arc::new(AsyncMutex::new(s)));
        bail!(CANCELLED);
    }
    // A new session cancels every call still waiting on the old one.
    bump();
    if let Some(old) = slot.replace(Arc::new(AsyncMutex::new(s))) {
        retire(old);
    }
    Ok(c)
}

fn open(job: impl Future<Output = Result<Session>>) -> Result<Connected> {
    let at = now_epoch();
    runtime().block_on(async {
        let t0 = Instant::now();
        let s = cancellable(at, job).await?;
        keep(at, s, t0.elapsed())
    })
}

/// Opens the session with a pairing link (scanned or pasted). Cancelled by [`disconnect`].
pub fn connect_link(link: String) -> Result<Connected> {
    let p = peer()?;
    let t = PairingTicket::parse(link.trim())?;
    open(async move { p.connect_ticket(&t).await })
}

/// Opens the session with the first device found over mDNS (not on iPhone: no multicast there). Cancelled by
/// [`disconnect`].
pub fn connect_discovered() -> Result<Connected> {
    let p = peer()?;
    open(async move {
        let near = p.discover(DISCOVER_TIMEOUT).await?;
        p.connect(near.id, &near.addrs).await
    })
}

/// Waits for another device to connect to this phone (it scanned our QR code) and keeps that session.
/// Cancelled by [`disconnect`].
pub fn accept_session() -> Result<Connected> {
    let p = peer()?;
    open(async move { p.accept().await })
}

/// Closes the open session and cancels every call still waiting (on it or on opening one). Returns at once.
pub fn disconnect() {
    drop_session();
}

// ---- Throw ----

/// A photo ready to throw: an upright JPEG, longest side at most 512 px, small enough for one frame. Prepared
/// when picked, so the throw itself only sends bytes.
#[derive(Debug, Clone)]
pub struct Photo {
    pub file_name: String,
    pub jpeg: Vec<u8>,
    pub width: u16,
    pub height: u16,
}

/// Any picked image (JPEG, PNG or WebP) → the preview the receiver plays.
pub fn prepare_photo(file_name: String, bytes: Vec<u8>) -> Result<Photo> {
    let e = preview::encode(&bytes)?;
    // The protocol allows 1..=512 bytes of name; a picker can hand anything over.
    let mut name: String = file_name.chars().filter(|c| !c.is_control()).collect();
    while name.len() > 200 {
        name.pop();
    }
    if name.trim().is_empty() {
        name = "photo.jpg".into();
    }
    Ok(Photo {
        file_name: name,
        jpeg: e.jpeg,
        width: e.width,
        height: e.height,
    })
}

/// Throws the photo; returns milliseconds from the call until the receiver reported its first frame on screen
/// (there and back, so an upper bound of "release → first frame"). Fails after
/// [`probe::ROUND_TRIP_TIMEOUT`] without an answer, and then the session is closed.
pub fn fling(photo: Photo, speed: f32, angle: f32, exit_x: f32) -> Result<f64> {
    let finite = |v: f32, or: f32| if v.is_finite() { v } else { or };
    let gesture = vutric_core::Gesture {
        speed: finite(speed, 0.0),
        angle: finite(angle, 0.0),
        exit_x: finite(exit_x, 0.5).clamp(0.0, 1.0),
    };
    let msg = probe::photo(
        &photo.file_name,
        photo.jpeg,
        photo.width,
        photo.height,
        gesture,
    );
    // A message this side would refuse to send is the caller's problem, not the session's.
    msg.validate()?;
    let (at, s) = session()?;
    let r = runtime().block_on(cancellable(at, async {
        let mut guard = s.lock().await;
        probe::round_trip(&mut guard, &msg).await
    }));
    forget_on_error(&s, r).map(ms)
}

// ---- Catch: this phone receives (desktop → phone, phone → phone) ----

/// A photo thrown at this phone; its preview has been checked (a real JPEG, preview-sized, as announced).
#[derive(Debug, Clone)]
pub struct Caught {
    /// Pass back to [`shown`] once the photo is on screen.
    pub id: Vec<u8>,
    pub file_name: String,
    pub jpeg: Vec<u8>,
    pub width: u16,
    pub height: u16,
    pub speed: f32,
    pub angle: f32,
    pub exit_x: f32,
}

fn receiver_clock() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

/// Files another device offers this phone: answer with [`receive_files`] or [`decline_files`].
#[derive(Debug, Clone)]
pub struct FilesOffer {
    pub files: u32,
    pub bytes: u64,
    /// The first few paths, to show.
    pub names: Vec<String>,
    /// The user's settings say yes without asking (accept rule and this device's override).
    pub auto_accept: bool,
}

// ---- Settings (ADR-0007 part 4) ----

fn settings_path() -> &'static Mutex<Option<std::path::PathBuf>> {
    static PATH: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);
    &PATH
}

/// Where this phone keeps its settings file (the app's support folder). Call once at start.
pub fn set_settings_dir(dir: String) {
    *settings_path().lock().expect("settings lock") =
        Some(std::path::Path::new(&dir).join(vutric_net::settings::FILE));
}

fn load_settings() -> vutric_net::settings::Settings {
    settings_path()
        .lock()
        .expect("settings lock")
        .as_deref()
        .map(vutric_net::settings::load)
        .unwrap_or_default()
}

/// The settings as JSON (every field present, defaults filled in), for the Settings screen.
pub fn get_settings() -> Result<String> {
    Ok(serde_json::to_string(&load_settings())?)
}

/// Saves settings edited on the Settings screen. Invalid JSON is rejected whole, never half-applied; values out of
/// range are clamped.
pub fn put_settings(json: String) -> Result<()> {
    let s: vutric_net::settings::Settings =
        serde_json::from_str(&json).context("settings are not valid")?;
    let path = settings_path()
        .lock()
        .expect("settings lock")
        .clone()
        .context("settings folder not set")?;
    vutric_net::settings::save(&path, &s.migrate())
}

/// What arrived on the open session: a photo thrown at this phone, or files offered to it.
#[derive(Debug, Clone)]
pub struct Incoming {
    pub photo: Option<Caught>,
    pub files: Option<FilesOffer>,
}

/// The offer [`next_incoming`] returned, with the session it came on.
fn pending_slot() -> &'static Mutex<Option<(Shared, transfer::IncomingOffer)>> {
    static PENDING: Mutex<Option<(Shared, transfer::IncomingOffer)>> = Mutex::new(None);
    &PENDING
}

/// Set by [`pause_incoming`], cleared by [`resume_incoming`].
fn pause_flag() -> &'static AtomicBool {
    static PAUSED: AtomicBool = AtomicBool::new(false);
    &PAUSED
}

fn pause_signal() -> &'static Notify {
    static SIGNAL: OnceLock<Notify> = OnceLock::new();
    SIGNAL.get_or_init(Notify::new)
}

/// Makes a waiting [`next_incoming`] return an empty [`Incoming`] at once and let go of the session, so this side
/// can throw or send on the same session; calls made while paused return empty at once too. Nothing is lost:
/// whatever the other side sends meanwhile waits in the stream until [`resume_incoming`] and the next call.
pub fn pause_incoming() {
    pause_flag().store(true, Ordering::SeqCst);
    pause_signal().notify_waiters();
}

/// Lets [`next_incoming`] wait for the other side again after [`pause_incoming`].
pub fn resume_incoming() {
    pause_flag().store(false, Ordering::SeqCst);
}

/// Waits for the next photo or file offer on the open session (either side may have opened it). Latency probes
/// are answered here and never shown; a fling whose preview is not a sane JPEG is refused (`FlingFailed`), so its
/// thrower learns at once. Returns an empty [`Incoming`] (neither photo nor files) when [`pause_incoming`] asks
/// for the session. Cancelled by [`disconnect`].
pub fn next_incoming() -> Result<Incoming> {
    let paused = || Incoming {
        photo: None,
        files: None,
    };
    let (at, s) = session()?;
    let r = runtime().block_on(cancellable(at, async {
        let mut guard = s.lock().await;
        loop {
            // Register for the pause signal before checking the flag, so a pause between the two is never missed.
            // `recv` only buffers what it read (cancel-safe), so giving up the wait loses no message.
            let signal = pause_signal().notified();
            tokio::pin!(signal);
            signal.as_mut().enable();
            if pause_flag().load(Ordering::SeqCst) {
                return Ok(paused());
            }
            let msg = tokio::select! {
                biased;
                () = &mut signal => return Ok(paused()),
                msg = guard.recv() => msg?,
            };
            if let Some(offer) = transfer::IncomingOffer::from_message(&msg) {
                let (_, accept) = load_settings().rules_for(&guard.remote_id().to_string());
                let files = FilesOffer {
                    files: offer.total_files,
                    bytes: offer.total_bytes,
                    names: offer.names(5),
                    // Only paired devices, checked by key, ever reach a session: the sender is trusted.
                    auto_accept: accept.auto(true, offer.total_bytes),
                };
                *pending_slot().lock().expect("pending lock") = Some((Arc::clone(&s), offer));
                return Ok(Incoming {
                    photo: None,
                    files: Some(files),
                });
            }
            let Message::FlingStart(f) = msg else {
                continue;
            };
            if probe::is_probe(&f) {
                guard
                    .send(&Message::EffectStarted {
                        id: f.id,
                        receiver_micros: vutric_net::micros_since(receiver_clock()),
                    })
                    .await?;
                continue;
            }
            let checked = f.preview.as_ref().map_or_else(
                || Err(anyhow::anyhow!("no preview")),
                |p| preview::check(&p.bytes, (p.width, p.height)),
            );
            let (Ok(()), Some(p)) = (checked, f.preview) else {
                guard
                    .send(&Message::FlingFailed {
                        id: f.id,
                        reason: FailReason::Rejected,
                    })
                    .await?;
                continue;
            };
            return Ok(Incoming {
                photo: Some(Caught {
                    id: f.id.0.to_vec(),
                    file_name: f.file_name,
                    jpeg: p.bytes,
                    width: p.width,
                    height: p.height,
                    speed: f.gesture.speed,
                    angle: f.gesture.angle,
                    exit_x: f.gesture.exit_x,
                }),
                files: None,
            });
        }
    }));
    forget_on_error(&s, r)
}

/// Tells the thrower this phone could not show the photo after all (its round trip ends with a refusal, at
/// once, instead of a timeout).
pub fn refuse(id: Vec<u8>) -> Result<()> {
    let id = FlingId(
        id.as_slice()
            .try_into()
            .context("fling id must be 16 bytes")?,
    );
    let (at, s) = session()?;
    let r = runtime().block_on(cancellable(at, async {
        s.lock()
            .await
            .send(&Message::FlingFailed {
                id,
                reason: FailReason::Rejected,
            })
            .await
    }));
    forget_on_error(&s, r)
}

/// Tells the thrower that the photo's first frame is on this screen (closes its round trip).
pub fn shown(id: Vec<u8>) -> Result<()> {
    let id = FlingId(
        id.as_slice()
            .try_into()
            .context("fling id must be 16 bytes")?,
    );
    let (at, s) = session()?;
    let r = runtime().block_on(cancellable(at, async {
        s.lock()
            .await
            .send(&Message::EffectStarted {
                id,
                receiver_micros: vutric_net::micros_since(receiver_clock()),
            })
            .await
    }));
    forget_on_error(&s, r)
}

// ---- Files: any number, type and size, both ways ----

/// A transfer in progress (about every 150 ms), and its end (`done`, with what failed and where files went).
#[derive(Debug, Clone)]
pub struct TransferProgress {
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_done: u32,
    pub files_total: u32,
    pub failed: u32,
    /// The file most recently started.
    pub current: String,
    pub bytes_per_sec: f64,
    pub done: bool,
    /// When done: paths that did not arrive whole.
    pub failed_names: Vec<String>,
    /// When done, receiving: where each file was saved.
    pub saved: Vec<String>,
    pub seconds: f64,
}

fn progress_of(p: &transfer::Progress) -> TransferProgress {
    TransferProgress {
        bytes_done: p.bytes_done,
        bytes_total: p.bytes_total,
        files_done: p.files_done,
        files_total: p.files_total,
        failed: p.failed,
        current: p.current.clone(),
        bytes_per_sec: p.bytes_per_sec,
        done: false,
        failed_names: Vec::new(),
        saved: Vec::new(),
        seconds: 0.0,
    }
}

type LastProgress = Arc<Mutex<transfer::Progress>>;

fn reporter(sink: &StreamSink<TransferProgress>) -> (transfer::OnProgress, LastProgress) {
    let last: LastProgress = Arc::new(Mutex::new(transfer::Progress::default()));
    let (sink, keep) = (sink.clone(), Arc::clone(&last));
    let on: transfer::OnProgress = Arc::new(move |p: transfer::Progress| {
        let _ = sink.add(progress_of(&p));
        *keep.lock().expect("progress lock") = p;
    });
    (on, last)
}

fn finished(last: &Mutex<transfer::Progress>, sum: transfer::Summary) -> TransferProgress {
    let mut p = progress_of(&last.lock().expect("progress lock"));
    p.done = true;
    p.failed_names = sum.failed;
    p.saved = sum
        .saved
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    p.seconds = sum.elapsed.as_secs_f64();
    p
}

/// Sends files on the open session. `names[i]` is what the receiver sees for `paths[i]` (pickers hand out
/// temporary copies with made-up names); an empty or missing name keeps the file's own. Progress goes to
/// `sink`; its last event has `done`. A "no" from the other side is the error "declined" and keeps the
/// session. Cancelled by [`disconnect`].
pub fn send_files(
    paths: Vec<String>,
    names: Vec<String>,
    sink: StreamSink<TransferProgress>,
) -> Result<()> {
    let files = paths
        .iter()
        .enumerate()
        .map(|(i, p)| transfer::Outgoing::file(p, names.get(i).cloned()))
        .collect::<Result<Vec<_>>>()?;
    let (at, s) = session()?;
    let (on, last) = reporter(&sink);
    let r = runtime().block_on(cancellable(at, async {
        transfer::send(&mut *s.lock().await, files, on).await
    }));
    let sum = forget_on_error(&s, r)?;
    sink.add(finished(&last, sum))
        .map_err(|e| anyhow::anyhow!("progress stream: {e:?}"))
}

/// Accepts the offer [`next_incoming`] returned and saves it under `dir` (folders kept). Each file appears
/// only once its hash matched; an interrupted transfer resumes where it stopped when sent again. Progress goes
/// to `sink`; its last event has `done` and `saved`. Cancelled by [`disconnect`].
pub fn receive_files(dir: String, date: String, sink: StreamSink<TransferProgress>) -> Result<()> {
    let settings = load_settings();
    let (at, s, offer) = take_offer()?;
    let (on, last) = reporter(&sink);
    let r = runtime().block_on(cancellable(at, async {
        let mut session = s.lock().await;
        let to = transfer::SaveTo::for_peer(&session, &settings, dir.into(), date);
        transfer::receive(&mut session, offer, &to, on).await
    }));
    let sum = forget_on_error(&s, r)?;
    sink.add(finished(&last, sum))
        .map_err(|e| anyhow::anyhow!("progress stream: {e:?}"))
}

/// Says no to the offer [`next_incoming`] returned.
pub fn decline_files() -> Result<()> {
    let (at, s, offer) = take_offer()?;
    let r = runtime().block_on(cancellable(at, async {
        transfer::decline(&mut *s.lock().await, &offer).await
    }));
    forget_on_error(&s, r)
}

/// The pending offer, if it came on the session that is still open.
fn take_offer() -> Result<(u64, Shared, transfer::IncomingOffer)> {
    let (at, cur) = session()?;
    let (s, offer) = pending_slot()
        .lock()
        .expect("pending lock")
        .take()
        .context("no files on offer")?;
    anyhow::ensure!(
        Arc::ptr_eq(&s, &cur),
        "that offer came on a session that is closed"
    );
    Ok((at, s, offer))
}
