//! The API the Flutter app calls (`flutter_rust_bridge` generates the Dart side).
//!
//! Every function here is blocking and runs on a bridge worker thread, never the UI thread. Inside, the
//! peer lives on one Tokio runtime owned by this crate, so iroh always has the runtime it needs.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use tokio::runtime::Runtime;
use vutric_core::{PairingTicket, Platform};
use vutric_net::{Peer, Session, probe};

#[cfg(target_os = "ios")]
const PLATFORM: Platform = Platform::Ios;
#[cfg(not(target_os = "ios"))]
const PLATFORM: Platform = Platform::Android;

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

/// Starts (or restarts) this device's LAN-only peer; returns its pairing link for the QR code.
pub fn start(name: String, identity: Vec<u8>) -> Result<String> {
    let key: [u8; 32] = identity
        .as_slice()
        .try_into()
        .context("identity must be 32 bytes")?;
    let old = peer_slot().lock().expect("peer lock").take();
    runtime().block_on(async {
        if let Some(old) = old {
            old.close().await;
        }
        let p = Peer::bind(key, &name, PLATFORM).await?;
        let link = p.ticket().to_uri();
        *peer_slot().lock().expect("peer lock") = Some(p);
        Ok(link)
    })
}

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

/// We connect: send our probes, then answer the peer's.
async fn as_initiator(s: &mut Session, runs: u32) -> Result<Sent> {
    let (_warm, stats) = probe::send_run(s, runs).await?;
    probe::answer_run(s, runs).await?;
    Ok(sent_of(&stats))
}

/// Finds a peer over mDNS, then measures both directions.
pub fn run_discovered(runs: u32) -> Result<RunReport> {
    let p = peer()?;
    runtime().block_on(async {
        let t0 = Instant::now();
        let near = p.discover(Duration::from_secs(15)).await?;
        let found = t0.elapsed();
        let t1 = Instant::now();
        let mut s = p.connect(near.id, &near.addrs).await?;
        let connect = t1.elapsed();
        let sent = as_initiator(&mut s, runs).await?;
        let r = report(&s, found, connect, sent, runs);
        tokio::time::sleep(Duration::from_millis(200)).await;
        s.close();
        r
    })
}

/// Connects with a pairing link (scanned or pasted), then measures both directions.
pub fn run_link(link: String, runs: u32) -> Result<RunReport> {
    let p = peer()?;
    let t = PairingTicket::parse(link.trim())?;
    runtime().block_on(async {
        let t1 = Instant::now();
        let mut s = p.connect(t.endpoint, &t.addrs).await?;
        let connect = t1.elapsed();
        let sent = as_initiator(&mut s, runs).await?;
        let r = report(&s, Duration::ZERO, connect, sent, runs);
        tokio::time::sleep(Duration::from_millis(200)).await;
        s.close();
        r
    })
}

/// Waits for a peer to connect (desktop: `vutric-net-spike send --discover`), answers its probes, then
/// measures this phone → that peer.
pub fn listen(runs: u32) -> Result<RunReport> {
    let p = peer()?;
    runtime().block_on(async {
        let t1 = Instant::now();
        let mut s = p.accept().await?;
        let waited = t1.elapsed();
        probe::answer_run(&mut s, runs).await?;
        let (_warm, stats) = probe::send_run(&mut s, runs).await?;
        let r = report(&s, waited, Duration::ZERO, sent_of(&stats), runs);
        s.close();
        r
    })
}
