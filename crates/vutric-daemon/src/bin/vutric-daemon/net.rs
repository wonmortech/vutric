//! The network side: one iroh peer, one task per connected device.
//!
//! Who may connect is W3's trust gate (ADR-0050, ADR-0064): paired devices always, unknown ones only while the
//! pairing card is on screen. An unknown dialer shows how it pairs (`Session::pair_intent`): it proved the QR's
//! one-time secret (trusted as `ticket`), started the six-digit check (the card asks the user), proved an old QR
//! (dropped) or neither (a guest whose every offer is asked about).
//!
//! Flings go to the overlay; offers are answered by the user's rules or by the "Qabul qilasizmi?" card and saved
//! by W3's storage engine (`transfer::SaveTo::for_peer`: rules, free-space guard, conflicts). Companion messages
//! (ADR-0065) and rings (ADR-0069) are taken from trusted devices only.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use tokio::sync::{broadcast, mpsc, oneshot};
use vutric_core::companion::{self, NoticeLimit};
use vutric_core::message::{
    FailReason, MAX_CLIPBOARD_BYTES, MAX_NOTICE_LINE_CHARS, MAX_NOTICE_TEXT_CHARS,
};
use vutric_core::ring::{self, RingLimit};
use vutric_core::sas::{LINK_SECRET_BYTES, LinkSecret};
use vutric_core::trust::{PairMethod, Visibility};
use vutric_core::{EndpointId, FlingId, Gesture, Message, Platform};
use vutric_daemon::cards::Question;
use vutric_daemon::ipc::{Device, Event};
use vutric_net::trust::Admission;
use vutric_net::{PAIR_CONFIRM_TIMEOUT, PAIR_INTENT_WAIT, PairIntent, Peer, Reach, Session};
use vutric_net::{preview, probe, transfer};
use winit::event_loop::EventLoopProxy;

use crate::UiEvent;

/// How long the overlay may take to show a fling. Covers a GPU that needs seconds to wake (D9): the first fling
/// after a cold start waits instead of being refused.
pub const FIRST_FRAME_WAIT: Duration = Duration::from_secs(8);
/// How long an unanswered "accept?" card waits before the offer is declined.
pub const OFFER_WAIT: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub struct Incoming {
    pub image: image::RgbaImage,
    pub gesture: Gesture,
    pub arrived: Instant,
    pub first_frame: oneshot::Sender<Instant>,
}

/// Work the UI or the IPC hands to one live session.
#[derive(Debug, Clone)]
pub enum Cmd {
    /// "Telefonni topish": ring the phone for this many seconds.
    Ring(u16),
    /// Stop our ring, or tell the phone that rang us that it was found.
    StopRing,
    /// Text for the phone's clipboard prompt (only on the user's action).
    Clipboard(String),
}

/// One connected device, as the UI and the IPC see it.
#[derive(Debug, Clone)]
pub struct Live {
    pub name: String,
    pub platform: Platform,
    pub trusted: bool,
    pub tx: mpsc::UnboundedSender<Cmd>,
}

/// State shared between the network thread, the UI thread and the IPC server.
pub struct Shared {
    pub paused: AtomicBool,
    pub online: AtomicBool,
    pub transfers: AtomicU32,
    pub events: broadcast::Sender<Event>,
    pub id: std::sync::OnceLock<String>,
    /// The plain pairing link (no secret); [`Shared::open_pairing`] gives what a QR shows.
    pub link: std::sync::OnceLock<String>,
    pub name: String,
    /// Who may connect now (ADR-0050). `Pairing` while the QR card is up.
    pub visibility: Mutex<Visibility>,
    /// The one-time secret of the open pairing window (ADR-0064).
    pub secret: Mutex<Option<LinkSecret>>,
    /// Connected devices by endpoint id (64 hex).
    pub live: Mutex<HashMap<String, Live>>,
}

impl Shared {
    #[must_use]
    pub fn new(name: String, events: broadcast::Sender<Event>) -> Self {
        Self {
            paused: false.into(),
            online: false.into(),
            transfers: 0.into(),
            events,
            id: std::sync::OnceLock::new(),
            link: std::sync::OnceLock::new(),
            name,
            visibility: Mutex::new(Visibility::TrustedOnly),
            secret: Mutex::new(None),
            live: Mutex::new(HashMap::new()),
        }
    }

    fn visibility_now(&self) -> Visibility {
        if self.paused.load(Ordering::Relaxed) {
            return Visibility::Off;
        }
        self.visibility
            .lock()
            .map_or(Visibility::TrustedOnly, |v| v.at(vutric_net::trust::now()))
    }

    /// `off`, `trusted_only`, `everyone` or `pairing`, for the IPC status.
    #[must_use]
    pub fn visibility_name(&self) -> &'static str {
        match self.visibility_now() {
            Visibility::Off => "off",
            Visibility::TrustedOnly => "trusted_only",
            Visibility::Everyone(_) => "everyone",
            Visibility::Pairing(_) => "pairing",
        }
    }

    /// "Everyone nearby may offer files" for 10 minutes (S3); every offer is still asked about.
    pub fn open_everyone(&self) {
        if let Ok(mut v) = self.visibility.lock() {
            *v = Visibility::everyone(vutric_net::trust::now());
        }
    }

    /// Opens the pairing window (10 minutes, ADR-0050) with a fresh one-time secret and returns the link the QR
    /// must show. `None` while the network is still starting.
    #[must_use]
    pub fn open_pairing(&self) -> Option<String> {
        let link = self.link.get()?.clone();
        let mut s = [0u8; LINK_SECRET_BYTES];
        s.copy_from_slice(&vutric_net::new_identity()[..LINK_SECRET_BYTES]);
        let secret = LinkSecret(s);
        if let Ok(mut v) = self.visibility.lock() {
            *v = Visibility::pairing(vutric_net::trust::now());
        }
        if let Ok(mut slot) = self.secret.lock() {
            *slot = Some(secret);
        }
        Some(vutric_core::pairing::with_secret(&link, &secret))
    }

    /// Closes the pairing window: only paired devices may connect again, and the shown QR stops pairing.
    pub fn close_pairing(&self) {
        if let Ok(mut v) = self.visibility.lock()
            && matches!(*v, Visibility::Pairing(_))
        {
            *v = Visibility::TrustedOnly;
        }
        if let Ok(mut slot) = self.secret.lock() {
            *slot = None;
        }
    }

    fn window_secret(&self) -> Option<LinkSecret> {
        if !matches!(self.visibility_now(), Visibility::Pairing(_)) {
            return None;
        }
        self.secret.lock().ok().and_then(|s| *s)
    }

    /// Sends `cmd` to every connected trusted device; returns how many got it.
    #[must_use]
    pub fn to_trusted(&self, cmd: &Cmd) -> usize {
        self.live.lock().map_or(0, |live| {
            live.values()
                .filter(|l| l.trusted && l.tx.send(cmd.clone()).is_ok())
                .count()
        })
    }

    /// Sends `cmd` to one connected device by id; `false` when it is not connected.
    #[must_use]
    pub fn to_device(&self, id: &str, cmd: Cmd) -> bool {
        self.live
            .lock()
            .ok()
            .and_then(|live| live.get(id).map(|l| l.tx.send(cmd).is_ok()))
            .unwrap_or(false)
    }

    /// The paired devices with whether each is connected now, plus connected guests, for the desktop window and
    /// `vutric-daemon ctl devices`.
    #[must_use]
    pub fn devices(&self) -> Vec<Device> {
        let store = load_trust();
        let live = self.live.lock().map(|l| l.clone()).unwrap_or_default();
        let mut out: Vec<Device> = store
            .iter()
            .map(|(id, d)| {
                let key = id.to_string();
                Device {
                    connected: live.contains_key(&key),
                    id: key,
                    name: d.name.clone(),
                    platform: d.platform.map(|p| format!("{p:?}")).unwrap_or_default(),
                    trusted: true,
                    method: format!("{:?}", d.method).to_lowercase(),
                    last_seen: d.last_seen,
                }
            })
            .collect();
        for (id, l) in &live {
            if !l.trusted {
                out.push(Device {
                    id: id.clone(),
                    name: l.name.clone(),
                    platform: format!("{:?}", l.platform),
                    trusted: false,
                    connected: true,
                    method: String::new(),
                    last_seen: vutric_net::trust::now(),
                });
            }
        }
        out.sort_by(|a, b| {
            b.connected
                .cmp(&a.connected)
                .then(b.last_seen.cmp(&a.last_seen))
        });
        out
    }

    /// Unpairs a device: it can no longer connect unless it pairs again. Returns whether it was paired.
    pub fn forget(&self, id: &str) -> Result<bool> {
        let Ok(eid) = id.parse::<EndpointId>() else {
            return Ok(false);
        };
        let mut gone = false;
        edit_trust(|t| gone = t.forget(&eid))?;
        if let Ok(mut live) = self.live.lock()
            && let Some(l) = live.get_mut(id)
        {
            l.trusted = false;
        }
        Ok(gone)
    }
}

fn trust_path() -> Result<std::path::PathBuf> {
    Ok(vutric_net::settings::desktop_dir()?.join(vutric_net::trust::FILE))
}

fn load_trust() -> vutric_core::trust::TrustStore {
    trust_path()
        .map(|p| vutric_net::trust::load(&p))
        .unwrap_or_default()
}

fn edit_trust(change: impl FnOnce(&mut vutric_core::trust::TrustStore)) -> Result<()> {
    let path = trust_path()?;
    let mut t = vutric_net::trust::load(&path);
    change(&mut t);
    vutric_net::trust::save(&path, &t)
}

pub fn spawn(
    shared: Arc<Shared>,
    lan_only: bool,
    proxy: EventLoopProxy<UiEvent>,
) -> Result<tokio::runtime::Handle> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("vutric-net")
        .build()?;
    let handle = rt.handle().clone();
    std::thread::Builder::new()
        .name("vutric-net".into())
        .spawn(move || {
            if let Err(e) = rt.block_on(serve(shared, lan_only, proxy)) {
                vutric_net::say(format!("network stopped: {e:#}"));
            }
        })?;
    Ok(handle)
}

#[must_use]
pub fn platform() -> Platform {
    if cfg!(windows) {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Linux
    }
}

async fn serve(shared: Arc<Shared>, lan_only: bool, proxy: EventLoopProxy<UiEvent>) -> Result<()> {
    let reach = if lan_only {
        Reach::Lan
    } else {
        Reach::Anywhere
    };
    let peer = Peer::bind(
        vutric_net::identity_file("desktop.key")?,
        &shared.name,
        platform(),
        reach,
    )
    .await?;
    let _ = shared.id.set(peer.id().to_string());
    let _ = shared.link.set(peer.ticket().to_uri());
    {
        let (peer, shared) = (peer.clone(), Arc::clone(&shared));
        tokio::spawn(async move {
            let online = peer.wait_online(Duration::from_secs(20)).await;
            shared.online.store(online, Ordering::Relaxed);
        });
    }
    loop {
        let gate = Arc::clone(&shared);
        // The file is read per knock, so a device paired a second ago is already known.
        let (mut s, admission) = peer
            .accept_with(move |id: &EndpointId| {
                vutric_net::trust::admit(
                    &load_trust(),
                    gate.visibility_now(),
                    id,
                    vutric_net::trust::now(),
                )
            })
            .await?;
        let (proxy, shared) = (proxy.clone(), Arc::clone(&shared));
        tokio::spawn(async move {
            let how = if admission == Admission::Trusted {
                Some(How::Trusted)
            } else {
                let secret = shared.window_secret();
                match s.pair_intent(secret.as_ref(), PAIR_INTENT_WAIT).await {
                    Ok(PairIntent::Link) => Some(How::Link),
                    Ok(PairIntent::Code) => Some(How::Code),
                    Ok(PairIntent::Neither) => Some(How::Guest),
                    Ok(PairIntent::WrongLink) => {
                        vutric_net::say("pairing: a proof for an older QR, dropped".into());
                        None
                    }
                    Err(e) => {
                        vutric_net::say(format!("pairing: {e:#}"));
                        None
                    }
                }
            };
            let Some(how) = how else {
                s.close();
                return;
            };
            // A connected phone is about to fling: the overlay warms its GPU now, not when the image lands.
            let _ = proxy.send_event(UiEvent::Peer(true));
            if let Err(e) = session(s, how, proxy.clone(), Arc::clone(&shared)).await {
                vutric_net::say(format!("session ended: {e:#}"));
            }
            let _ = proxy.send_event(UiEvent::Peer(false));
        });
    }
}

/// How a session's peer got in (ADR-0064).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum How {
    Trusted,
    Link,
    Code,
    Guest,
}

/// `123456` as people compare it: `123 456`.
fn grouped(code: &str) -> String {
    if code.len() == 6 && code.is_ascii() {
        format!("{} {}", &code[..3], &code[3..])
    } else {
        code.to_owned()
    }
}

/// Records the peer as `how` allows and returns whether it is trusted now. A `Code` peer is trusted only after
/// both users confirmed the six digits.
async fn admit(
    s: &mut Session,
    how: How,
    proxy: &EventLoopProxy<UiEvent>,
    shared: &Shared,
) -> Result<bool> {
    let (id, name, platform, now) = (
        s.remote_id(),
        s.remote_name.clone(),
        s.remote_platform,
        vutric_net::trust::now(),
    );
    let method = match how {
        How::Trusted => {
            let _ = edit_trust(|t| t.seen(&id, &name, now));
            return Ok(true);
        }
        How::Guest => return Ok(false),
        How::Link => PairMethod::Ticket,
        How::Code => {
            let code = s.pair_code().await?;
            let (tx, rx) = oneshot::channel();
            proxy
                .send_event(UiEvent::Ask {
                    q: Question::Check,
                    device: name.clone(),
                    body: grouped(&code),
                    answer: tx,
                })
                .map_err(|_| anyhow!("ui closed"))?;
            let yes = matches!(
                tokio::time::timeout(PAIR_CONFIRM_TIMEOUT, rx).await,
                Ok(Ok(true))
            );
            if !s.pair_finish(yes).await? {
                return Ok(false);
            }
            PairMethod::Code
        }
    };
    edit_trust(|t| t.trust(id, &name, Some(platform), method, now))?;
    // Paired: the window closes and the QR on screen stops working.
    shared.close_pairing();
    let _ = proxy.send_event(UiEvent::Paired { device: name });
    Ok(true)
}

async fn refuse(s: &mut Session, id: FlingId, reason: FailReason) -> Result<()> {
    s.send(&Message::FlingFailed { id, reason }).await
}

async fn offer(
    s: &mut Session,
    offer: transfer::IncomingOffer,
    trusted: bool,
    proxy: &EventLoopProxy<UiEvent>,
    shared: &Shared,
) -> Result<()> {
    let settings = vutric_net::settings::load(
        &vutric_net::settings::desktop_dir()?.join(vutric_net::settings::FILE),
    );
    let (_, accept) = settings.rules_for(&s.remote_id().to_string());
    let id = hex(&offer.id.0);
    let _ = shared.events.send(Event::Offer {
        id: id.clone(),
        from: s.remote_name.clone(),
        files: offer.total_files,
        bytes: offer.total_bytes,
    });
    let yes = if accept.auto(trusted, offer.total_bytes) {
        true
    } else {
        let (tx, rx) = oneshot::channel();
        proxy
            .send_event(UiEvent::Offer {
                device: s.remote_name.clone(),
                files: offer.total_files,
                bytes: offer.total_bytes,
                answer: tx,
            })
            .map_err(|_| anyhow!("ui closed"))?;
        matches!(tokio::time::timeout(OFFER_WAIT, rx).await, Ok(Ok(true)))
    };
    if !yes {
        return transfer::decline(s, &offer).await;
    }
    let to = transfer::SaveTo::for_peer(
        s,
        &settings,
        transfer::downloads_dir()?,
        transfer::today_utc(),
    );
    let folder = to
        .rules
        .folder
        .clone()
        .unwrap_or_else(|| to.default_folder.clone());
    shared.transfers.fetch_add(1, Ordering::Relaxed);
    let progress: transfer::OnProgress = {
        let (proxy, events, id) = (proxy.clone(), shared.events.clone(), id.clone());
        Arc::new(move |p: transfer::Progress| {
            let _ = proxy.send_event(UiEvent::Progress {
                done: p.bytes_done,
                total: p.bytes_total,
            });
            let _ = events.send(Event::Progress {
                id: id.clone(),
                bytes_done: p.bytes_done,
                bytes_total: p.bytes_total,
                files_done: p.files_done,
                files_total: p.files_total,
            });
        })
    };
    let result = transfer::receive(s, offer, &to, progress).await;
    shared.transfers.fetch_sub(1, Ordering::Relaxed);
    match result {
        Ok(sum) => {
            let _ = shared.events.send(Event::Received {
                id,
                files: sum.files,
                folder: folder.display().to_string(),
            });
            let _ = proxy.send_event(UiEvent::Received {
                device: s.remote_name.clone(),
                files: sum.saved,
                folder,
            });
            Ok(())
        }
        Err(e) => {
            let reason = if e.is::<transfer::NoRoom>() {
                "no_room".to_owned()
            } else {
                format!("{e:#}")
            };
            let _ = shared.events.send(Event::Failed { id, reason });
            let _ = proxy.send_event(UiEvent::Failed);
            Err(e)
        }
    }
}

/// A companion message or ring from a trusted device, turned into UI work (ADR-0065, ADR-0069). `None`: not
/// one, not trusted, or over its rate limit.
fn companion_event(
    msg: &Message,
    device: &str,
    id: &str,
    trusted: bool,
    notices: &mut NoticeLimit,
    rings: &mut RingLimit,
) -> Option<UiEvent> {
    let admission = if trusted {
        Admission::Trusted
    } else {
        Admission::Unknown
    };
    match msg {
        Message::Notice(n) => (companion::accepts(msg, admission) && notices.allow(Instant::now()))
            .then(|| UiEvent::Notice {
                device: device.to_owned(),
                app: companion::shown(&n.app, MAX_NOTICE_LINE_CHARS, false),
                title: companion::shown(&n.title, MAX_NOTICE_LINE_CHARS, false),
                text: companion::shown(&n.text, MAX_NOTICE_TEXT_CHARS, true),
            }),
        Message::Clipboard { text } => {
            if !companion::accepts(msg, admission) {
                return None;
            }
            let clean = companion::shown(text, MAX_CLIPBOARD_BYTES, true);
            (!clean.is_empty()).then(|| UiEvent::Clipboard {
                device: device.to_owned(),
                text: clean,
            })
        }
        Message::Ring(r) => {
            if !ring::accepts(msg, admission) || !rings.allow(*r, Instant::now()) {
                return None;
            }
            Some(match r.duration() {
                Some(d) => UiEvent::Ring {
                    device: device.to_owned(),
                    id: id.to_owned(),
                    length: d,
                },
                None => UiEvent::Found {
                    device: device.to_owned(),
                },
            })
        }
        _ => None,
    }
}

async fn session(
    mut s: Session,
    how: How,
    proxy: EventLoopProxy<UiEvent>,
    shared: Arc<Shared>,
) -> Result<()> {
    let mut trusted = admit(&mut s, how, &proxy, &shared).await?;
    let key = s.remote_id().to_string();
    let (tx, mut rx) = mpsc::unbounded_channel();
    if let Ok(mut live) = shared.live.lock() {
        live.insert(
            key.clone(),
            Live {
                name: s.remote_name.clone(),
                platform: s.remote_platform,
                trusted,
                tx,
            },
        );
    }
    let r = session_loop(&mut s, &mut trusted, &key, &mut rx, &proxy, &shared).await;
    if let Ok(mut live) = shared.live.lock() {
        live.remove(&key);
    }
    r
}

#[allow(clippy::too_many_lines)] // one receive loop, read top to bottom
async fn session_loop(
    s: &mut Session,
    trusted: &mut bool,
    key: &str,
    rx: &mut mpsc::UnboundedReceiver<Cmd>,
    proxy: &EventLoopProxy<UiEvent>,
    shared: &Shared,
) -> Result<()> {
    let clock = Instant::now();
    let (mut notices, mut rings) = (NoticeLimit::default(), RingLimit::default());
    loop {
        // `Session::recv` is cancel-safe: its only await is a QUIC stream read, and a decoded message is never
        // held across an await.
        let msg = tokio::select! {
            m = s.recv() => m?,
            cmd = rx.recv() => {
                match cmd {
                    Some(Cmd::Ring(secs)) => s.ring(secs).await?,
                    Some(Cmd::StopRing) => s.stop_ring().await?,
                    Some(Cmd::Clipboard(text)) => s.send_clipboard(&text).await?,
                    None => return Ok(()),
                }
                continue;
            }
        };
        // A device forgotten in the desktop window loses its trust in this session too.
        if *trusted
            && shared
                .live
                .lock()
                .is_ok_and(|live| live.get(key).is_some_and(|l| !l.trusted))
        {
            *trusted = false;
        }
        let arrived = Instant::now();
        if let Some(ev) = companion_event(
            &msg,
            &s.remote_name,
            key,
            *trusted,
            &mut notices,
            &mut rings,
        ) {
            let _ = proxy.send_event(ev);
            continue;
        }
        if let Some(o) = transfer::IncomingOffer::from_message(&msg) {
            offer(s, o, *trusted, proxy, shared).await?;
            continue;
        }
        let Message::FlingStart(f) = msg else {
            continue;
        };
        if probe::is_probe(&f) {
            s.send(&Message::EffectStarted {
                id: f.id,
                receiver_micros: vutric_net::micros_since(clock),
            })
            .await?;
            continue;
        }
        if shared.paused.load(Ordering::Relaxed) {
            refuse(s, f.id, FailReason::Rejected).await?;
            continue;
        }
        let Some(p) = f.preview else {
            refuse(s, f.id, FailReason::Rejected).await?;
            continue;
        };
        let decoded = tokio::task::spawn_blocking(move || {
            preview::decode(&p.bytes, (p.width, p.height)).map(|img| {
                image::DynamicImage::ImageRgba8(img)
                    .thumbnail(1024, 1024)
                    .to_rgba8()
            })
        })
        .await?;
        let Ok(img) = decoded else {
            refuse(s, f.id, FailReason::Rejected).await?;
            continue;
        };
        let (tx, rx_frame) = oneshot::channel();
        proxy
            .send_event(UiEvent::Fling(Incoming {
                image: img,
                gesture: f.gesture,
                arrived,
                first_frame: tx,
            }))
            .map_err(|_| anyhow!("overlay closed"))?;
        if let Ok(Ok(_)) = tokio::time::timeout(FIRST_FRAME_WAIT, rx_frame).await {
            s.send(&Message::EffectStarted {
                id: f.id,
                receiver_micros: vutric_net::micros_since(clock),
            })
            .await?;
        } else {
            refuse(s, f.id, FailReason::Cancelled).await?;
        }
    }
}

/// `s` cut to at most `max` bytes on a character boundary.
#[must_use]
pub fn clip_bytes(s: &str, max: usize) -> String {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

fn hex(b: &[u8]) -> String {
    use std::fmt::Write as _;
    b.iter()
        .fold(String::with_capacity(b.len() * 2), |mut s, x| {
            let _ = write!(s, "{x:02x}");
            s
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vutric_core::ring::Ring;

    fn ring_msg(secs: u16) -> Message {
        Message::Ring(Ring::start(secs))
    }

    #[test]
    fn only_trusted_devices_ring_or_send_text() {
        let (mut n, mut r) = (NoticeLimit::default(), RingLimit::default());
        assert!(companion_event(&ring_msg(10), "P", "id", false, &mut n, &mut r).is_none());
        let clip = Message::Clipboard {
            text: "salom".into(),
        };
        assert!(companion_event(&clip, "P", "id", false, &mut n, &mut r).is_none());
        assert!(matches!(
            companion_event(&clip, "P", "id", true, &mut n, &mut r),
            Some(UiEvent::Clipboard { .. })
        ));
        assert!(matches!(
            companion_event(&ring_msg(10), "P", "id", true, &mut n, &mut r),
            Some(UiEvent::Ring { .. })
        ));
        assert!(matches!(
            companion_event(&Message::Ring(Ring::Stop), "P", "id", true, &mut n, &mut r),
            Some(UiEvent::Found { .. })
        ));
    }

    #[test]
    fn rings_are_rate_limited() {
        let (mut n, mut r) = (NoticeLimit::default(), RingLimit::default());
        let rang = (0..10)
            .filter(|_| companion_event(&ring_msg(5), "P", "id", true, &mut n, &mut r).is_some())
            .count();
        assert_eq!(rang, RingLimit::BURST as usize);
    }

    #[test]
    fn control_only_clipboard_text_is_dropped() {
        let (mut n, mut r) = (NoticeLimit::default(), RingLimit::default());
        let clip = Message::Clipboard {
            text: "\u{0}\u{7}".into(),
        };
        assert!(companion_event(&clip, "P", "id", true, &mut n, &mut r).is_none());
    }

    #[test]
    fn text_is_cut_on_a_character_boundary() {
        assert_eq!(clip_bytes("o\u{2bb}g", 2), "o");
        assert_eq!(clip_bytes("salom", 64), "salom");
    }

    #[test]
    fn codes_are_grouped_for_reading() {
        assert_eq!(grouped("123456"), "123 456");
        assert_eq!(grouped("12"), "12");
    }
}
