//! Spike 0.2: two Vutric peers on one LAN, no internet, measure a 30 KB preview in both directions.
//!
//! ```text
//! vutric-net-spike listen [--name NAME] [--runs N]
//!     Shows the pairing QR and link, waits for a peer, answers N probes, then sends N back.
//! vutric-net-spike send (--ticket LINK | --discover) [--name NAME] [--runs N]
//!     Finds the peer (pairing link or mDNS), sends N probes, then answers N.
//! vutric-net-spike fling (--ticket LINK | --discover) --photo PATH [--count N]
//!     Flings a real photo to a listening overlay (`vutric-overlay-spike --listen`) and prints the round trip
//!     until its first frame was on that screen.
//! ```
//! Both sides print the same six-digit pairing check and their own statistics; the first 3 probes per direction
//! are warm-up (QUIC path validation) and are reported separately.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use qrcode::QrCode;
use qrcode::render::unicode::Dense1x2;
use vutric_core::{Gesture, PairingTicket, Platform};
use vutric_net::probe;
use vutric_net::{Peer, Session};

struct Args {
    mode: String,
    name: String,
    runs: u32,
    ticket: Option<String>,
    discover: bool,
    photo: Option<PathBuf>,
    count: u32,
}

fn parse() -> Result<Args> {
    let mut it = std::env::args().skip(1);
    let mode = it.next().unwrap_or_default();
    let mut a = Args {
        mode,
        name: default_name(),
        runs: 100,
        ticket: None,
        discover: false,
        photo: None,
        count: 1,
    };
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--name" => a.name = it.next().context("--name needs a value")?,
            "--runs" => a.runs = it.next().context("--runs needs a value")?.parse()?,
            "--ticket" => a.ticket = Some(it.next().context("--ticket needs a value")?),
            "--discover" => a.discover = true,
            "--photo" => a.photo = Some(it.next().context("--photo needs a path")?.into()),
            "--count" => a.count = it.next().context("--count needs a value")?.parse()?,
            other => bail!("unknown argument {other}"),
        }
    }
    Ok(a)
}

fn default_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "Vutric desktop".into())
}

fn platform() -> Platform {
    if cfg!(windows) {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Linux
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

async fn send_run(s: &mut Session, runs: u32, label: &str) -> Result<()> {
    let (warm, stats) = probe::send_run(s, runs).await?;
    println!("{label}: warm-up {}", warm.summary());
    println!("{label}: {}", stats.summary());
    Ok(())
}

async fn listen(peer: &Peer, a: &Args) -> Result<()> {
    let link = peer.ticket().to_uri();
    let qr = QrCode::new(link.as_bytes())?;
    println!("{}", qr.render::<Dense1x2>().quiet_zone(true).build());
    println!("pairing link: {link}");
    println!("waiting for a peer (Ctrl+C to stop)...");
    let mut s = peer.accept().await?;
    println!(
        "connected: {} ({:?}), pairing check {}",
        s.remote_name,
        s.remote_platform,
        s.check_code()?
    );
    probe::answer_run(&mut s, a.runs).await?;
    println!(
        "answered {} probes from {}",
        probe::WARMUP + a.runs,
        s.remote_name
    );
    let label = format!("{} -> {}", a.name, s.remote_name);
    send_run(&mut s, a.runs, &label).await?;
    s.close();
    Ok(())
}

async fn send(peer: &Peer, a: &Args) -> Result<()> {
    let t0 = Instant::now();
    let (id, addrs) = if let Some(link) = &a.ticket {
        let t = PairingTicket::parse(link)?;
        println!(
            "pairing link for {} with {} address hint(s)",
            t.device_name,
            t.addrs.len()
        );
        (t.endpoint, t.addrs)
    } else if a.discover {
        let n = peer.discover(Duration::from_secs(15)).await?;
        println!(
            "mDNS found {} in {:.0} ms at {:?}",
            n.name.as_deref().unwrap_or("(no name)"),
            ms(t0.elapsed()),
            n.addrs
        );
        (n.id, n.addrs)
    } else {
        bail!("send needs --ticket LINK or --discover");
    };
    let t1 = Instant::now();
    let mut s = peer.connect(id, &addrs).await?;
    println!(
        "connected to {} ({:?}) in {:.0} ms, pairing check {}",
        s.remote_name,
        s.remote_platform,
        ms(t1.elapsed()),
        s.check_code()?
    );
    if let Some(path) = &a.photo {
        fling_photo(&mut s, path, a.count).await?;
        tokio::time::sleep(Duration::from_millis(200)).await;
        s.close();
        return Ok(());
    }
    let label = format!("{} -> {}", a.name, s.remote_name);
    send_run(&mut s, a.runs, &label).await?;
    probe::answer_run(&mut s, a.runs).await?;
    println!(
        "answered {} probes from {}",
        probe::WARMUP + a.runs,
        s.remote_name
    );
    // Give the last answer time to leave before the connection closes.
    tokio::time::sleep(Duration::from_millis(200)).await;
    s.close();
    Ok(())
}

/// Flings a photo like the phone does: a JPEG preview, longest side 512 px.
async fn fling_photo(s: &mut Session, path: &Path, count: u32) -> Result<()> {
    let img = image::open(path)
        .with_context(|| format!("open {}", path.display()))?
        .thumbnail(512, 512)
        .to_rgb8();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80).encode_image(&img)?;
    let name = path
        .file_name()
        .map_or_else(|| "photo.jpg".into(), |n| n.to_string_lossy().into_owned());
    let (w, h) = (u16::try_from(img.width())?, u16::try_from(img.height())?);
    for i in 0..count {
        let gesture = Gesture {
            speed: 2400.0,
            angle: 0.15 * (f32::from(u8::try_from(i % 3)?) - 1.0),
            exit_x: 0.5,
        };
        let rtt = probe::round_trip(s, &probe::photo(&name, jpeg.clone(), w, h, gesture)).await?;
        println!(
            "{name} ({} KB) -> {}: sent -> first frame there and back {:.1} ms",
            jpeg.len() / 1024,
            s.remote_name,
            ms(rtt)
        );
        if i + 1 < count {
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let a = parse()?;
    let peer = Peer::bind(
        vutric_net::identity_file("net-spike.key")?,
        &a.name,
        platform(),
    )
    .await?;
    println!("{} ({:?}) id {}", a.name, platform(), peer.id());
    let result = match a.mode.as_str() {
        "listen" => listen(&peer, &a).await,
        "send" | "fling" => send(&peer, &a).await,
        _ => Err(anyhow::anyhow!(
            "usage: vutric-net-spike listen|send|fling [--ticket LINK | --discover] [--name NAME] [--runs N] [--photo PATH [--count N]]"
        )),
    };
    peer.close().await;
    result
}
