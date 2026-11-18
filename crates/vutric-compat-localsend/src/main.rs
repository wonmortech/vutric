//! The `LocalSend` adapter process (ADR-0054). Started by the daemon through
//! [`vutric_compat_localsend::host::Adapter`]; speaks [`vutric_compat_localsend::ipc`] on stdin/stdout and exits
//! when stdin closes. All foreign bytes are parsed here, never in the daemon.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use tokio::io::{AsyncWriteExt as _, BufReader};
use tokio::sync::{Mutex, mpsc, oneshot};
use vutric_compat_localsend::client::{self, Target};
use vutric_compat_localsend::discovery::Discovery;
use vutric_compat_localsend::host::read_line;
use vutric_compat_localsend::ipc::{self, FromAdapter, OfferFile, Start, ToAdapter};
use vutric_compat_localsend::server::{self, Decision, Event, Receiver};
use vutric_compat_localsend::tls::Identity;

type Out = mpsc::Sender<FromAdapter>;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    let mut input = BufReader::new(tokio::io::stdin());
    let mut buf = Vec::new();
    if !read_line(&mut input, &mut buf, ipc::MAX_LINE).await? {
        return Ok(());
    }
    let Some(ToAdapter::Start(start)) = ipc::parse(buf.trim_ascii_end()) else {
        anyhow::bail!("first message must be start");
    };

    let (out, mut out_rx) = mpsc::channel::<FromAdapter>(64);
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(m) = out_rx.recv().await {
            if stdout.write_all(&ipc::line(&m)).await.is_err() || stdout.flush().await.is_err() {
                break;
            }
        }
    });

    if let Err(e) = run(start, &mut input, out.clone()).await {
        let _ = out
            .send(FromAdapter::Error {
                message: format!("{e:#}"),
            })
            .await;
    }
    drop(out);
    let _ = writer.await;
    Ok(())
}

async fn run(start: Start, input: &mut BufReader<tokio::io::Stdin>, out: Out) -> Result<()> {
    let identity = Identity::load_or_create(&start.identity).context("identity")?;
    let (receiver, events) = Receiver::start(
        server::Config {
            alias: start.alias.clone(),
            device_type: start.device_type,
            port: start.port,
            bind: start.bind,
            staging: start.staging.clone(),
            pin: start.pin.clone(),
            decide_timeout: Duration::from_secs(start.decide_timeout_secs.clamp(5, 600)),
        },
        &identity,
    )
    .await?;
    let us = server::our_info(
        &start.alias,
        start.device_type,
        receiver.fingerprint(),
        receiver.addr().port(),
    );
    let discovery = if start.multicast {
        Some(start_discovery(start.port, us.clone(), identity.clone(), out.clone()).await?)
    } else {
        None
    };
    out.send(FromAdapter::Ready {
        fingerprint: receiver.fingerprint().to_owned(),
        port: receiver.addr().port(),
    })
    .await?;

    let pending: Pending = Arc::default();
    spawn_bridge(events, out.clone(), Arc::clone(&pending));

    let mut buf = Vec::new();
    while read_line(input, &mut buf, ipc::MAX_LINE).await? {
        let Some(cmd) = ipc::parse::<ToAdapter>(buf.trim_ascii_end()) else {
            continue;
        };
        match cmd {
            ToAdapter::Start(_) => {}
            ToAdapter::Decide { session_id, accept } => {
                if let Some(reply) = pending.lock().await.remove(&session_id) {
                    let _ = reply.send(if accept.is_empty() {
                        Decision::Reject
                    } else {
                        Decision::Accept(accept)
                    });
                }
            }
            ToAdapter::Announce => {
                if let Some(d) = &discovery {
                    let _ = d.announce().await;
                }
            }
            ToAdapter::Send {
                request_id,
                to,
                https,
                paths,
                pin,
            } => spawn_send(
                out.clone(),
                identity.clone(),
                us.clone(),
                request_id,
                Target { addr: to, https },
                paths,
                pin,
            ),
            ToAdapter::Share {
                request_id,
                paths,
                ttl_secs,
            } => {
                let msg = match receiver.share(&paths, Duration::from_secs(ttl_secs)) {
                    Ok(token) => FromAdapter::Shared { request_id, token },
                    Err(e) => FromAdapter::ShareFailed {
                        request_id,
                        reason: format!("{e:#}"),
                    },
                };
                out.send(msg).await?;
            }
            ToAdapter::Unshare { token } => receiver.unshare(&token),
            ToAdapter::Stop => break,
        }
    }
    drop(discovery);
    drop(receiver);
    Ok(())
}

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<Decision>>>>;

/// Forwards receiver events to the daemon; offers wait in `pending` for the daemon's `decide`.
fn spawn_bridge(mut events: mpsc::Receiver<Event>, out: Out, pending: Pending) {
    tokio::spawn(async move {
        while let Some(ev) = events.recv().await {
            let msg = match ev {
                Event::Seen { info, ip } => FromAdapter::Seen {
                    to: std::net::SocketAddr::new(ip, info.port.unwrap_or(0)),
                    https: info.protocol != Some(vutric_compat_localsend::proto::Scheme::Http),
                    alias: info.alias,
                    device_type: info.device_type,
                },
                Event::Offer { offer, reply } => {
                    pending.lock().await.insert(offer.session_id.clone(), reply);
                    FromAdapter::Offer {
                        session_id: offer.session_id,
                        peer: offer.peer,
                        peer_cert: offer.peer_cert,
                        alias: offer.from.alias,
                        files: offer
                            .files
                            .into_iter()
                            .map(|f| OfferFile {
                                id: f.id,
                                name: f.file_name,
                                size: f.size,
                                file_type: f.file_type,
                            })
                            .collect(),
                    }
                }
                Event::Received {
                    session_id,
                    file_id,
                    name,
                    path,
                    size,
                } => FromAdapter::Received {
                    session_id,
                    file_id,
                    name,
                    path,
                    size,
                },
                Event::Finished { session_id } => FromAdapter::Finished { session_id },
                Event::Cancelled { session_id } => FromAdapter::Cancelled { session_id },
                Event::Downloaded { token, name } => FromAdapter::Downloaded { token, name },
            };
            if out.send(msg).await.is_err() {
                break;
            }
        }
    });
}

/// Runs one send in the background and reports how it went.
fn spawn_send(
    out: Out,
    identity: Identity,
    us: vutric_compat_localsend::proto::DeviceInfo,
    request_id: u64,
    t: Target,
    paths: Vec<std::path::PathBuf>,
    pin: Option<String>,
) {
    tokio::spawn(async move {
        let msg = match client::send(&identity, t, &us, &paths, pin.as_deref()).await {
            Ok(s) => FromAdapter::Sent {
                request_id,
                files: s.files,
                bytes: s.bytes,
                peer_cert: s.peer_cert,
            },
            Err(e) => FromAdapter::SendFailed {
                request_id,
                reason: format!("{e:#}"),
            },
        };
        let _ = out.send(msg).await;
    });
}

/// Joins the multicast group, forwards devices seen, and announces us once.
async fn start_discovery(
    port: u16,
    us: vutric_compat_localsend::proto::DeviceInfo,
    identity: Identity,
    out: Out,
) -> Result<Discovery> {
    let (d, mut found) = Discovery::start(port, &[], us, identity)?;
    tokio::spawn(async move {
        while let Some(f) = found.recv().await {
            let _ = out
                .send(FromAdapter::Seen {
                    alias: f.info.alias,
                    device_type: f.info.device_type,
                    to: f.target.addr,
                    https: f.target.https,
                })
                .await;
        }
    });
    let _ = d.announce().await;
    Ok(d)
}
