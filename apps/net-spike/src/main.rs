//! Spike 0.2 and the desktop command line (backlog F10, ADR-0069): two Vutric peers, latency probes, files,
//! clipboard text, mirrored notices and "ring my phone", for power users and scripts.
//!
//! ```text
//! vutric-net-spike listen [--name NAME] [--runs N]
//!     Shows the pairing QR and link, waits for a peer, answers N probes, then sends N back.
//! vutric-net-spike send (--ticket LINK | --discover) [--name NAME] [--runs N]
//!     Finds the peer (pairing link or mDNS), sends N probes, then answers N.
//! vutric-net-spike send PATH... (--ticket LINK | --discover)
//!     With paths: the same as `files`.
//! vutric-net-spike fling (--ticket LINK | --discover) --photo PATH [--count N]
//!     Flings a real photo to a listening overlay (`vutric-overlay-spike --listen`) and prints the round trip
//!     until its first frame was on that screen. The target may be a phone on its catch screen, too.
//! vutric-net-spike fling --photo PATH [--count N]
//!     Shows the pairing QR, waits for a phone to scan it (catch screen), then flings the photo to it.
//! vutric-net-spike files PATH... [--ticket LINK | --discover]
//!     Sends files and folders (any type and size) to the target, or shows the QR and sends to whoever connects.
//! vutric-net-spike receive [--dir DIR] [--json] [--yes]
//!     Shows the QR (pairing window: 10 minutes) and keeps running. Saves files from paired devices (`--yes`:
//!     from unpaired ones too), prints their clipboard text and notices, and rings the terminal bell on a ring.
//! vutric-net-spike ring (--ticket LINK | --discover) [--seconds N] [--stop]
//!     Rings the other device for N seconds (default 30, at most 60) and waits until it is found or the time is
//!     up; `--stop` ends a ring. Exit code 0 when the other side said "found", 3 when the time ran out.
//! vutric-net-spike clipboard (TEXT... | -) (--ticket LINK | --discover)
//!     Sends text to the other device's copy prompt; `-` reads it from standard input.
//! vutric-net-spike notice --app APP [--title TITLE] (TEXT... | -) (--ticket LINK | --discover)
//!     Sends one notice, as a phone mirrors a notification.
//! vutric-net-spike devices [--json]
//!     Lists the devices this command line has paired with.
//! ```
//! Every mode takes `--lan-only` (never use the internet). Connecting modes with `--ticket` also take
//! `--via-internet`: ignore the link's LAN addresses and reach the peer by its key, relay and lookup server
//! only, as a device on another network would. A link with a one-time secret (`s=`) pairs both sides.
//!
//! Both sides print the same six-digit pairing check and their own statistics; the first 3 probes per direction
//! are warm-up (QUIC path validation) and are reported separately.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use qrcode::QrCode;
use qrcode::render::unicode::Dense1x2;
use serde_json::json;
use vutric_core::companion::{self, NoticeLimit};
use vutric_core::message::{MAX_CLIPBOARD_BYTES, MAX_NOTICE_LINE_CHARS, MAX_NOTICE_TEXT_CHARS};
use vutric_core::ring::{self, DEFAULT_RING_SECS, Ring, RingLimit};
use vutric_core::sas::LinkSecret;
use vutric_core::{Gesture, Message, PairingTicket, Platform, pairing};
use vutric_net::trust::{self, PairMethod, TrustStore, Visibility};
use vutric_net::{Admission, PAIR_INTENT_WAIT, PairIntent, Peer, Reach, Route, Session};
use vutric_net::{preview, probe, transfer};

/// Exit code of `ring` when nobody stopped it in time.
const NOT_FOUND_EXIT: i32 = 3;

#[allow(clippy::struct_excessive_bools)] // command-line switches
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
    words: Vec<String>,
    dir: Option<PathBuf>,
    seconds: u16,
    stop: bool,
    app: Option<String>,
    title: String,
    json: bool,
    yes: bool,
}

impl Args {
    fn paths(&self) -> Vec<PathBuf> {
        self.words.iter().map(PathBuf::from).collect()
    }

    /// The positional text, or standard input for a lone `-`.
    fn text(&self) -> Result<String> {
        if self.words == ["-"] {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)?;
            return Ok(s.trim_end_matches(['\r', '\n']).to_owned());
        }
        Ok(self.words.join(" "))
    }
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
        words: Vec::new(),
        dir: None,
        seconds: DEFAULT_RING_SECS,
        stop: false,
        app: None,
        title: String::new(),
        json: false,
        yes: false,
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
            "--dir" => a.dir = Some(it.next().context("--dir needs a path")?.into()),
            "--seconds" => a.seconds = it.next().context("--seconds needs a value")?.parse()?,
            "--stop" => a.stop = true,
            "--app" => a.app = Some(it.next().context("--app needs a value")?),
            "--title" => a.title = it.next().context("--title needs a value")?,
            "--json" => a.json = true,
            "--yes" => a.yes = true,
            other if other == "-" || !other.starts_with("--") => a.words.push(other.into()),
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

/// This command line's own trust store. It is not the daemon's: the command line has its own key.
fn trust_file() -> Result<PathBuf> {
    Ok(vutric_net::settings::desktop_dir()?.join("net-spike-trusted.json"))
}

fn remember(s: &Session, method: PairMethod) -> Result<()> {
    let path = trust_file()?;
    let mut store = trust::load(&path);
    store.trust(
        s.remote_id(),
        &s.remote_name,
        Some(s.remote_platform),
        method,
        trust::now(),
    );
    trust::save(&path, &store)
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

/// Shows this peer's pairing QR and link (after waiting for the relay, so the link works from anywhere). With a
/// secret, a device that scans it pairs (ADR-0064).
async fn show_qr(peer: &Peer, secret: Option<&LinkSecret>) -> Result<()> {
    if peer.reach() == Reach::Anywhere {
        let online = peer.wait_online(Duration::from_secs(8)).await;
        eprintln!(
            "{}",
            if online {
                "reachable on this network and over the internet (encrypted relay)"
            } else {
                "no internet right now: reachable on this network only"
            }
        );
    }
    let mut link = peer.ticket().to_uri();
    if let Some(secret) = secret {
        link = pairing::with_secret(&link, secret);
    }
    let qr = QrCode::new(link.as_bytes())?;
    eprintln!("{}", qr.render::<Dense1x2>().quiet_zone(true).build());
    eprintln!("pairing link: {link}");
    Ok(())
}

async fn listen(peer: &Peer, a: &Args) -> Result<()> {
    show_qr(peer, None).await?;
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

/// Connects to the target given by `--ticket` or `--discover`. A link with a one-time secret is proven to the
/// other side, and both then trust each other (ADR-0064); its key is stored here as `ticket`.
async fn connect(peer: &Peer, a: &Args) -> Result<Session> {
    let t0 = Instant::now();
    let t1;
    let s = if let Some(link) = &a.ticket {
        let mut t = PairingTicket::parse(link)?;
        eprintln!(
            "pairing link for {} with {} address hint(s), relay {}",
            t.device_name,
            t.addrs.len(),
            t.relay.as_deref().unwrap_or("none")
        );
        t1 = Instant::now();
        let secret = pairing::secret_in(link)?;
        if a.via_internet {
            t.addrs.clear();
        }
        let mut s = peer.connect_ticket(&t).await?;
        if let Some(secret) = secret {
            s.prove_link(&secret).await?;
            remember(&s, PairMethod::Ticket)?;
        }
        s
    } else if a.discover {
        let n = peer.discover(Duration::from_secs(15)).await?;
        eprintln!(
            "mDNS found {} in {:.0} ms at {:?}",
            n.name.as_deref().unwrap_or("(no name)"),
            ms(t0.elapsed()),
            n.addrs
        );
        t1 = Instant::now();
        peer.connect(n.id, &n.addrs).await?
    } else {
        bail!("{} needs --ticket LINK or --discover", a.mode);
    };
    eprintln!(
        "connected to {} ({:?}) in {:.0} ms, {}, pairing check {}",
        s.remote_name,
        s.remote_platform,
        ms(t1.elapsed()),
        via(&s),
        s.check_code()?
    );
    Ok(s)
}

async fn send(peer: &Peer, a: &Args) -> Result<()> {
    let mut s = connect(peer, a).await?;
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

fn failed_note(label: &str, failed: &[String]) -> String {
    if failed.is_empty() {
        String::new()
    } else {
        format!("; {label}: {}", failed.join(", "))
    }
}

fn rate(sum: &transfer::Summary) -> f64 {
    sum.bytes as f64 / 1e6 / sum.elapsed.as_secs_f64().max(1e-3)
}

/// Sends files and folders to the target, or to whoever scans our QR code.
async fn files(peer: &Peer, a: &Args) -> Result<()> {
    let list = transfer::collect(&a.paths())?;
    let total: u64 = list.iter().map(|f| f.size).sum();
    println!("{} file(s), {:.1} MB", list.len(), total as f64 / 1e6);
    let mut s = if a.ticket.is_some() || a.discover {
        connect(peer, a).await?
    } else {
        show_qr(peer, None).await?;
        println!("waiting for a phone to scan the QR code...");
        let s = peer.accept().await?;
        println!(
            "connected: {} ({}), pairing check {}",
            s.remote_name,
            via(&s),
            s.check_code()?
        );
        s
    };
    let sum = transfer::send(&mut s, list, transfer::log_progress("sending")).await?;
    println!(
        "sent {} file(s), {:.1} MB in {:.1} s ({:.1} MB/s, {}){}",
        sum.files,
        sum.bytes as f64 / 1e6,
        sum.elapsed.as_secs_f64(),
        rate(&sum),
        via(&s),
        failed_note("not sent", &sum.failed)
    );
    s.finish().await;
    Ok(())
}

/// `ring`: rings the target and waits for "found" (or `--stop`s a ring). Returns the exit code.
async fn ring_cmd(peer: &Peer, a: &Args) -> Result<i32> {
    let mut s = connect(peer, a).await?;
    if a.stop {
        s.stop_ring().await?;
        s.finish().await;
        println!("ring stopped on {}", s.remote_name);
        return Ok(0);
    }
    let Ring::Start { secs } = Ring::start(a.seconds) else {
        unreachable!("Ring::start always starts")
    };
    s.ring(secs).await?;
    println!(
        "ringing {} for {secs} s (it rings only if it trusts this device)",
        s.remote_name
    );
    let until = tokio::time::Instant::now() + Duration::from_secs(u64::from(secs) + 5);
    let code = loop {
        match tokio::time::timeout_at(until, s.recv()).await {
            Ok(Ok(Message::Ring(Ring::Stop))) => {
                println!("{} was found", s.remote_name);
                break 0;
            }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => bail!("{} closed the session: {e:#}", s.remote_name),
            Err(_) => {
                println!("nobody stopped the ring on {}", s.remote_name);
                break NOT_FOUND_EXIT;
            }
        }
    };
    s.finish().await;
    Ok(code)
}

async fn clipboard_cmd(peer: &Peer, a: &Args) -> Result<()> {
    let text = a.text()?;
    if text.is_empty() || text.len() > MAX_CLIPBOARD_BYTES {
        bail!("clipboard text must be 1 byte to {MAX_CLIPBOARD_BYTES} bytes");
    }
    let mut s = connect(peer, a).await?;
    s.send_clipboard(&text).await?;
    s.finish().await;
    println!(
        "sent {} byte(s) to {}'s copy prompt",
        text.len(),
        s.remote_name
    );
    Ok(())
}

async fn notice_cmd(peer: &Peer, a: &Args) -> Result<()> {
    let app = a.app.as_deref().context("notice needs --app APP")?;
    let text = a.text()?;
    let mut s = connect(peer, a).await?;
    let at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    s.send_notice(app, &a.title, &text, at_ms).await?;
    s.finish().await;
    println!("notice sent to {}", s.remote_name);
    Ok(())
}

fn devices_cmd(a: &Args) -> Result<()> {
    let store = trust::load(&trust_file()?);
    for (id, d) in store.iter() {
        if a.json {
            println!(
                "{}",
                json!({"id": id.to_string(), "name": d.name, "method": format!("{:?}", d.method),
                       "paired_at": d.paired_at, "last_seen": d.last_seen})
            );
        } else {
            println!("{id}  {:<24} {:?}", d.name, d.method);
        }
    }
    if store.is_empty() && !a.json {
        println!("no paired devices yet: run `receive` and scan its QR code");
    }
    Ok(())
}

/// One line per thing that happened, plain or JSON (`--json`), for people and for scripts.
#[derive(Clone, Copy)]
struct Out {
    json: bool,
}

impl Out {
    fn event(self, kind: &str, from: &str, text: &str, extra: &serde_json::Value) {
        if self.json {
            let mut v = json!({"event": kind, "from": from, "text": text});
            if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
                o.extend(e.clone());
            }
            println!("{v}");
        } else if text.is_empty() {
            println!("{kind} from {from}");
        } else {
            println!("{kind} from {from}: {text}");
        }
    }
}

/// What every session of `receive` shares.
struct Host {
    store: Mutex<TrustStore>,
    trust_path: PathBuf,
    visibility: Visibility,
    secret: LinkSecret,
    dir: PathBuf,
    explicit_dir: bool,
    out: Out,
    yes: bool,
}

impl Host {
    fn trust(&self, s: &Session, method: PairMethod) {
        let mut store = self.store.lock().expect("trust store");
        store.trust(
            s.remote_id(),
            &s.remote_name,
            Some(s.remote_platform),
            method,
            trust::now(),
        );
        if let Err(e) = trust::save(&self.trust_path, &store) {
            eprintln!("could not save the trust store: {e:#}");
        }
    }
}

/// Saves files, shows clipboard text and notices, and rings on a ring, until Ctrl+C. Latency probes are
/// answered too. Each device gets its own session task.
async fn receive(peer: &Peer, a: &Args) -> Result<()> {
    let dir = match &a.dir {
        Some(d) => d.clone(),
        None => transfer::downloads_dir()?,
    };
    let trust_path = trust_file()?;
    let mut secret = [0u8; 16];
    secret.copy_from_slice(&vutric_net::new_identity()[..16]);
    let host = Arc::new(Host {
        store: Mutex::new(trust::load(&trust_path)),
        trust_path,
        visibility: Visibility::pairing(trust::now()),
        secret: LinkSecret(secret),
        dir,
        explicit_dir: a.dir.is_some(),
        out: Out { json: a.json },
        yes: a.yes,
    });
    show_qr(peer, Some(&host.secret)).await?;
    eprintln!(
        "saving into {}; pairing window open for 10 minutes, then only paired devices (Ctrl+C to stop)",
        host.dir.display()
    );
    loop {
        let gate = host.clone();
        let (s, admission) = peer
            .accept_with(move |id| {
                let store = gate.store.lock().expect("trust store");
                trust::admit(&store, gate.visibility, id, trust::now())
            })
            .await?;
        let host = host.clone();
        tokio::spawn(async move {
            let mut s = s;
            let from = s.remote_name.clone();
            if let Err(e) = serve(&mut s, admission, &host).await {
                host.out
                    .event("closed", &from, &format!("{e:#}"), &json!({}));
            }
        });
    }
}

/// Asks the person at the terminal a yes/no question (no on end of input).
async fn ask(question: String) -> bool {
    tokio::task::spawn_blocking(move || {
        eprint!("{question} [y/N] ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).is_ok() && line.trim().eq_ignore_ascii_case("y")
    })
    .await
    .unwrap_or(false)
}

async fn serve(s: &mut Session, mut admission: Admission, host: &Host) -> Result<()> {
    let from = s.remote_name.clone();
    let out = host.out;
    if admission == Admission::Unknown {
        match s.pair_intent(Some(&host.secret), PAIR_INTENT_WAIT).await? {
            PairIntent::Link => {
                host.trust(s, PairMethod::Ticket);
                admission = Admission::Trusted;
                out.event("paired", &from, "scanned the QR code", &json!({}));
            }
            PairIntent::Code => {
                let code = s.pair_code().await?;
                let yes = ask(format!("{from} shows {code}. Do the digits match?")).await;
                if s.pair_finish(yes).await? {
                    host.trust(s, PairMethod::Code);
                    admission = Admission::Trusted;
                    out.event("paired", &from, "six digits", &json!({}));
                } else {
                    s.finish().await;
                    bail!("pairing not confirmed");
                }
            }
            PairIntent::WrongLink => {
                out.event("guest", &from, "an old or forged pairing link", &json!({}));
            }
            PairIntent::Neither => out.event("guest", &from, "not paired", &json!({})),
        }
    } else {
        out.event("connected", &from, &via(s), &json!({}));
    }
    let clock = Instant::now();
    let mut notices = NoticeLimit::default();
    let mut rings = RingLimit::default();
    let mut ringing: Option<tokio::time::Instant> = None;
    let mut bell = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            msg = s.recv() => {
                let msg = msg?;
                handle(s, msg, admission, host, (&mut notices, &mut rings, &mut ringing), clock).await?;
            }
            _ = bell.tick(), if ringing.is_some() => {
                if ringing.is_some_and(|until| tokio::time::Instant::now() >= until) {
                    ringing = None;
                    out.event("ring-ended", &from, "", &json!({}));
                } else if !out.json {
                    eprint!("\x07");
                    let _ = std::io::stderr().flush();
                }
            }
        }
    }
}

type Limits<'a> = (
    &'a mut NoticeLimit,
    &'a mut RingLimit,
    &'a mut Option<tokio::time::Instant>,
);

async fn handle(
    s: &mut Session,
    msg: Message,
    admission: Admission,
    host: &Host,
    (notices, rings, ringing): Limits<'_>,
    clock: Instant,
) -> Result<()> {
    let from = s.remote_name.clone();
    let out = host.out;
    if let Some(offer) = transfer::IncomingOffer::from_message(&msg) {
        if admission != Admission::Trusted && !host.yes {
            out.event(
                "declined",
                &from,
                &offer.names(3).join(", "),
                &json!({"reason": "not paired (run with --yes to take files from guests)"}),
            );
            return transfer::decline(s, &offer).await;
        }
        let settings = vutric_net::settings::load(
            &vutric_net::settings::desktop_dir()?.join(vutric_net::settings::FILE),
        );
        let mut to =
            transfer::SaveTo::for_peer(s, &settings, host.dir.clone(), transfer::today_utc());
        if host.explicit_dir {
            to.rules.folder = None; // --dir wins over the settings file
        }
        let sum = transfer::receive(s, offer, &to, transfer::log_progress("receiving")).await?;
        out.event(
            "files",
            &from,
            &format!(
                "saved {} file(s), {:.1} MB in {:.1} s ({:.1} MB/s, {}){}",
                sum.files,
                sum.bytes as f64 / 1e6,
                sum.elapsed.as_secs_f64(),
                rate(&sum),
                via(s),
                failed_note("failed", &sum.failed)
            ),
            &json!({"files": sum.files, "bytes": sum.bytes, "failed": sum.failed}),
        );
        return Ok(());
    }
    let paired = companion::accepts(&msg, admission) || ring::accepts(&msg, admission);
    match msg {
        Message::FlingStart(f) => {
            s.send(&Message::EffectStarted {
                id: f.id,
                receiver_micros: vutric_net::micros_since(clock),
            })
            .await?;
        }
        Message::Notice(n) if paired => {
            if notices.allow(Instant::now()) {
                let app = companion::shown(&n.app, MAX_NOTICE_LINE_CHARS, false);
                let title = companion::shown(&n.title, MAX_NOTICE_LINE_CHARS, false);
                let text = companion::shown(&n.text, MAX_NOTICE_TEXT_CHARS, true);
                let line = if title.is_empty() {
                    format!("[{app}] {text}")
                } else {
                    format!("[{app}] {title}: {text}")
                };
                out.event(
                    "notice",
                    &from,
                    &line,
                    &json!({"app": app, "title": title, "body": text, "at_ms": n.at_ms}),
                );
            }
        }
        Message::Clipboard { text } if paired => {
            out.event(
                "clipboard",
                &from,
                &companion::shown(&text, MAX_CLIPBOARD_BYTES, true),
                &json!({}),
            );
        }
        Message::Ring(r) if paired => {
            if !rings.allow(r, Instant::now()) {
                return Ok(());
            }
            if let Some(d) = r.duration() {
                *ringing = Some(tokio::time::Instant::now() + d);
                out.event(
                    "ring",
                    &from,
                    &format!("for {} s", d.as_secs()),
                    &json!({"seconds": d.as_secs()}),
                );
            } else {
                *ringing = None;
                out.event("ring-stopped", &from, "", &json!({}));
            }
        }
        Message::Notice(_) | Message::Clipboard { .. } | Message::Ring(_) => {
            out.event(
                "dropped",
                &from,
                msg.kind(),
                &json!({"reason": "not paired"}),
            );
        }
        _ => {}
    }
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

/// `lan`: this machine's ranked addresses, the QR hints, and who answers a beacon query (R1 field check).
async fn lan_report(peer: &Peer) -> Result<()> {
    let t0 = Instant::now();
    let locals = vutric_net::lan::local_addrs();
    println!("local addresses ({} in {:?}):", locals.len(), t0.elapsed());
    for l in &locals {
        println!(
            "  {:<8?} {:<40} /{:<3} if{} {}",
            l.medium, l.ip, l.prefix, l.iface, l.iface_name
        );
    }
    let t = peer.ticket();
    println!("QR hints ({}):", t.addrs.len());
    for h in &t.addrs {
        println!("  {h}");
    }
    let quic = t.addrs.first().map_or(0, std::net::SocketAddr::port);
    let beacon = vutric_net::beacon::BeaconService::start(peer.id(), quic, false)?;
    let mut rx = beacon.subscribe();
    beacon.query().await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while let Ok(Ok(h)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        println!("heard {} at {} (phone {})", h.id, h.addr, h.phone);
    }
    println!("beacon health: {:?}", beacon.health());
    Ok(())
}

const USAGE: &str = "usage: vutric-net-spike listen|send [PATH...]|fling|files PATH...|receive|ring|clipboard TEXT|notice --app APP TEXT|devices|lan \
[--ticket LINK | --discover] [--dir DIR] [--json] [--yes] [--seconds N] [--stop] [--title T] [--name NAME] [--runs N] [--photo PATH [--count N]] [--lan-only] [--via-internet]";

#[tokio::main]
async fn main() -> Result<()> {
    let a = parse()?;
    if a.mode == "devices" {
        return devices_cmd(&a);
    }
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
    eprintln!("{} ({:?}) id {}", a.name, platform(), peer.id());
    let mut exit = 0;
    let result = match a.mode.as_str() {
        "lan" => lan_report(&peer).await,
        "listen" => listen(&peer, &a).await,
        // Without a target, `fling` shows its QR code and throws at whoever connects (a phone that cannot be
        // found over mDNS, such as an iPhone).
        "fling" if a.ticket.is_none() && !a.discover => listen(&peer, &a).await,
        "send" | "files" if !a.words.is_empty() => files(&peer, &a).await,
        "send" | "fling" => send(&peer, &a).await,
        "receive" => receive(&peer, &a).await,
        "ring" => ring_cmd(&peer, &a).await.map(|c| exit = c),
        "clipboard" if !a.words.is_empty() => clipboard_cmd(&peer, &a).await,
        "notice" if !a.words.is_empty() => notice_cmd(&peer, &a).await,
        _ => Err(anyhow::anyhow!(USAGE)),
    };
    peer.close().await;
    result?;
    if exit != 0 {
        std::process::exit(exit);
    }
    Ok(())
}
