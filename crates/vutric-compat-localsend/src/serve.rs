//! The compat adapters' event loop (ADR-0054, ADR-0056, ADR-0062, ADR-0066): `LocalSend`, the browser page,
//! Quick Share and KDE Connect, driven by [`crate::ipc`] lines. All foreign bytes are parsed here, never in the
//! host.
//!
//! - On desktops it runs as the `vutric-compat` process, on stdin/stdout, started by [`crate::host::Adapter::spawn`].
//! - On Android (ADR-0066) the app runs the same loop in a separate `:compat` process, on one end of a socket pair
//!   ([`serve_socket_fd`]); the app process talks to it with [`crate::host::Adapter::attach`].
//!
//! It returns when its input closes or a `Stop` arrives.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::client::{self, Target};
use crate::discovery::Discovery;
use crate::host::read_line;
use crate::ipc::{self, FromAdapter, OfferFile, Start, ToAdapter, Via};
use crate::server::{self, Decision, Event, Receiver};
use crate::tls::Identity;
use anyhow::{Context as _, Result};
use tokio::io::{AsyncBufRead, AsyncRead, AsyncWrite, AsyncWriteExt as _, BufReader};
use tokio::sync::{Mutex, mpsc, oneshot};
use vutric_compat_kdeconnect as kdc;
use vutric_compat_quickshare as qs;
use vutric_compat_wormhole as wh;

type Out = mpsc::Sender<FromAdapter>;

/// Runs the adapters on any byte stream: reads [`ToAdapter`] lines from `input` (the first must be `Start`) and
/// writes [`FromAdapter`] lines to `output`. A failure after `Start` is reported as `FromAdapter::Error` before
/// returning.
pub async fn serve<R, W>(input: R, mut output: W) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut input = BufReader::new(input);
    let mut buf = Vec::new();
    if !read_line(&mut input, &mut buf, ipc::MAX_LINE).await? {
        return Ok(());
    }
    let Some(ToAdapter::Start(start)) = ipc::parse(buf.trim_ascii_end()) else {
        anyhow::bail!("first message must be start");
    };

    let (out, mut out_rx) = mpsc::channel::<FromAdapter>(64);
    let writer = tokio::spawn(async move {
        while let Some(m) = out_rx.recv().await {
            if output.write_all(&ipc::line(&m)).await.is_err() || output.flush().await.is_err() {
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

#[allow(clippy::too_many_lines)] // one dispatch over every command, read top to bottom
async fn run<R: AsyncBufRead + Unpin>(start: Start, input: &mut R, out: Out) -> Result<()> {
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
    let pending: Pending = Arc::default();
    let quickshare = start_quickshare(&start, out.clone(), Arc::clone(&pending)).await?;
    let kde = start_kdeconnect(&start, out.clone(), Arc::clone(&pending)).await?;
    out.send(FromAdapter::Ready {
        fingerprint: receiver.fingerprint().to_owned(),
        port: receiver.addr().port(),
        quickshare_port: quickshare.as_ref().map(|(r, _)| r.addr().port()),
        kdeconnect_port: kde.as_ref().map(|(r, _)| r.addr().port()),
        kdeconnect_id: kde.as_ref().map(|(_, id)| id.clone()),
    })
    .await?;
    spawn_bridge(events, out.clone(), Arc::clone(&pending));

    let mut buf = Vec::new();
    while read_line(input, &mut buf, ipc::MAX_LINE).await? {
        let Some(cmd) = ipc::parse::<ToAdapter>(buf.trim_ascii_end()) else {
            continue;
        };
        match cmd {
            ToAdapter::Start(_) => {}
            ToAdapter::Decide { session_id, accept } => decide(&pending, &session_id, accept).await,
            ToAdapter::Announce => {
                announce(discovery.as_ref(), kde.as_ref(), start.multicast).await;
            }
            cmd @ ToAdapter::KdeSend { .. } => kde_send(kde.as_ref(), &out, cmd),
            ToAdapter::WormholeSend {
                request_id,
                paths,
                text,
                servers,
            } => {
                let cfg = wormhole_config(&start, servers);
                tokio::spawn(wormhole_send(cfg, out.clone(), request_id, paths, text));
            }
            ToAdapter::WormholeReceive {
                request_id,
                code,
                servers,
            } => {
                let job = WormholeReceive {
                    cfg: wormhole_config(&start, servers),
                    out: out.clone(),
                    pending: Arc::clone(&pending),
                    staging: start.staging.clone(),
                    decide_timeout: Duration::from_secs(start.decide_timeout_secs.clamp(5, 600)),
                };
                tokio::spawn(job.run(request_id, code));
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
    drop(kde);
    drop(quickshare);
    drop(discovery);
    drop(receiver);
    Ok(())
}

/// An offer waiting for the daemon's answer, by protocol.
#[derive(Debug)]
enum Reply {
    LocalSend(oneshot::Sender<Decision>),
    QuickShare(oneshot::Sender<qs::server::Decision>),
    KdePair(oneshot::Sender<bool>),
    Wormhole(oneshot::Sender<bool>),
}

type Pending = Arc<Mutex<HashMap<String, Reply>>>;

/// Passes the daemon's answer on; an empty `accept` rejects. Quick Share takes all files or none.
async fn decide(pending: &Pending, session_id: &str, accept: Vec<String>) {
    match pending.lock().await.remove(session_id) {
        Some(Reply::LocalSend(r)) => {
            let _ = r.send(if accept.is_empty() {
                Decision::Reject
            } else {
                Decision::Accept(accept)
            });
        }
        Some(Reply::QuickShare(r)) => {
            let _ = r.send(if accept.is_empty() {
                qs::server::Decision::Reject
            } else {
                qs::server::Decision::Accept
            });
        }
        Some(Reply::KdePair(r) | Reply::Wormhole(r)) => {
            let _ = r.send(!accept.is_empty());
        }
        None => {}
    }
}

/// Starts the KDE Connect receiver when `start.kdeconnect` (UDP discovery and an announcement when multicast is
/// allowed) and forwards its events. Its identity and paired devices live next to the `LocalSend` identity file.
async fn start_kdeconnect(
    start: &Start,
    out: Out,
    pending: Pending,
) -> Result<Option<(Arc<kdc::receiver::Receiver>, String)>> {
    if !start.kdeconnect {
        return Ok(None);
    }
    let tls = kdc::tls::Identity::load_or_create(&start.identity.with_file_name("kdeconnect.id"))
        .context("kde connect identity")?;
    let id = tls.device_id.clone();
    let kind = match start.device_type {
        crate::proto::DeviceType::Mobile => kdc::proto::DeviceType::Phone,
        _ => kdc::proto::DeviceType::Desktop,
    };
    let fixed = start.port != 0;
    let (rx, mut events) = kdc::receiver::Receiver::start(kdc::receiver::Config {
        me: kdc::link::Me {
            tls,
            name: start.alias.clone(),
            kind,
        },
        bind: start.bind,
        tcp_ports: if fixed {
            kdc::receiver::TCP_PORTS
        } else {
            0..=0
        },
        udp_port: start.multicast.then_some(kdc::receiver::UDP_PORT),
        staging: start.staging.clone(),
        trust: kdc::trust::Trust::load(&start.identity.with_file_name("kdeconnect-trusted.json")),
        decide_timeout: Duration::from_secs(start.decide_timeout_secs.clamp(5, 600)),
        payload_ports: if fixed { kdc::payload::PORTS } else { 0..=0 },
    })
    .await?;
    if start.multicast {
        rx.announce().await;
    }
    tokio::spawn(async move {
        while let Some(ev) = events.recv().await {
            let msg = kde_message(ev, &pending).await;
            if out.send(msg).await.is_err() {
                break;
            }
        }
    });
    Ok(Some((Arc::new(rx), id)))
}

/// "I am here" over `LocalSend` multicast and the KDE Connect UDP broadcast, where they run.
async fn announce(
    discovery: Option<&Discovery>,
    kde: Option<&(Arc<kdc::receiver::Receiver>, String)>,
    broadcast: bool,
) {
    if let Some(d) = discovery {
        let _ = d.announce().await;
    }
    if let Some((k, _)) = kde.filter(|_| broadcast) {
        k.announce().await;
    }
}

/// Sends to a paired KDE Connect device in the background and reports how it went.
fn kde_send(kde: Option<&(Arc<kdc::receiver::Receiver>, String)>, out: &Out, cmd: ToAdapter) {
    let ToAdapter::KdeSend {
        request_id,
        to,
        device_id,
        paths,
    } = cmd
    else {
        return;
    };
    let (kde, out) = (kde.map(|(k, _)| Arc::clone(k)), out.clone());
    tokio::spawn(async move {
        let sent = match &kde {
            Some(k) => k.send(to, &device_id, &paths).await,
            None => Err(anyhow::anyhow!("KDE Connect is off")),
        };
        let msg = match sent {
            Ok(bytes) => FromAdapter::Sent {
                request_id,
                files: paths.len(),
                bytes,
                peer_cert: None,
            },
            Err(e) => FromAdapter::SendFailed {
                request_id,
                reason: format!("{e:#}"),
            },
        };
        let _ = out.send(msg).await;
    });
}

/// One KDE Connect event as a daemon message; a pairing question waits in `pending`.
async fn kde_message(ev: kdc::receiver::Event, pending: &Pending) -> FromAdapter {
    use kdc::receiver::Event as K;
    match ev {
        K::Seen {
            device_id,
            name,
            addr,
        } => FromAdapter::Seen {
            alias: name,
            device_type: None,
            to: addr,
            https: true,
            via: Via::Kdeconnect,
            device_id: Some(device_id),
        },
        K::PairRequest {
            device_id,
            name,
            key,
            answer,
        } => {
            let session_id = format!("kdc-pair-{device_id}");
            pending
                .lock()
                .await
                .insert(session_id.clone(), Reply::KdePair(answer));
            FromAdapter::PairRequest {
                session_id,
                via: Via::Kdeconnect,
                device_id,
                alias: name,
                code: key,
            }
        }
        K::Paired { device_id, name } => FromAdapter::Paired {
            via: Via::Kdeconnect,
            device_id,
            alias: name,
        },
        K::Unpaired { device_id } => FromAdapter::Unpaired {
            via: Via::Kdeconnect,
            device_id,
        },
        K::File {
            device_id,
            path,
            size,
            ..
        } => FromAdapter::Received {
            session_id: format!("kdc-{device_id}"),
            file_id: String::new(),
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path,
            size,
        },
        K::Text {
            device_id, text, ..
        } => FromAdapter::Text {
            session_id: format!("kdc-{device_id}"),
            kind: 1,
            text,
        },
        K::Url { device_id, url, .. } => FromAdapter::Text {
            session_id: format!("kdc-{device_id}"),
            kind: 2,
            text: url,
        },
    }
}

/// Starts the Quick Share receiver when `start.quickshare` (and its mDNS advert when multicast is allowed) and
/// forwards its events.
async fn start_quickshare(
    start: &Start,
    out: Out,
    pending: Pending,
) -> Result<Option<(qs::server::Receiver, Option<qs::mdns::Advert>)>> {
    if !start.quickshare {
        return Ok(None);
    }
    let device = match start.device_type {
        crate::proto::DeviceType::Mobile => qs::endpoint::Device::Phone,
        _ => qs::endpoint::Device::Laptop,
    };
    let (rx, mut events) = qs::server::Receiver::start(qs::server::Config {
        name: start.alias.clone(),
        device,
        port: 0,
        bind: start.bind,
        staging: start.staging.clone(),
        decide_timeout: Duration::from_secs(start.decide_timeout_secs.clamp(5, 600)),
    })
    .await?;
    let advert = if start.multicast {
        qs::mdns::Advert::start(rx.endpoint_id(), rx.endpoint_info(), rx.addr().port()).ok()
    } else {
        None
    };
    tokio::spawn(async move {
        while let Some(ev) = events.recv().await {
            let msg = match ev {
                qs::server::Event::Offer { offer, reply } => {
                    pending
                        .lock()
                        .await
                        .insert(offer.session_id.clone(), Reply::QuickShare(reply));
                    FromAdapter::Offer {
                        session_id: offer.session_id,
                        peer: offer.peer,
                        peer_cert: None,
                        alias: offer.from.unwrap_or_else(|| "Quick Share".to_owned()),
                        files: offer
                            .files
                            .into_iter()
                            .enumerate()
                            .map(|(n, f)| OfferFile {
                                id: n.to_string(),
                                name: f.name,
                                size: f.size,
                                file_type: f.mime,
                            })
                            .collect(),
                        via: Via::Quickshare,
                        code: Some(offer.pin),
                    }
                }
                qs::server::Event::Received {
                    session_id,
                    name,
                    path,
                    size,
                } => FromAdapter::Received {
                    session_id,
                    file_id: String::new(),
                    name,
                    path,
                    size,
                },
                qs::server::Event::Text {
                    session_id,
                    kind,
                    text,
                } => FromAdapter::Text {
                    session_id,
                    kind,
                    text,
                },
                qs::server::Event::Finished { session_id } => FromAdapter::Finished { session_id },
                qs::server::Event::Cancelled { session_id } => {
                    FromAdapter::Cancelled { session_id }
                }
            };
            if out.send(msg).await.is_err() {
                break;
            }
        }
    });
    Ok(Some((rx, advert)))
}

/// Forwards receiver events to the daemon; offers wait in `pending` for the daemon's `decide`.
fn spawn_bridge(mut events: mpsc::Receiver<Event>, out: Out, pending: Pending) {
    tokio::spawn(async move {
        while let Some(ev) = events.recv().await {
            let msg = match ev {
                Event::Seen { info, ip } => FromAdapter::Seen {
                    to: std::net::SocketAddr::new(ip, info.port.unwrap_or(0)),
                    https: info.protocol != Some(crate::proto::Scheme::Http),
                    alias: info.alias,
                    device_type: info.device_type,
                    via: Via::Localsend,
                    device_id: None,
                },
                Event::Offer { offer, reply } => {
                    pending
                        .lock()
                        .await
                        .insert(offer.session_id.clone(), Reply::LocalSend(reply));
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
                        via: Via::Localsend,
                        code: None,
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

fn wormhole_config(start: &Start, servers: ipc::WormholeServers) -> wh::transfer::Config {
    let d = wh::transfer::Config::default();
    wh::transfer::Config {
        mailbox: servers.mailbox.unwrap_or(d.mailbox),
        relay: servers.relay.or(d.relay),
        bind: start.bind,
    }
}

/// One magic-wormhole send (C7): allocate a code, report it, wait for the receiver, send, report.
async fn wormhole_send(
    cfg: wh::transfer::Config,
    out: Out,
    request_id: u64,
    paths: Vec<std::path::PathBuf>,
    text: Option<String>,
) {
    let r: Result<(usize, u64)> = async {
        if text.is_some() != paths.is_empty() || paths.len() > 1 {
            anyhow::bail!("magic-wormhole sends one file or one text per code");
        }
        let pending = wh::wormhole::Wormhole::create(&cfg.mailbox, wh::transfer::APPID).await?;
        out.send(FromAdapter::WormholeCode {
            request_id,
            code: pending.code.as_str().to_owned(),
        })
        .await?;
        let mut w = pending.open().await?;
        let done = match (&text, paths.first()) {
            (Some(t), _) => wh::transfer::send_text(&mut w, t)
                .await
                .map(|()| (1, t.len() as u64)),
            (None, Some(p)) => wh::transfer::send_file(&mut w, p, &cfg, &|_| {})
                .await
                .map(|b| (1, b)),
            (None, None) => unreachable!("checked above"),
        };
        w.close(if done.is_ok() { "happy" } else { "errory" }).await;
        done
    }
    .await;
    let msg = match r {
        Ok((files, bytes)) => FromAdapter::Sent {
            request_id,
            files,
            bytes,
            peer_cert: None,
        },
        Err(e) => FromAdapter::SendFailed {
            request_id,
            reason: format!("{e:#}"),
        },
    };
    let _ = out.send(msg).await;
}

/// One magic-wormhole receive (C7): join by code, ask the user about a file, stage it.
struct WormholeReceive {
    cfg: wh::transfer::Config,
    out: Out,
    pending: Pending,
    staging: std::path::PathBuf,
    decide_timeout: Duration,
}

impl WormholeReceive {
    async fn run(self, request_id: u64, code: String) {
        let session_id = format!("wh-{request_id}");
        if let Err(e) = self.receive(request_id, &session_id, &code).await {
            self.pending.lock().await.remove(&session_id);
            let _ = self
                .out
                .send(FromAdapter::WormholeFailed {
                    request_id,
                    reason: format!("{e:#}"),
                })
                .await;
        }
    }

    async fn receive(&self, request_id: u64, session_id: &str, code: &str) -> Result<()> {
        use wh::transfer::{self as t, Offer};
        let code = wh::code::Code::parse(code).context("not a wormhole code")?;
        let mut w = wh::wormhole::Wormhole::join(&self.cfg.mailbox, t::APPID, &code).await?;
        let (offer, hints) = t::receive_offer(&mut w).await?;
        let (name, size) = match offer {
            Offer::Text(text) => {
                t::accept_text(&mut w).await?;
                w.close("happy").await;
                self.out
                    .send(FromAdapter::Text {
                        session_id: session_id.to_owned(),
                        kind: 1,
                        text,
                    })
                    .await?;
                self.out
                    .send(FromAdapter::Finished {
                        session_id: session_id.to_owned(),
                    })
                    .await?;
                return Ok(());
            }
            Offer::File { name, size } => (name, size),
        };
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .await
            .insert(session_id.to_owned(), Reply::Wormhole(tx));
        self.out
            .send(FromAdapter::Offer {
                session_id: session_id.to_owned(),
                peer: std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
                peer_cert: None,
                alias: "magic-wormhole".into(),
                files: vec![OfferFile {
                    id: "0".into(),
                    name: name.clone(),
                    size,
                    file_type: String::new(),
                }],
                via: Via::Wormhole,
                code: None,
            })
            .await?;
        let yes = tokio::time::timeout(self.decide_timeout, rx)
            .await
            .unwrap_or(Ok(false))
            .unwrap_or(false);
        if !yes {
            self.pending.lock().await.remove(session_id);
            t::reject(&mut w).await?;
            w.close("happy").await;
            self.out
                .send(FromAdapter::Cancelled {
                    session_id: session_id.to_owned(),
                })
                .await?;
            return Ok(());
        }
        tokio::fs::create_dir_all(&self.staging).await?;
        let part = self.staging.join(format!("wh-{request_id}.vutric-part"));
        let got = t::accept_file(&mut w, size, hints, &part, &self.cfg, &|_| {}).await;
        w.close(if got.is_ok() { "happy" } else { "errory" }).await;
        if let Err(e) = got {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(e);
        }
        let path = t::free_name(&self.staging, &name);
        tokio::fs::rename(&part, &path).await?;
        self.out
            .send(FromAdapter::Received {
                session_id: session_id.to_owned(),
                file_id: "0".into(),
                name,
                path,
                size,
            })
            .await?;
        self.out
            .send(FromAdapter::Finished {
                session_id: session_id.to_owned(),
            })
            .await?;
        Ok(())
    }
}

/// Runs one send in the background and reports how it went.
fn spawn_send(
    out: Out,
    identity: Identity,
    us: crate::proto::DeviceInfo,
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
    us: crate::proto::DeviceInfo,
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
                    via: Via::Localsend,
                    device_id: None,
                })
                .await;
        }
    });
    let _ = d.announce().await;
    Ok(d)
}

/// [`serve`] on its own small runtime (two worker threads), blocking the calling thread until it returns. For
/// hosts without a Tokio runtime, such as the Android `:compat` service thread.
pub fn serve_blocking<R, W>(input: R, output: W) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("runtime")?
        .block_on(serve(input, output))
}

/// [`serve_blocking`] on a connected Unix stream socket, such as one end of Android's
/// `ParcelFileDescriptor.createSocketPair()` handed to the `:compat` process (ADR-0066).
///
/// # Safety
///
/// `fd` must be an open, connected stream socket that nothing else uses or closes; this takes ownership and
/// closes it when it returns.
#[cfg(unix)]
pub unsafe fn serve_socket_fd(fd: std::os::fd::RawFd) -> Result<()> {
    use std::os::fd::FromRawFd as _;
    // SAFETY: the caller hands over an open, connected stream socket it no longer uses.
    let std = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
    std.set_nonblocking(true)?;
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("runtime")?
        .block_on(async move {
            let (r, w) = tokio::net::UnixStream::from_std(std)?.into_split();
            serve(r, w).await
        })
}
