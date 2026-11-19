//! The network side: one iroh peer, one task per connected device. Flings go to the overlay; file offers are
//! answered by the user's rules or by the "Qabul qilasizmi?" card; received files raise a toast.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use tokio::sync::{broadcast, oneshot};
use vutric_core::message::FailReason;
use vutric_core::{FlingId, Gesture, Message, Platform};
use vutric_daemon::ipc::Event;
use vutric_net::{Peer, Reach, Session, preview, probe, transfer};
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

/// State shared between the network thread, the UI thread and the IPC server.
pub struct Shared {
    pub paused: AtomicBool,
    pub online: AtomicBool,
    pub transfers: AtomicU32,
    pub events: broadcast::Sender<Event>,
    pub id: std::sync::OnceLock<String>,
    pub link: std::sync::OnceLock<String>,
    pub name: String,
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

async fn serve(shared: Arc<Shared>, lan_only: bool, proxy: EventLoopProxy<UiEvent>) -> Result<()> {
    let platform = if cfg!(windows) {
        Platform::Windows
    } else {
        Platform::Linux
    };
    let reach = if lan_only {
        Reach::Lan
    } else {
        Reach::Anywhere
    };
    let peer = Peer::bind(
        vutric_net::identity_file("desktop.key")?,
        &shared.name,
        platform,
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
        let s = peer.accept().await?;
        if shared.paused.load(Ordering::Relaxed) {
            s.close();
            continue;
        }
        let (proxy, shared) = (proxy.clone(), Arc::clone(&shared));
        tokio::spawn(async move {
            // A connected phone is about to fling: the overlay warms its GPU now, not when the image lands.
            let _ = proxy.send_event(UiEvent::Peer(true));
            if let Err(e) = session(s, proxy.clone(), shared).await {
                vutric_net::say(format!("session ended: {e:#}"));
            }
            let _ = proxy.send_event(UiEvent::Peer(false));
        });
    }
}

async fn refuse(s: &mut Session, id: FlingId, reason: FailReason) -> Result<()> {
    s.send(&Message::FlingFailed { id, reason }).await
}

async fn offer(
    s: &mut Session,
    offer: transfer::IncomingOffer,
    proxy: &EventLoopProxy<UiEvent>,
    shared: &Shared,
) -> Result<()> {
    let settings = vutric_net::settings::load(
        &vutric_net::settings::desktop_dir()?.join(vutric_net::settings::FILE),
    );
    let (_, accept) = settings.rules_for(&s.remote_id().to_string());
    // TEMP until W3 S1 (trust store): no device counts as trusted yet, so every offer asks.
    let trusted = false;
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
            let _ = shared.events.send(Event::Failed {
                id,
                reason: format!("{e:#}"),
            });
            let _ = proxy.send_event(UiEvent::Failed);
            Err(e)
        }
    }
}

async fn session(
    mut s: Session,
    proxy: EventLoopProxy<UiEvent>,
    shared: Arc<Shared>,
) -> Result<()> {
    let clock = Instant::now();
    loop {
        let msg = s.recv().await?;
        let arrived = Instant::now();
        if let Some(o) = transfer::IncomingOffer::from_message(&msg) {
            offer(&mut s, o, &proxy, &shared).await?;
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
            refuse(&mut s, f.id, FailReason::Rejected).await?;
            continue;
        }
        let Some(p) = f.preview else {
            refuse(&mut s, f.id, FailReason::Rejected).await?;
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
            refuse(&mut s, f.id, FailReason::Rejected).await?;
            continue;
        };
        let (tx, rx) = oneshot::channel();
        proxy
            .send_event(UiEvent::Fling(Incoming {
                image: img,
                gesture: f.gesture,
                arrived,
                first_frame: tx,
            }))
            .map_err(|_| anyhow!("overlay closed"))?;
        if let Ok(Ok(_)) = tokio::time::timeout(FIRST_FRAME_WAIT, rx).await {
            s.send(&Message::EffectStarted {
                id: f.id,
                receiver_micros: vutric_net::micros_since(clock),
            })
            .await?;
        } else {
            refuse(&mut s, f.id, FailReason::Cancelled).await?;
        }
    }
}

fn hex(b: &[u8]) -> String {
    use std::fmt::Write as _;
    b.iter()
        .fold(String::with_capacity(b.len() * 2), |mut s, x| {
            let _ = write!(s, "{x:02x}");
            s
        })
}
