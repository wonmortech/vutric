//! Spike 0.2: two Vutric peers on one LAN, no internet, measure a 30 KB preview in both directions.
//!
//! ```text
//! vutric-net-spike listen [--name NAME] [--runs N]
//!     Shows the pairing QR and link, waits for a peer, answers N probes, then sends N back.
//! vutric-net-spike send (--ticket LINK | --discover) [--name NAME] [--runs N]
//!     Finds the peer (pairing link or mDNS), sends N probes, then answers N.
//! vutric-net-spike fling (--ticket LINK | --discover) --photo PATH [--count N]
//!     Flings a real photo to a listening overlay (`vutric-overlay-spike --listen`) and prints the round trip
//!     until its first frame was on that screen. The target may be a phone on its catch screen, too.
//! vutric-net-spike fling --photo PATH [--count N]
//!     Shows the pairing QR, waits for a phone to scan it (catch screen), then flings the photo to it.
//! ```
//! Every mode takes `--lan-only` (never use the internet). `send`/`fling` with `--ticket` also take
//! `--via-internet`: ignore the link's LAN addresses and reach the peer by its key, relay and lookup server
//! only, as a device on another network would.
//!
//! Both sides print the same six-digit pairing check and their own statistics; the first 3 probes per direction
//! are warm-up (QUIC path validation) and are reported separately.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use qrcode::QrCode;
use qrcode::render::unicode::Dense1x2;
use vutric_core::{Gesture, PairingTicket, Platform};
use vutric_net::{Peer, Reach, Route, Session};
use vutric_net::{preview, probe};

struct Args {
    mode: String,
    name: String,
    runs: u32,
    ticket: Option<String>,
    discover: bool,
    photo: Option<PathBuf>,
    count: u32,
    lan_only: bool,
    via_internet: bool,
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
        lan_only: false,
        via_internet: false,
    };
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--name" => a.name = it.next().context("--name needs a value")?,
            "--runs" => a.runs = it.next().context("--runs needs a value")?.parse()?,
            "--ticket" => a.ticket = Some(it.next().context("--ticket needs a value")?),
            "--discover" => a.discover = true,
            "--photo" => a.photo = Some(it.next().context("--photo needs a path")?.into()),
            "--count" => a.count = it.next().context("--count needs a value")?.parse()?,
            "--lan-only" => a.lan_only = true,
            "--via-internet" => a.via_internet = true,
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

/// "direct, 0.4 ms" or "relay, 38.0 ms": how the session's packets travel now.
fn via(s: &Session) -> String {
    match s.route() {
        (Route::Direct, Some(rtt)) => format!("direct, {:.1} ms", ms(rtt)),
        (Route::Relay, Some(rtt)) => format!("relay, {:.1} ms", ms(rtt)),
        _ => "path pending".into(),
    }
}

async fn listen(peer: &Peer, a: &Args) -> Result<()> {
    if peer.reach() == Reach::Anywhere {
        let online = peer.wait_online(Duration::from_secs(8)).await;
        println!(
            "{}",
            if online {
                "reachable on this network and over the internet (encrypted relay)"
            } else {
                "no internet right now: reachable on this network only"
            }
        );
    }
    let link = peer.ticket().to_uri();
    let qr = QrCode::new(link.as_bytes())?;
    println!("{}", qr.render::<Dense1x2>().quiet_zone(true).build());
    println!("pairing link: {link}");
    println!("waiting for a peer (Ctrl+C to stop)...");
    let mut s = peer.accept().await?;
    println!(
        "connected: {} ({:?}), {}, pairing check {}",
        s.remote_name,
        s.remote_platform,
        via(&s),
        s.check_code()?
    );
    if let Some(path) = &a.photo {
        // The phone scanned our QR code and waits on its catch screen: throw at it over this session.
        fling_photo(&mut s, path, a.count).await?;
        s.finish().await;
        return Ok(());
    }
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
    let t1;
    let mut s = if let Some(link) = &a.ticket {
        let mut t = PairingTicket::parse(link)?;
        println!(
            "pairing link for {} with {} address hint(s), relay {}",
            t.device_name,
            t.addrs.len(),
            t.relay.as_deref().unwrap_or("none")
        );
        if a.via_internet {
            t.addrs.clear();
        }
        t1 = Instant::now();
        peer.connect_ticket(&t).await?
    } else if a.discover {
        let n = peer.discover(Duration::from_secs(15)).await?;
        println!(
            "mDNS found {} in {:.0} ms at {:?}",
            n.name.as_deref().unwrap_or("(no name)"),
            ms(t0.elapsed()),
            n.addrs
        );
        t1 = Instant::now();
        peer.connect(n.id, &n.addrs).await?
    } else {
        bail!("send needs --ticket LINK or --discover");
    };
    println!(
        "connected to {} ({:?}) in {:.0} ms, {}, pairing check {}",
        s.remote_name,
        s.remote_platform,
        ms(t1.elapsed()),
        via(&s),
        s.check_code()?
    );
    if let Some(path) = &a.photo {
        fling_photo(&mut s, path, a.count).await?;
        s.finish().await;
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
    // Our last act was an answer: close only once the peer has it.
    s.finish().await;
    Ok(())
}

/// Flings a photo like the phone does (the same preview, see `vutric_net::preview`).
async fn fling_photo(s: &mut Session, path: &Path, count: u32) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let preview::Encoded {
        jpeg,
        width: w,
        height: h,
    } = preview::encode(&bytes)?;
    let name = path
        .file_name()
        .map_or_else(|| "photo.jpg".into(), |n| n.to_string_lossy().into_owned());
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
        if a.lan_only {
            Reach::Lan
        } else {
            Reach::Anywhere
        },
    )
    .await?;
    println!("{} ({:?}) id {}", a.name, platform(), peer.id());
    let result = match a.mode.as_str() {
        "listen" => listen(&peer, &a).await,
        // Without a target, `fling` shows its QR code and throws at whoever connects (a phone that cannot be
        // found over mDNS, such as an iPhone).
        "fling" if a.ticket.is_none() && !a.discover => listen(&peer, &a).await,
        "send" | "fling" => send(&peer, &a).await,
        _ => Err(anyhow::anyhow!(
            "usage: vutric-net-spike listen|send|fling [--ticket LINK | --discover] [--name NAME] [--runs N] [--photo PATH [--count N]]"
        )),
    };
    peer.close().await;
    result
}
