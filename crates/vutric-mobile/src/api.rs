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
use vutric_core::code::ShortCode;
use vutric_core::companion::{self, NoticeLimit};
use vutric_core::hotspot::{HotspotTicket, Security, Wifi};
use vutric_core::message::{
    FailReason, MAX_CLIPBOARD_BYTES, MAX_NOTICE_LINE_CHARS, MAX_NOTICE_TEXT_CHARS,
};
use vutric_core::route::{Offer, Picker, Situation, Step};
use vutric_core::sas::{LINK_SECRET_BYTES, LinkSecret};
use vutric_core::storage::DeviceRules;
use vutric_core::{EndpointId, FlingId, Message, PairingTicket, Platform, pairing};
use vutric_net::beacon::BeaconService;
use vutric_net::trust::{Admission, PairMethod, TrustStore, Visibility};
use vutric_net::{
    PAIR_INTENT_WAIT, PairIntent, Peer, Reach, Route, Session, find, lan, preview, probe, transfer,
};

use crate::frb_generated::StreamSink;

#[cfg(target_os = "ios")]
const PLATFORM: Platform = Platform::Ios;
#[cfg(not(target_os = "ios"))]
const PLATFORM: Platform = Platform::Android;

/// The error text of a call that [`disconnect`] (or a newer session) cancelled; the app ignores it.
pub const CANCELLED: &str = "vutric: cancelled";

/// How long "find a computer" looks over mDNS.
const DISCOVER_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) fn runtime() -> &'static Runtime {
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

/// The LAN beacon (ADR-0052): answers address and short-code lookups (ADR-0059) and runs ours.
fn beacon_slot() -> &'static Mutex<Option<Arc<BeaconService>>> {
    static BEACON: Mutex<Option<Arc<BeaconService>>> = Mutex::new(None);
    &BEACON
}

fn beacon() -> Result<Arc<BeaconService>> {
    beacon_slot()
        .lock()
        .expect("beacon lock")
        .clone()
        .context("find.no_beacon")
}

/// Starts (or restarts, after a rename or a new route choice) this device's peer; returns its pairing link for
/// the QR code. The route choice in the settings (ADR-0060) decides the reach: "Avtomatik" and "Internet orqali"
/// can use the internet (direct through NAT, else the encrypted relay); every other choice keeps everything on
/// the local network. Also starts the LAN beacon, so this phone can be found by its address or short code.
/// Cancels whatever was running and closes the open session. Returns at once: the link carries this network's
/// addresses, and [`wait_online`] then [`current_link`] give the one that also carries the internet relay.
pub fn start(name: String, identity: Vec<u8>) -> Result<String> {
    static STARTING: Mutex<()> = Mutex::new(());
    let key: [u8; 32] = identity
        .as_slice()
        .try_into()
        .context("identity must be 32 bytes")?;
    // One restart at a time, so two quick renames cannot leave a second peer advertising unseen.
    let _one = STARTING.lock().expect("start lock");
    drop_session();
    let old = peer_slot().lock().expect("peer lock").take();
    let old_beacon = beacon_slot().lock().expect("beacon lock").take();
    drop(old_beacon);
    let reach = Reach::for_picker(load_settings().route);
    runtime().block_on(async {
        if let Some(old) = old {
            old.close().await;
        }
        let p = Peer::bind(key, &name, PLATFORM, reach).await?;
        let t = p.ticket();
        let quic = t.addrs.first().map_or(0, std::net::SocketAddr::port);
        // A beacon that cannot bind (the port is taken, no network yet) only costs the typed-code lookup.
        match BeaconService::start(p.id(), quic, true) {
            Ok(b) => {
                b.set_answering(visibility_now() != Visibility::Off);
                *beacon_slot().lock().expect("beacon lock") = Some(Arc::new(b));
            }
            Err(e) => vutric_net::say(format!("beacon off: {e:#}")),
        }
        *peer_slot().lock().expect("peer lock") = Some(p);
        Ok(shown_link(t.to_uri()))
    })
}

/// This phone's short connect code (`ABCD-EFGH`, ADR-0059), shown under its QR for typing on another device.
pub fn my_code() -> Result<String> {
    Ok(ShortCode::of(&peer()?.id()).to_string())
}

// ---- Who may connect (ADR-0050): the trust store and the visibility window ----

fn visibility_slot() -> &'static Mutex<Visibility> {
    static VIS: Mutex<Visibility> = Mutex::new(Visibility::TrustedOnly);
    &VIS
}

fn visibility_now() -> Visibility {
    visibility_slot()
        .lock()
        .expect("visibility lock")
        .at(vutric_net::trust::now())
}

/// What the visibility switch shows: `mode` is `off`, `trusted_only`, `everyone` or `pairing`, and
/// `remaining_secs` the countdown of a window (0 otherwise).
#[derive(Debug, Clone)]
pub struct VisibilityState {
    pub mode: String,
    pub remaining_secs: u32,
}

fn visibility_state_of(v: Visibility, now: u64) -> VisibilityState {
    let v = v.at(now);
    let mode = match v {
        Visibility::Off => "off",
        Visibility::TrustedOnly => "trusted_only",
        Visibility::Everyone(_) => "everyone",
        Visibility::Pairing(_) => "pairing",
    };
    VisibilityState {
        mode: mode.into(),
        remaining_secs: v
            .remaining(now)
            .map_or(0, |s| s.min(u64::from(u32::MAX)) as u32),
    }
}

/// Sets who may connect: `off` (pause), `trusted_only` (the default), `everyone` (10 minutes, every offer asked
/// about) or `pairing` (10 minutes, while this phone's QR or code is on screen). Opening the pairing window makes
/// a new one-time link secret (ADR-0064): the QR shown from then on carries it, and an older QR stops pairing.
pub fn set_visibility(mode: String) -> Result<VisibilityState> {
    let now = vutric_net::trust::now();
    let v = match mode.as_str() {
        "off" => Visibility::Off,
        "trusted_only" => Visibility::TrustedOnly,
        "everyone" => Visibility::everyone(now),
        "pairing" => Visibility::pairing(now),
        other => bail!("unknown visibility {other:?}"),
    };
    *secret_slot().lock().expect("secret lock") =
        matches!(v, Visibility::Pairing(_)).then(new_link_secret);
    *visibility_slot().lock().expect("visibility lock") = v;
    if let Ok(b) = beacon() {
        b.set_answering(v != Visibility::Off);
    }
    Ok(visibility_state_of(v, now))
}

/// The one-time secret of the open pairing window (ADR-0064), `None` outside it.
fn secret_slot() -> &'static Mutex<Option<LinkSecret>> {
    static SECRET: Mutex<Option<LinkSecret>> = Mutex::new(None);
    &SECRET
}

fn new_link_secret() -> LinkSecret {
    let mut s = [0u8; LINK_SECRET_BYTES];
    s.copy_from_slice(&vutric_net::new_identity()[..LINK_SECRET_BYTES]);
    LinkSecret(s)
}

/// The secret a dialer must prove right now: only while the pairing window is still open.
fn window_secret() -> Option<LinkSecret> {
    matches!(visibility_now(), Visibility::Pairing(_))
        .then(|| *secret_slot().lock().expect("secret lock"))
        .flatten()
}

/// `uri` as the QR shows it: with the window's secret while the pairing window is open.
fn shown_link(uri: String) -> String {
    match window_secret() {
        Some(s) => pairing::with_secret(&uri, &s),
        None => uri,
    }
}

/// The visibility in force now (an expired window reads as `trusted_only`).
#[must_use]
pub fn visibility() -> VisibilityState {
    visibility_state_of(
        *visibility_slot().lock().expect("visibility lock"),
        vutric_net::trust::now(),
    )
}

fn trust_path() -> Option<std::path::PathBuf> {
    settings_dir()
        .as_deref()
        .map(|d| d.join(vutric_net::trust::FILE))
}

fn load_trust() -> TrustStore {
    trust_path()
        .as_deref()
        .map(vutric_net::trust::load)
        .unwrap_or_default()
}

fn edit_trust(change: impl FnOnce(&mut TrustStore)) -> Result<()> {
    let path = trust_path().context("settings folder not set")?;
    let mut t = vutric_net::trust::load(&path);
    change(&mut t);
    vutric_net::trust::save(&path, &t)
}

/// A paired device, for the device list and the device detail screen (T3).
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// The endpoint id, 64 hex: the key of its overrides.
    pub id: String,
    pub name: String,
    /// `android`, `ios`, `windows`, `linux`, `macos` or "".
    pub platform: String,
    /// `ticket` (QR or link), `code` (six digits compared) or `pinned` (a foreign app).
    pub method: String,
    pub paired_at: u64,
    pub last_seen: u64,
    /// Its short code (`ABCD-EFGH`), to tell two devices with one name apart.
    pub code: String,
    /// Whether any override is set for it.
    pub has_rules: bool,
}

fn platform_key(p: Option<Platform>) -> String {
    p.map(|p| format!("{p:?}").to_ascii_lowercase())
        .unwrap_or_default()
}

fn method_key(m: PairMethod) -> String {
    match m {
        PairMethod::Ticket => "ticket",
        PairMethod::Code => "code",
        PairMethod::Pinned => "pinned",
    }
    .into()
}

/// The paired devices, the one seen most recently first.
#[must_use]
pub fn devices() -> Vec<DeviceInfo> {
    let settings = load_settings();
    let mut out: Vec<DeviceInfo> = load_trust()
        .iter()
        .map(|(id, d)| DeviceInfo {
            id: id.to_string(),
            name: d.name.clone(),
            platform: platform_key(d.platform),
            method: method_key(d.method),
            paired_at: d.paired_at,
            last_seen: d.last_seen,
            code: ShortCode::of(&id).to_string(),
            has_rules: settings.devices.contains_key(&id.to_string()),
        })
        .collect();
    out.sort_by_key(|d| std::cmp::Reverse(d.last_seen));
    out
}

/// Forgets a device and its overrides; it has to pair again.
pub fn forget_device(id: String) -> Result<bool> {
    let key: EndpointId = id.parse().context("not a device id")?;
    let mut was = false;
    edit_trust(|t| was = t.forget(&key))?;
    let mut s = load_settings();
    if s.devices.remove(&key.to_string()).is_some() {
        save_settings(&s)?;
    }
    Ok(was)
}

/// One device's overrides as JSON (`{"folder":null,"split_by_type":null,"subfolder":null,"accept":null,
/// "gallery":null}`; `null` follows the general setting).
pub fn device_rules(id: String) -> Result<String> {
    let key: EndpointId = id.parse().context("not a device id")?;
    let s = load_settings();
    let rules = s.devices.get(&key.to_string()).cloned().unwrap_or_default();
    Ok(serde_json::to_string(&rules)?)
}

/// Saves one device's overrides (all `null` removes the entry). Invalid JSON is refused whole.
pub fn put_device_rules(id: String, json: String) -> Result<()> {
    let rules: DeviceRules = serde_json::from_str(&json).context("overrides are not valid")?;
    let mut s = load_settings();
    anyhow::ensure!(
        s.set_device_rules(&id, rules),
        "not a device id, or too many devices"
    );
    save_settings(&s)
}

/// Waits up to `max_ms` for the internet relay (about 3 s on a good line). True: devices on other networks
/// can reach this one too. False at once in LAN-only mode.
pub fn wait_online(max_ms: u32) -> Result<bool> {
    let p = peer()?;
    Ok(runtime().block_on(p.wait_online(Duration::from_millis(max_ms.into()))))
}

/// This device's pairing link as of now; once online it also carries the relay. While the pairing window is open
/// it also carries the window's one-time secret (ADR-0064), so a phone that scans it is trusted without digits.
pub fn current_link() -> Result<String> {
    Ok(shown_link(peer()?.ticket().to_uri()))
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
    let at = now_epoch();
    runtime().block_on(cancellable(at, async {
        let t1 = Instant::now();
        let s = p.connect_link(link.trim()).await?;
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
        && e.downcast_ref::<transfer::TooBigForRelay>().is_none()
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
    /// The peer's endpoint id (64 hex): the key of its overrides on the device screen.
    pub peer_id: String,
    /// Its short code (`ABCD-EFGH`).
    pub peer_code: String,
    /// In this phone's trust list (paired before, by the QR just scanned, or by a dialer that proved our QR).
    pub trusted: bool,
    /// The six digits must be compared now ([`pair_start`], then [`pair_confirm`]) before anything else uses the
    /// session: we found the device nearby, by address or by code, or it dialed us that way (ADR-0064 `Code`).
    /// False with `trusted` false: a guest (let in through "everyone" or the pairing window without proving our
    /// QR); it is never stored as trusted and every offer from it is asked about.
    pub needs_check: bool,
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

/// The open session's route badge (ADR-0060) as its i18n key (`route.badge.direct_lan`, `route.badge.relay`,
/// `route.badge.hotspot`, …); "" when not connected.
#[must_use]
pub fn current_badge() -> String {
    let Ok((_, s)) = session() else {
        return String::new();
    };
    runtime().block_on(async { s.lock().await.badge().key().to_owned() })
}

/// How a session was opened, which decides what it takes to trust the peer (ADR-0050, ADR-0064).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pairing {
    /// We dialed the key from its QR or link (pinned out of band), or the peer dialed us and proved our QR's
    /// one-time secret: trusted at once as `ticket`.
    Ticket,
    /// Found by mDNS, a typed address or a short code, or the dialer started the six-digit check on us: nothing
    /// pins the key, so the six digits must be compared unless it is paired already.
    Check,
    /// Let in through "everyone" or the pairing window without proving anything: never trusted here, a guest.
    Guest,
}

/// Records the peer in the trust store as `how` allows; returns whether it is trusted now.
fn admit_peer(s: &Session, how: Pairing) -> bool {
    let id = s.remote_id();
    let now = vutric_net::trust::now();
    let platform = Some(s.remote_platform);
    let mut trusted = false;
    let r = edit_trust(|t| {
        match how {
            Pairing::Ticket => t.trust(id, &s.remote_name, platform, PairMethod::Ticket, now),
            Pairing::Check | Pairing::Guest => t.seen(&id, &s.remote_name, now),
        }
        trusted = t.is_trusted(&id);
    });
    if let Err(e) = r {
        vutric_net::say(format!("trust store not written: {e:#}"));
        // Without a store, only a pinned key counts.
        return how == Pairing::Ticket;
    }
    trusted
}

/// What the listener does with an unknown dialer's [`PairIntent`] (ADR-0064); `None`: drop the session.
fn pairing_of(intent: PairIntent) -> Option<Pairing> {
    match intent {
        PairIntent::Link => Some(Pairing::Ticket),
        PairIntent::Code => Some(Pairing::Check),
        PairIntent::WrongLink => None,
        PairIntent::Neither => Some(Pairing::Guest),
    }
}

/// Makes `s` the open session, unless the call that opened it was cancelled meanwhile (then `s` is closed).
fn keep(at: u64, s: Session, connect: Duration, how: Pairing) -> Result<Connected> {
    let trusted = admit_peer(&s, how);
    let c = Connected {
        peer_name: s.remote_name.clone(),
        peer_platform: format!("{:?}", s.remote_platform),
        check_code: s.check_code()?,
        connect_ms: ms(connect),
        route: route_name(&s),
        peer_id: s.remote_id().to_string(),
        peer_code: ShortCode::of(&s.remote_id()).to_string(),
        trusted,
        needs_check: !trusted && how == Pairing::Check,
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

fn open(how: Pairing, job: impl Future<Output = Result<Session>>) -> Result<Connected> {
    let at = now_epoch();
    runtime().block_on(async {
        let t0 = Instant::now();
        let s = cancellable(at, job).await?;
        keep(at, s, t0.elapsed(), how)
    })
}

/// Opens the session with a pairing link (scanned or pasted). When the link carries the other device's one-time
/// secret (its pairing window, ADR-0064), the proof goes out first, so that device trusts this phone without the
/// six digits. With the "Internet orqali" route the link's LAN hints are skipped (ADR-0060). Cancelled by
/// [`disconnect`].
pub fn connect_link(link: String) -> Result<Connected> {
    let p = peer()?;
    let link = link.trim().to_owned();
    let t = PairingTicket::parse(&link)?;
    let secret = pairing::secret_in(&link)?;
    let lan = load_settings().route.uses_lan();
    open(Pairing::Ticket, async move {
        if lan {
            return p.connect_link(&link).await;
        }
        let mut s = p.connect(t.endpoint, &[]).await?;
        if let Some(secret) = secret {
            s.prove_link(&secret).await?;
        }
        Ok(s)
    })
}

/// Opens the session with the first device found over mDNS (not on iPhone: no multicast there). An unpaired
/// device comes back with `needs_check`. Cancelled by [`disconnect`].
pub fn connect_discovered() -> Result<Connected> {
    let p = peer()?;
    open(Pairing::Check, async move {
        let near = p.discover(DISCOVER_TIMEOUT).await?;
        p.connect(near.id, &near.addrs).await
    })
}

/// How long a typed address or code is looked for.
const FIND_WAIT: Duration = Duration::from_secs(6);

/// What a typed text is: `link`, `address`, `code` or "" (nothing we can use), for the field's hint as the
/// user types.
#[must_use]
pub fn typed_kind(text: String) -> String {
    let t = text.trim();
    if t.starts_with("vutric://pair?") {
        return "link".into();
    }
    match find::Target::parse(t) {
        Some(find::Target::Address(_)) => "address".into(),
        Some(find::Target::Code(_)) => "code".into(),
        None => String::new(),
    }
}

/// Connects with whatever the user typed (issue #9, ADR-0059): a pairing link, an IPv4 address (with or
/// without a port) or a short code. An address or code is looked up through the LAN beacon (`scan` also sweeps
/// this phone's /24 for a code); the device found is not authenticated, so unless it is already paired it comes
/// back with `trusted: false` and the six digits must be compared. Errors carry an i18n key: `find.silent`,
/// `find.denied` or `find.unreadable`. Cancelled by [`disconnect`].
pub fn connect_typed(text: String, scan: bool) -> Result<Connected> {
    let t = text.trim();
    if t.starts_with("vutric://pair?") {
        return connect_link(t.to_owned());
    }
    let target = find::Target::parse(t).context("find.unreadable")?;
    let p = peer()?;
    let b = beacon()?;
    open(Pairing::Check, async move {
        let found = find::resolve(&b, target, vutric_core::beacon::PORT, FIND_WAIT, scan)
            .await
            .map_err(|nf| anyhow::anyhow!(nf.key()))?;
        p.connect(found.id, &[found.addr]).await
    })
}

/// Waits for another device to connect to this phone and keeps that session. Who gets in follows the trust
/// list and the visibility (ADR-0050): a paired device always (unless paused); an unknown one only while the
/// pairing window (this phone's QR on screen) or "everyone" is open. An unknown device then shows how it pairs
/// (ADR-0064, [`Session::pair_intent`], up to [`PAIR_INTENT_WAIT`]):
///
/// - it proved our QR's one-time secret: paired as `ticket` at once;
/// - it started the six-digit check (it typed our short code or address): `needs_check`, and the screen runs
///   [`pair_start`] / [`pair_confirm`];
/// - it proved the secret of an older QR: dropped, and this phone keeps waiting;
/// - neither: a guest, never stored as trusted, every offer from it asked about.
///
/// Cancelled by [`disconnect`].
pub fn accept_session() -> Result<Connected> {
    let p = peer()?;
    let at = now_epoch();
    runtime().block_on(async {
        let t0 = Instant::now();
        loop {
            let (mut s, admission) = cancellable(at, async {
                let store = load_trust();
                p.accept_with(move |id: &EndpointId| {
                    vutric_net::trust::admit(&store, visibility_now(), id, vutric_net::trust::now())
                })
                .await
            })
            .await?;
            let how = if admission == Admission::Trusted {
                Pairing::Ticket
            } else {
                let secret = window_secret();
                let intent =
                    cancellable(at, s.pair_intent(secret.as_ref(), PAIR_INTENT_WAIT)).await?;
                if let Some(how) = pairing_of(intent) {
                    how
                } else {
                    vutric_net::say("pairing: a proof for another QR, dropped".into());
                    s.close();
                    continue;
                }
            };
            return keep(at, s, t0.elapsed(), how);
        }
    })
}

/// Pairing without a QR (ADR-0050): the commit-then-reveal exchange on the open session, run by both devices at
/// once. Returns the six digits both screens show. Call it right after a connect that returned
/// `trusted: false`, before anything else uses the session. Cancelled by [`disconnect`].
pub fn pair_start() -> Result<String> {
    let (at, s) = session()?;
    let r = runtime().block_on(cancellable(at, async { s.lock().await.pair_code().await }));
    forget_on_error(&s, r)
}

/// Sends this user's answer to the six digits and waits for the other user's (up to two minutes). True only when
/// both said yes: the device is then paired (`code`). Otherwise the session is closed.
pub fn pair_confirm(matches: bool) -> Result<bool> {
    let (at, s) = session()?;
    let r = runtime().block_on(cancellable(at, async {
        let mut guard = s.lock().await;
        let both = guard.pair_finish(matches).await?;
        if both {
            let (id, name, platform) = (
                guard.remote_id(),
                guard.remote_name.clone(),
                guard.remote_platform,
            );
            edit_trust(|t| {
                t.trust(
                    id,
                    &name,
                    Some(platform),
                    PairMethod::Code,
                    vutric_net::trust::now(),
                );
            })?;
        }
        Ok(both)
    }));
    let both = forget_on_error(&s, r)?;
    if !both {
        drop_session();
    }
    Ok(both)
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

fn settings_dir_slot() -> &'static Mutex<Option<std::path::PathBuf>> {
    static DIR: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);
    &DIR
}

fn settings_dir() -> Option<std::path::PathBuf> {
    settings_dir_slot().lock().expect("settings lock").clone()
}

fn settings_path() -> Option<std::path::PathBuf> {
    settings_dir().map(|d| d.join(vutric_net::settings::FILE))
}

/// Where this phone keeps its settings and trust files (the app's support folder). Call once at start.
pub fn set_settings_dir(dir: String) {
    *settings_dir_slot().lock().expect("settings lock") = Some(dir.into());
}

pub(crate) fn load_settings() -> vutric_net::settings::Settings {
    settings_path()
        .as_deref()
        .map(vutric_net::settings::load)
        .unwrap_or_default()
}

fn save_settings(s: &vutric_net::settings::Settings) -> Result<()> {
    let path = settings_path().context("settings folder not set")?;
    vutric_net::settings::save(&path, &s.clone().migrate())
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
    save_settings(&s)
}

/// What arrived on the open session: a photo thrown at this phone, files offered to it, or (from a paired device
/// only, ADR-0065) a mirrored notification or clipboard text. At most one field is set; none when paused.
#[derive(Debug, Clone)]
pub struct Incoming {
    pub photo: Option<Caught>,
    pub files: Option<FilesOffer>,
    pub notice: Option<CompanionNotice>,
    /// Text the other device sent for this phone's clipboard, cleaned for display ([`companion::shown`]). The
    /// screen offers "Nusxalash"; nothing is copied without the user's tap.
    pub clipboard: Option<String>,
}

impl Incoming {
    fn none() -> Self {
        Self {
            photo: None,
            files: None,
            notice: None,
            clipboard: None,
        }
    }
}

/// A notification mirrored from the other device, every field cleaned for display.
#[derive(Debug, Clone)]
pub struct CompanionNotice {
    pub app: String,
    pub title: String,
    pub text: String,
    /// The other device's clock, Unix milliseconds (display only).
    pub at_ms: u64,
}

/// The rate limit on notices shown from the open session's peer (burst 10, then 1 per 500 ms, ADR-0065).
fn notice_limit() -> &'static Mutex<(Option<EndpointId>, NoticeLimit)> {
    static LIMIT: OnceLock<Mutex<(Option<EndpointId>, NoticeLimit)>> = OnceLock::new();
    LIMIT.get_or_init(|| Mutex::new((None, NoticeLimit::default())))
}

/// What to hand the screen for a companion message from `from` (trusted or not), or `None` to drop it: only a
/// paired device's messages are taken, text is cleaned and bounded, notices are rate-limited (ADR-0065).
fn companion_incoming(msg: &Message, from: EndpointId, trusted: bool) -> Option<Incoming> {
    let admission = if trusted {
        Admission::Trusted
    } else {
        Admission::Unknown
    };
    if !companion::accepts(msg, admission) {
        return None;
    }
    match msg {
        Message::Notice(n) => {
            let mut limit = notice_limit().lock().expect("notice limit");
            if limit.0 != Some(from) {
                *limit = (Some(from), NoticeLimit::default());
            }
            if !limit.1.allow(Instant::now()) {
                return None;
            }
            let notice = CompanionNotice {
                app: companion::shown(&n.app, MAX_NOTICE_LINE_CHARS, false),
                title: companion::shown(&n.title, MAX_NOTICE_LINE_CHARS, false),
                text: companion::shown(&n.text, MAX_NOTICE_TEXT_CHARS, true),
                at_ms: n.at_ms,
            };
            Some(Incoming {
                notice: Some(notice),
                ..Incoming::none()
            })
        }
        Message::Clipboard { text } => {
            let clean = companion::shown(text, MAX_CLIPBOARD_BYTES, true);
            (!clean.is_empty()).then(|| Incoming {
                clipboard: Some(clean),
                ..Incoming::none()
            })
        }
        _ => None,
    }
}

/// Sends text to the open session's device for its clipboard prompt (ADR-0065); only on the user's tap. Only to a
/// paired device. Errors carry an i18n key: `companion.clipboard.empty`, `companion.clipboard.too_long`,
/// `companion.untrusted`.
pub fn send_clipboard(text: String) -> Result<()> {
    if text.trim().is_empty() {
        bail!("companion.clipboard.empty");
    }
    if text.len() > MAX_CLIPBOARD_BYTES {
        bail!("companion.clipboard.too_long");
    }
    let (at, s) = session()?;
    let r = runtime().block_on(cancellable(at, async {
        let mut guard = s.lock().await;
        anyhow::ensure!(
            load_trust().is_trusted(&guard.remote_id()),
            "companion.untrusted"
        );
        guard.send_clipboard(&text).await
    }));
    forget_on_error(&s, r)
}

/// Mirrors one notification of this phone to the open session's device (ADR-0065). Sent only to a paired
/// device: returns `false` (and sends nothing) for a guest or with no session. Long text is cut to the caps.
pub fn send_notice(app: String, title: String, text: String, at_ms: u64) -> Result<bool> {
    let Ok((at, s)) = session() else {
        return Ok(false);
    };
    let r = runtime().block_on(cancellable(at, async {
        let mut guard = s.lock().await;
        if !load_trust().is_trusted(&guard.remote_id()) {
            return Ok(false);
        }
        let app = if app.trim().is_empty() {
            "Android"
        } else {
            &app
        };
        guard.send_notice(app, &title, &text, at_ms).await?;
        Ok(true)
    }));
    forget_on_error(&s, r)
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

/// Waits for the next photo, file offer or companion message on the open session (either side may have opened
/// it). Latency probes are answered here and never shown; a fling whose preview is not a sane JPEG is refused
/// (`FlingFailed`), so its thrower learns at once; a notice or clipboard text from a device that is not paired is
/// dropped unread (ADR-0065). Returns an empty [`Incoming`] when [`pause_incoming`] asks for the session.
/// Cancelled by [`disconnect`].
pub fn next_incoming() -> Result<Incoming> {
    let paused = Incoming::none;
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
                // A device let in through "everyone" is not in the store: its offers are always asked about.
                let trusted = load_trust().is_trusted(&guard.remote_id());
                let files = FilesOffer {
                    files: offer.total_files,
                    bytes: offer.total_bytes,
                    names: offer.names(5),
                    auto_accept: accept.auto(trusted, offer.total_bytes),
                };
                *pending_slot().lock().expect("pending lock") = Some((Arc::clone(&s), offer));
                return Ok(Incoming {
                    files: Some(files),
                    ..Incoming::none()
                });
            }
            if matches!(msg, Message::Notice(_) | Message::Clipboard { .. }) {
                let from = guard.remote_id();
                if let Some(c) = companion_incoming(&msg, from, load_trust().is_trusted(&from)) {
                    return Ok(c);
                }
                continue;
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
                ..Incoming::none()
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
    let sum = forget_on_error(&s, r).map_err(|e| {
        if e.downcast_ref::<transfer::TooBigForRelay>().is_some() {
            anyhow::anyhow!("bulk.too_big")
        } else {
            e
        }
    })?;
    sink.add(finished(&last, sum))
        .map_err(|e| anyhow::anyhow!("progress stream: {e:?}"))
}

/// Before sending `total` bytes on the open session (backlog Rb, ADR-0068): `go`, `ask` (the path goes through
/// the relay and it is over 2 GiB: ask, offering a hotspot or cable) or `too_big` (relayed and over 10 GiB:
/// [`send_files`] refuses with `bulk.too_big` unless the path turns direct within 10 s). Call it with the session
/// to itself (the app's `exclusive`), like a send.
pub fn bulk_verdict(total: u64) -> Result<String> {
    let (at, s) = session()?;
    let r = runtime().block_on(cancellable(at, async {
        Ok(match s.lock().await.bulk(total) {
            vutric_core::route::Bulk::Go => "go",
            vutric_core::route::Bulk::Ask => "ask",
            vutric_core::route::Bulk::TooBig => "too_big",
        }
        .to_owned())
    }));
    forget_on_error(&s, r)
}

// ---- Why a connection failed (R1, R1a: ADR-0052, ADR-0068) ----

/// Why the device of a link could not be reached, for the screen after a failed connect.
#[derive(Debug, Clone)]
pub struct Diagnosis {
    /// i18n key `lan.unreachable.<key>`: `local_network_denied`, `isolated`, `other_network`, `no_network`,
    /// `captive`.
    pub key: String,
    /// What to offer next, best first: `fix_permission`, `relay`, `hotspot`, `cable`, `sign_in`.
    pub next: Vec<String>,
    /// The Wi-Fi's login page for `captive`, when the network said where it is (http(s) only, bounded); "".
    pub login: String,
}

fn next_name(n: lan::NextRoute) -> &'static str {
    match n {
        lan::NextRoute::FixPermission => "fix_permission",
        lan::NextRoute::Relay => "relay",
        lan::NextRoute::Hotspot => "hotspot",
        lan::NextRoute::Cable => "cable",
        lan::NextRoute::SignIn => "sign_in",
    }
}

/// How long the diagnosis waits for a beacon answer, and for the captive check.
const DIAGNOSE_WAIT: Duration = Duration::from_millis(1500);

/// After a failed connect to `link`: asks the other device's beacon at each of the link's addresses, and, when
/// nothing answers and this phone may use the internet, checks for a Wi-Fi login page against our own relay's
/// `/generate_204` (never a third-party server, ADR-0068). Then names the reason ([`lan::diagnose`]).
pub fn diagnose_link(link: String) -> Result<Diagnosis> {
    let t = PairingTicket::parse(link.trim())?;
    let p = peer()?;
    let b = beacon().ok();
    let ours = p.ticket().relay;
    let may_check = p.reach() == Reach::Anywhere;
    runtime().block_on(async move {
        let probe_answered = match &b {
            Some(b) => {
                let mut heard = b.subscribe();
                let ips: Vec<_> = t.addrs.iter().map(std::net::SocketAddr::ip).collect();
                b.probe(&ips).await;
                tokio::time::timeout(DIAGNOSE_WAIT, async {
                    while let Ok(h) = heard.recv().await {
                        if h.id == t.endpoint {
                            return true;
                        }
                    }
                    false
                })
                .await
                .unwrap_or(false)
            }
            None => false,
        };
        let mut login = String::new();
        let mut captive = false;
        if !probe_answered
            && may_check
            && let Some(host) = ours
                .as_deref()
                .or(t.relay.as_deref())
                .and_then(vutric_net::captive::host_for_relay)
            && let vutric_core::captive::Portal::Captive { login: l } =
                vutric_net::captive::check(&host, DIAGNOSE_WAIT).await
        {
            captive = true;
            login = l.unwrap_or_default();
        }
        let verdict = lan::diagnose(&lan::Evidence {
            locals: lan::local_addrs(),
            hints: t.addrs.clone(),
            local_network_denied: b.as_ref().is_some_and(|b| b.health().denied_sends > 0),
            probe_answered,
            internet: ours.is_some(),
            captive,
        });
        Ok(Diagnosis {
            key: format!("lan.unreachable.{}", verdict.key()),
            next: verdict
                .next_routes()
                .iter()
                .map(|n| next_name(*n).to_owned())
                .collect(),
            login,
        })
    })
}

// ---- Other apps on the phone: LocalSend, KDE Connect, magic-wormhole (C10, ADR-0066, ADR-0067) ----

/// Starts driving the adapters over `fd`, the app process's end of the socket pair whose other end Kotlin gave
/// to the `:compat` service (ADR-0066). `alias` is the name other apps see; `data_dir` keeps the adapters' own
/// identity files (never Vutric's), `cache_dir` their staging folder. Events go to `sink`, one JSON object per
/// event in the adapter's own IPC shape (`{"t":"seen",…}`, `offer`, `pair_request`, `received`, `sent`,
/// `send_failed`, `text`, `wormhole_code`, `wormhole_failed`, …), plus `{"t":"closed"}` when the loop ends; a
/// `received` path is already checked (a plain file directly inside the staging folder, or the event becomes
/// `{"t":"error"}`). A loop already running is stopped first. Android only; elsewhere `compat.unavailable`.
pub fn compat_start(
    fd: i32,
    alias: String,
    data_dir: String,
    cache_dir: String,
    sink: StreamSink<String>,
) -> Result<()> {
    #[cfg(not(target_os = "ios"))]
    return crate::compat_host::compat_start(fd, alias, data_dir, cache_dir, sink);
    #[cfg(target_os = "ios")]
    {
        let _ = (fd, alias, data_dir, cache_dir, sink);
        bail!("compat.unavailable")
    }
}

/// Whether the adapters run now.
#[must_use]
pub fn compat_running() -> bool {
    #[cfg(not(target_os = "ios"))]
    return crate::compat_host::compat_running();
    #[cfg(target_os = "ios")]
    false
}

/// Makes `LocalSend` devices on this Wi-Fi answer (they announce only now and then).
pub fn compat_announce() -> Result<()> {
    #[cfg(not(target_os = "ios"))]
    return crate::compat_host::compat_announce();
    #[cfg(target_os = "ios")]
    bail!("compat.off")
}

/// Answers an `offer` or a `pair_request`: the file ids to take (all of them for "Qabul qilish"), or none to
/// refuse; for a pair request any non-empty list pairs. Foreign offers are always asked about (ADR-0010 rule 2).
pub fn compat_decide(session_id: String, accept: Vec<String>) -> Result<()> {
    #[cfg(not(target_os = "ios"))]
    return crate::compat_host::compat_decide(session_id, accept);
    #[cfg(target_os = "ios")]
    {
        let _ = (session_id, accept);
        bail!("compat.off")
    }
}

/// Sends files to a `LocalSend` device at `to` (`ip:port` from its `seen` event). The answer is a `sent` or
/// `send_failed` event with `request_id`.
pub fn compat_send(request_id: u64, to: String, https: bool, paths: Vec<String>) -> Result<()> {
    #[cfg(not(target_os = "ios"))]
    return crate::compat_host::compat_send(request_id, to, https, paths);
    #[cfg(target_os = "ios")]
    {
        let _ = (request_id, to, https, paths);
        bail!("compat.off")
    }
}

/// Sends files to a KDE Connect device (its `seen` event's address and `device_id`). It must have paired with
/// this phone first (from the KDE Connect app; this phone answers the `pair_request`).
pub fn compat_kde_send(
    request_id: u64,
    to: String,
    device_id: String,
    paths: Vec<String>,
) -> Result<()> {
    #[cfg(not(target_os = "ios"))]
    return crate::compat_host::compat_kde_send(request_id, to, device_id, paths);
    #[cfg(target_os = "ios")]
    {
        let _ = (request_id, to, device_id, paths);
        bail!("compat.off")
    }
}

/// Sends one file or a text by magic-wormhole (C7). The code to read out arrives as `wormhole_code`, the end as
/// `sent` or `send_failed`. Refused with `wormhole.lan_only` when the route keeps everything on this network.
pub fn compat_wormhole_send(
    request_id: u64,
    paths: Vec<String>,
    text: Option<String>,
) -> Result<()> {
    #[cfg(not(target_os = "ios"))]
    return crate::compat_host::compat_wormhole_send(request_id, paths, text);
    #[cfg(target_os = "ios")]
    {
        let _ = (request_id, paths, text);
        bail!("compat.off")
    }
}

/// Receives by a magic-wormhole code the user typed (C7): a file arrives as an `offer` (session
/// `wh-<request_id>`, asked about like any other), a text as `text`, a failure as `wormhole_failed`.
pub fn compat_wormhole_receive(request_id: u64, code: String) -> Result<()> {
    #[cfg(not(target_os = "ios"))]
    return crate::compat_host::compat_wormhole_receive(request_id, code);
    #[cfg(target_os = "ios")]
    {
        let _ = (request_id, code);
        bail!("compat.off")
    }
}

/// Stops the adapters; the `:compat` process ends its loop and the service stops itself.
pub fn compat_stop() {
    #[cfg(not(target_os = "ios"))]
    crate::compat_host::compat_stop();
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

// ---- Routes (ADR-0060) and the privacy line (ADR-0061) ----

fn picker_of(choice: &str) -> Result<Picker> {
    serde_json::from_value(serde_json::Value::String(choice.to_owned()))
        .with_context(|| format!("unknown route {choice:?}"))
}

fn picker_name(p: Picker) -> String {
    p.key().trim_start_matches("route.picker.").to_owned()
}

/// Every route choice, in the order the picker lists them (`auto`, `lan_only`, `internet`, `offline`,
/// `bluetooth`); the i18n key of each is `route.picker.<choice>`.
#[must_use]
pub fn route_choices() -> Vec<String> {
    Picker::ALL.into_iter().map(picker_name).collect()
}

/// Whether the choice works on any device yet (`bluetooth` waits for R4).
pub fn route_built(choice: String) -> Result<bool> {
    Ok(picker_of(&choice)?.built())
}

/// The privacy lines for the choice, as i18n keys: `privacy.encrypted` always, `privacy.nothing_leaves` only
/// when nothing can leave the local network (ADR-0061).
pub fn privacy_lines(choice: String) -> Result<Vec<String>> {
    Ok(picker_of(&choice)?
        .privacy_lines()
        .iter()
        .map(|k| (*k).to_owned())
        .collect())
}

/// The routes to try for a choice, best first.
#[derive(Debug, Clone)]
pub struct RoutePlan {
    /// `same_network`, `internet_direct`, `relay`, `offer_hotspot`, `offer_wifi_aware`, `offer_cable`.
    pub steps: Vec<String>,
    /// When `steps` is empty: why, as an i18n key (`route.none.not_built`, `route.none.unavailable`).
    pub none: String,
}

fn step_name(s: Step) -> &'static str {
    match s {
        Step::SameNetwork => "same_network",
        Step::InternetDirect => "internet_direct",
        Step::Relay => "relay",
        Step::Offer(Offer::Hotspot) => "offer_hotspot",
        Step::Offer(Offer::WifiAware) => "offer_wifi_aware",
        Step::Offer(Offer::Cable) => "offer_cable",
    }
}

/// The "Avtomatik" order and the forced choices (ADR-0060) for what this phone can do now: `lan` (it has a
/// local network address, see [`has_lan`]), `internet` (the relay answered) and `hotspot` (it can open one).
pub fn route_plan(choice: String, lan: bool, internet: bool, hotspot: bool) -> Result<RoutePlan> {
    let s = Situation {
        lan,
        internet,
        hotspot,
        wifi_aware: false,
        cable: false,
    };
    Ok(match picker_of(&choice)?.plan(s) {
        Ok(steps) => RoutePlan {
            steps: steps.into_iter().map(|s| step_name(s).to_owned()).collect(),
            none: String::new(),
        },
        Err(e) => RoutePlan {
            steps: Vec::new(),
            none: e.key().to_owned(),
        },
    })
}

/// Whether this phone has a local network address another device could reach (Wi-Fi, Ethernet, a hotspot);
/// VPN and virtual adapters do not count.
#[must_use]
pub fn has_lan() -> bool {
    lan::local_addrs()
        .iter()
        .any(|a| a.medium != lan::Medium::Virtual)
}

// ---- Hotspot in the pairing link (ADR-0058) ----

/// A Wi-Fi network carried by a pairing link.
#[derive(Debug, Clone)]
pub struct HotspotNet {
    pub ssid: String,
    /// "" for an open network.
    pub password: String,
    /// `wpa2`, `wpa3` or `open`.
    pub security: String,
    pub hidden: bool,
}

fn wifi_of(ssid: &str, password: &str, security: &str, hidden: bool) -> Result<Wifi> {
    let sec = match security {
        "wpa2" => Security::Wpa2,
        "wpa3" => Security::Wpa3,
        "open" => Security::Open,
        other => bail!("unknown hotspot security {other:?}"),
    };
    let pw = (sec != Security::Open).then_some(password);
    Ok(Wifi::new(ssid, pw, sec, hidden)?)
}

fn net_of(w: &Wifi) -> HotspotNet {
    HotspotNet {
        ssid: w.ssid().to_owned(),
        password: w.password().unwrap_or_default().to_owned(),
        security: match w.security() {
            Security::Wpa2 => "wpa2",
            Security::Wpa3 => "wpa3",
            Security::Open => "open",
        }
        .into(),
        hidden: w.hidden(),
    }
}

/// This phone's pairing link with its hotspot in it: one QR both joins the network and pairs (R3). Refuses a
/// network another phone could not join.
pub fn hotspot_link(ssid: String, password: String, security: String) -> Result<String> {
    let wifi = wifi_of(&ssid, &password, &security, false)?;
    Ok(shown_link(
        HotspotTicket {
            pair: peer()?.ticket(),
            wifi,
        }
        .to_uri(),
    ))
}

/// The standard `WIFI:` payload for a phone's camera app (no Vutric needed to join).
pub fn wifi_qr(ssid: String, password: String, security: String, hidden: bool) -> Result<String> {
    Ok(wifi_of(&ssid, &password, &security, hidden)?.to_wifi_qr())
}

/// The hotspot a scanned link carries, if any. A half or broken hotspot part is an error, so the user is not
/// sent to a network that cannot work.
pub fn hotspot_of_link(link: String) -> Result<Option<HotspotNet>> {
    Ok(Wifi::in_link(link.trim())?.as_ref().map(net_of))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_names_round_trip_and_privacy_is_honest() {
        let all = route_choices();
        assert_eq!(
            all,
            ["auto", "lan_only", "internet", "offline", "bluetooth"]
        );
        for c in &all {
            assert_eq!(picker_name(picker_of(c).unwrap()), *c);
        }
        assert!(picker_of("everywhere").is_err());
        assert_eq!(privacy_lines("auto".into()).unwrap(), ["privacy.encrypted"]);
        assert_eq!(
            privacy_lines("lan_only".into()).unwrap(),
            ["privacy.encrypted", "privacy.nothing_leaves"]
        );
        assert!(!route_built("bluetooth".into()).unwrap());
    }

    #[test]
    fn plans_follow_the_engine() {
        let p = route_plan("auto".into(), true, true, true).unwrap();
        assert_eq!(
            p.steps,
            ["same_network", "internet_direct", "relay", "offer_hotspot"]
        );
        let off = route_plan("offline".into(), false, false, true).unwrap();
        assert_eq!(off.steps, ["offer_hotspot"]);
        let none = route_plan("lan_only".into(), false, true, true).unwrap();
        assert!(none.steps.is_empty());
        assert_eq!(none.none, "route.none.unavailable");
        assert_eq!(
            route_plan("bluetooth".into(), true, true, true)
                .unwrap()
                .none,
            "route.none.not_built"
        );
    }

    #[test]
    fn hotspot_wifi_checks_and_round_trips() {
        assert!(wifi_of("Vutric_1", "short", "wpa2", false).is_err());
        assert!(wifi_of("Vutric_1", "abcd1234", "wep", false).is_err());
        let open = wifi_of("Kafe", "ignored", "open", false).unwrap();
        assert_eq!(open.password(), None);
        let w = wifi_of("AndroidShare_7", "a;b,c:d\"e", "wpa2", false).unwrap();
        assert_eq!(
            w.to_wifi_qr(),
            "WIFI:T:WPA;S:AndroidShare_7;P:a\\;b\\,c\\:d\\\"e;;"
        );
        let t = PairingTicket {
            endpoint: EndpointId([7; 32]),
            device_name: "Pixel".into(),
            addrs: vec!["192.168.43.1:41000".parse().unwrap()],
            relay: None,
        };
        let link = HotspotTicket { pair: t, wifi: w }.to_uri();
        let net = hotspot_of_link(link).unwrap().unwrap();
        assert_eq!(
            (
                net.ssid.as_str(),
                net.password.as_str(),
                net.security.as_str()
            ),
            ("AndroidShare_7", "a;b,c:d\"e", "wpa2")
        );
        let plain = PairingTicket {
            endpoint: EndpointId([7; 32]),
            device_name: "Pixel".into(),
            addrs: vec![],
            relay: None,
        };
        assert!(hotspot_of_link(plain.to_uri()).unwrap().is_none());
    }

    #[test]
    fn typed_text_is_recognised() {
        assert_eq!(typed_kind("vutric://pair?v=2&id=1".into()), "link");
        assert_eq!(typed_kind(" 192.168.1.24 ".into()), "address");
        assert_eq!(typed_kind("192.168.1.24:47110".into()), "address");
        assert_eq!(typed_kind("k7qd-2m9x".into()), "code");
        assert_eq!(typed_kind("fe80::1".into()), "");
        assert_eq!(typed_kind("salom".into()), "");
    }

    #[test]
    fn visibility_reads_with_its_countdown() {
        let now = 1_795_000_000;
        let s = visibility_state_of(Visibility::everyone(now), now + 60);
        assert_eq!((s.mode.as_str(), s.remaining_secs), ("everyone", 540));
        let gone = visibility_state_of(Visibility::pairing(now), now + 601);
        assert_eq!(
            (gone.mode.as_str(), gone.remaining_secs),
            ("trusted_only", 0)
        );
        assert_eq!(visibility_state_of(Visibility::Off, now).mode, "off");
    }
}
