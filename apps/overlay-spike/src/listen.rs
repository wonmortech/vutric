//! `--listen`: the end-to-end Phase 0 check. A phone flings a photo preview over the LAN; the overlay plays Halqa
//! with it, and `EffectStarted` goes back only after that effect's first frame was presented. The phone's round
//! trip is therefore an upper bound of "finger released → first frame on the desktop".
//!
//! The 0.2 latency probes are answered at once, without the overlay, so the phone app's measurement buttons keep
//! working against this listener. A preview that is not a sane JPEG (checked with hard size limits, off the
//! network threads) is refused with `FlingFailed`, and the session stays up.

use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use qrcode::QrCode;
use qrcode::render::unicode::Dense1x2;
use tokio::sync::oneshot;
use vutric_core::message::FailReason;
use vutric_core::{FlingId, Gesture, Message, Platform};
use vutric_net::{Peer, Reach, Route, Session, preview, probe};
use winit::event_loop::EventLoopProxy;

/// A fling handed from the network thread to the overlay thread.
pub struct Incoming {
    pub image: image::RgbaImage,
    pub gesture: Gesture,
    /// When the whole `FlingStart` frame had arrived.
    pub arrived: Instant,
    /// Fired by the overlay with the moment the first frame of this fling was presented.
    pub first_frame: oneshot::Sender<Instant>,
}

/// Runs the LAN peer on its own thread and Tokio runtime; the overlay keeps the main thread.
pub fn spawn(name: String, lan_only: bool, proxy: EventLoopProxy<Incoming>) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("vutric-net")
        .build()?;
    std::thread::Builder::new()
        .name("vutric-net".into())
        .spawn(move || {
            if let Err(e) = rt.block_on(serve(&name, lan_only, proxy)) {
                eprintln!("network stopped: {e:#}");
            }
        })?;
    Ok(())
}

async fn serve(name: &str, lan_only: bool, proxy: EventLoopProxy<Incoming>) -> Result<()> {
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
        name,
        platform,
        reach,
    )
    .await?;
    // With internet, wait briefly for a relay so the QR also works from another network.
    let online = peer.wait_online(Duration::from_secs(8)).await;
    let link = peer.ticket().to_uri();
    let qr = QrCode::new(link.as_bytes())?;
    println!("{}", qr.render::<Dense1x2>().quiet_zone(true).build());
    println!("pairing link: {link}");
    println!(
        "{}",
        match (reach, online) {
            (Reach::Lan, _) => "local network only (--lan-only)",
            (Reach::Anywhere, true) =>
                "reachable on this network and over the internet (encrypted relay)",
            (Reach::Anywhere, false) => "no internet right now: reachable on this network only",
        }
    );
    // From here on the network thread never writes to the console itself: a console frozen by a mouse
    // selection (Windows QuickEdit) would otherwise stall every connection.
    vutric_net::say("waiting for a phone; fling a photo from it (Ctrl+C to stop)".into());
    loop {
        let s = match peer.accept().await {
            Ok(s) => s,
            // accept() itself only fails once the endpoint is closed (bad clients are skipped inside it).
            Err(e) => {
                return Err(e);
            }
        };
        let check = s
            .check_code()
            .unwrap_or_else(|e| format!("unavailable ({e})"));
        let via = match s.route() {
            (Route::Direct, Some(rtt)) => format!("direct, {:.1} ms", rtt.as_secs_f64() * 1000.0),
            (Route::Relay, Some(rtt)) => format!("relay, {:.1} ms", rtt.as_secs_f64() * 1000.0),
            _ => "path pending".into(),
        };
        vutric_net::say(format!(
            "connected: {} ({:?}), {via}, pairing check {check}",
            s.remote_name, s.remote_platform
        ));
        let proxy = proxy.clone();
        tokio::spawn(async move {
            if let Err(e) = session(s, proxy).await {
                vutric_net::say(format!("session ended: {e:#}"));
            }
        });
    }
}

async fn refuse(s: &mut Session, id: FlingId, reason: FailReason) -> Result<()> {
    s.send(&Message::FlingFailed { id, reason }).await
}

async fn session(mut s: Session, proxy: EventLoopProxy<Incoming>) -> Result<()> {
    let clock = Instant::now();
    loop {
        let msg = s.recv().await?;
        let arrived = Instant::now();
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
        let Some(p) = f.preview else {
            refuse(&mut s, f.id, FailReason::Rejected).await?;
            continue;
        };
        let kb = p.bytes.len() / 1024;
        // Decoding is CPU work on untrusted bytes: bounded by preview::decode, and kept off the network threads.
        let decoded = tokio::task::spawn_blocking(move || {
            preview::decode(&p.bytes, (p.width, p.height)).map(|img| {
                image::DynamicImage::ImageRgba8(img)
                    .thumbnail(1024, 1024)
                    .to_rgba8()
            })
        })
        .await?;
        let img = match decoded {
            Ok(img) => img,
            Err(e) => {
                vutric_net::say(format!("{}: refused: {e:#}", s.remote_name));
                refuse(&mut s, f.id, FailReason::Rejected).await?;
                continue;
            }
        };
        let decode = arrived.elapsed();
        let (tx, rx) = oneshot::channel();
        proxy
            .send_event(Incoming {
                image: img,
                gesture: f.gesture,
                arrived,
                first_frame: tx,
            })
            .map_err(|_| anyhow!("overlay closed"))?;
        let shown = match tokio::time::timeout(Duration::from_secs(2), rx).await {
            Ok(Ok(shown)) => shown,
            // Another fling took the screen before this one drew, or the GPU never came up: say so, keep going.
            Ok(Err(_)) => {
                refuse(&mut s, f.id, FailReason::Cancelled).await?;
                continue;
            }
            Err(_) => {
                vutric_net::say(format!(
                    "{}: overlay did not draw within 2 s",
                    s.remote_name
                ));
                refuse(&mut s, f.id, FailReason::Cancelled).await?;
                continue;
            }
        };
        s.send(&Message::EffectStarted {
            id: f.id,
            receiver_micros: vutric_net::micros_since(clock),
        })
        .await?;
        vutric_net::say(format!(
            "{} from {}: arrived -> first frame {:.1} ms (decode {:.1} ms), {kb} KB preview",
            f.file_name
                .chars()
                .filter(|c| !c.is_control())
                .collect::<String>(),
            s.remote_name,
            (shown - arrived).as_secs_f64() * 1000.0,
            decode.as_secs_f64() * 1000.0,
        ));
    }
}
