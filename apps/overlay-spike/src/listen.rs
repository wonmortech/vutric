//! `--listen`: the end-to-end Phase 0 check. A phone flings a photo preview over the LAN; the overlay plays Halqa
//! with it, and `EffectStarted` goes back only after that effect's first frame was presented. The phone's round
//! trip is therefore an upper bound of "finger released → first frame on the desktop".
//!
//! Previews that are not a decodable image (the 0.2 latency probes) are answered at once, without the overlay,
//! so the phone app's measurement buttons keep working against this listener.

use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow};
use qrcode::QrCode;
use qrcode::render::unicode::Dense1x2;
use tokio::sync::oneshot;
use vutric_core::{Gesture, Message, Platform};
use vutric_net::{Peer, Session};
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
pub fn spawn(name: String, proxy: EventLoopProxy<Incoming>) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("vutric-net")
        .build()?;
    std::thread::Builder::new()
        .name("vutric-net".into())
        .spawn(move || {
            if let Err(e) = rt.block_on(serve(&name, proxy)) {
                eprintln!("network stopped: {e:#}");
            }
        })?;
    Ok(())
}

async fn serve(name: &str, proxy: EventLoopProxy<Incoming>) -> Result<()> {
    let platform = if cfg!(windows) {
        Platform::Windows
    } else {
        Platform::Linux
    };
    let peer = Peer::bind(vutric_net::identity_file("desktop.key")?, name, platform).await?;
    let link = peer.ticket().to_uri();
    let qr = QrCode::new(link.as_bytes())?;
    println!("{}", qr.render::<Dense1x2>().quiet_zone(true).build());
    println!("pairing link: {link}");
    println!("waiting for a phone; fling a photo from it (Ctrl+C to stop)");
    loop {
        let s = match peer.accept().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("connection refused: {e:#}");
                continue;
            }
        };
        println!(
            "connected: {} ({:?}), pairing check {}",
            s.remote_name,
            s.remote_platform,
            s.check_code()?
        );
        let proxy = proxy.clone();
        tokio::spawn(async move {
            if let Err(e) = session(s, proxy).await {
                println!("session ended: {e:#}");
            }
        });
    }
}

async fn session(mut s: Session, proxy: EventLoopProxy<Incoming>) -> Result<()> {
    let clock = Instant::now();
    loop {
        let msg = s.recv().await?;
        let arrived = Instant::now();
        let Message::FlingStart(f) = msg else {
            continue;
        };
        let decoded = f
            .preview
            .as_ref()
            .and_then(|p| image::load_from_memory(&p.bytes).ok());
        let Some(img) = decoded else {
            // A latency probe, not a photo: answer at once.
            s.send(&Message::EffectStarted {
                id: f.id,
                receiver_micros: vutric_net::micros_since(clock),
            })
            .await?;
            continue;
        };
        let decode = arrived.elapsed();
        let (tx, rx) = oneshot::channel();
        proxy
            .send_event(Incoming {
                image: img.thumbnail(1024, 1024).to_rgba8(),
                gesture: f.gesture,
                arrived,
                first_frame: tx,
            })
            .map_err(|_| anyhow!("overlay closed"))?;
        let shown = tokio::time::timeout(Duration::from_secs(2), rx)
            .await
            .context("overlay did not draw within 2 s")?
            .context("overlay dropped the fling")?;
        s.send(&Message::EffectStarted {
            id: f.id,
            receiver_micros: vutric_net::micros_since(clock),
        })
        .await?;
        println!(
            "{} from {}: arrived -> first frame {:.1} ms (decode {:.1} ms), {} KB preview",
            f.file_name,
            s.remote_name,
            (shown - arrived).as_secs_f64() * 1000.0,
            decode.as_secs_f64() * 1000.0,
            f.preview.map_or(0, |p| p.bytes.len()) / 1024
        );
    }
}
