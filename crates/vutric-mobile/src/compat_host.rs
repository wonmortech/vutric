//! The app process's side of the foreign-app adapters (C10, C7; ADR-0066, ADR-0067): drives the loop in the
//! Android `:compat` process over a socket pair. [`crate::api`] exposes these to Dart; not built for iOS.

// Same owned-argument shape as the bridge functions in `api` that call these.
#![allow(clippy::needless_pass_by_value)]

use std::future::Future;
use std::sync::OnceLock;

use anyhow::{Context as _, Result};
use tokio::sync::Mutex as AsyncMutex;

use crate::api::{load_settings, runtime};
use crate::frb_generated::StreamSink;

/// The adapters' loop in the `:compat` process, as the app process drives it.
fn compat_slot() -> &'static AsyncMutex<Option<vutric_compat_localsend::host::Adapter>> {
    static COMPAT: OnceLock<AsyncMutex<Option<vutric_compat_localsend::host::Adapter>>> =
        OnceLock::new();
    COMPAT.get_or_init(|| AsyncMutex::new(None))
}

/// The `LocalSend` port (the protocol's default, so other apps find us).
#[cfg(unix)]
const COMPAT_PORT: u16 = 53317;

/// How long a foreign offer waits for the user before the adapter refuses it.
#[cfg(unix)]
const COMPAT_DECIDE_SECS: u64 = 120;

/// Events from the `:compat` process for the app, one JSON object per event, in the adapter's own IPC shape
/// (`{"t":"seen",…}`, `offer`, `pair_request`, `received`, `sent`, `send_failed`, `text`, `wormhole_code`,
/// `wormhole_failed`, …), plus `{"t":"closed"}` when the loop ends. A `received` path is already checked: a
/// plain file directly inside the staging folder, or the event becomes `{"t":"error"}`.
#[cfg(unix)]
fn forward_compat(
    mut rx: tokio::sync::mpsc::Receiver<vutric_compat_localsend::ipc::FromAdapter>,
    staging: std::path::PathBuf,
    sink: StreamSink<String>,
) {
    use vutric_compat_localsend::ipc::FromAdapter;
    runtime().spawn(async move {
        while let Some(ev) = rx.recv().await {
            let ev = match ev {
                FromAdapter::Received {
                    session_id,
                    file_id,
                    name,
                    path,
                    size,
                } => match vutric_compat_localsend::host::checked_path(&staging, &path) {
                    Some(path) => FromAdapter::Received {
                        session_id,
                        file_id,
                        name,
                        path,
                        size,
                    },
                    None => FromAdapter::Error {
                        message: "compat.bad_path".into(),
                    },
                },
                other => other,
            };
            let line = serde_json::to_string(&ev).unwrap_or_default();
            if sink.add(line).is_err() {
                break;
            }
        }
        let _ = sink.add(r#"{"t":"closed"}"#.to_owned());
    });
}

/// Starts driving the adapters over `fd`, the app process's end of the socket pair whose other end Kotlin gave
/// to the `:compat` service (ADR-0066). `alias` is the name other apps see; `data_dir` keeps the adapters' own
/// identity files (never Vutric's), `cache_dir` their staging folder. Events go to `sink` (see the JSON shapes
/// above). A loop already running is stopped first. Android only; elsewhere the error `compat.unavailable`.
pub(crate) fn compat_start(
    fd: i32,
    alias: String,
    data_dir: String,
    cache_dir: String,
    sink: StreamSink<String>,
) -> Result<()> {
    #[cfg(unix)]
    {
        use vutric_compat_localsend::ipc::Start;
        use vutric_compat_localsend::proto::DeviceType;
        anyhow::ensure!(fd >= 0, "compat.unavailable");
        let alias: String = alias.chars().filter(|c| !c.is_control()).take(48).collect();
        let data = std::path::Path::new(&data_dir).join("compat");
        std::fs::create_dir_all(&data)?;
        let staging = std::path::Path::new(&cache_dir).join("compat-staging");
        let start = Start {
            alias: if alias.trim().is_empty() {
                "Vutric".into()
            } else {
                alias
            },
            device_type: DeviceType::Mobile,
            port: COMPAT_PORT,
            bind: std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
            staging: staging.clone(),
            identity: data.join("localsend-identity"),
            pin: None,
            multicast: true,
            decide_timeout_secs: COMPAT_DECIDE_SECS,
            quickshare: false,
            kdeconnect: true,
        };
        runtime().block_on(async move {
            let mut slot = compat_slot().lock().await;
            if let Some(old) = slot.take() {
                let _ = old.stop().await;
            }
            // SAFETY: Kotlin detached `fd` from its ParcelFileDescriptor for us; nothing else uses or closes it.
            let (adapter, rx) =
                unsafe { vutric_compat_localsend::host::Adapter::attach_socket_fd(fd, start) }
                    .await?;
            *slot = Some(adapter);
            forward_compat(rx, staging, sink);
            Ok(())
        })
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, alias, data_dir, cache_dir, sink);
        anyhow::bail!("compat.unavailable")
    }
}

/// Runs `job` on the running adapter; `compat.off` when none runs.
fn with_compat<F>(job: F) -> Result<()>
where
    F: for<'a> FnOnce(
        &'a mut vutric_compat_localsend::host::Adapter,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>,
{
    runtime().block_on(async move {
        let mut slot = compat_slot().lock().await;
        let a = slot.as_mut().context("compat.off")?;
        job(a).await
    })
}

/// Whether the adapters run now.
#[must_use]
pub(crate) fn compat_running() -> bool {
    runtime().block_on(async { compat_slot().lock().await.is_some() })
}

/// Makes `LocalSend` devices on this Wi-Fi answer (they announce only now and then).
pub(crate) fn compat_announce() -> Result<()> {
    with_compat(|a| Box::pin(a.announce()))
}

/// Answers an `offer` or a `pair_request`: the file ids to take (all of them for "Qabul qilish"), or none to
/// refuse. Foreign offers are always asked about (ADR-0010 rule 2).
pub(crate) fn compat_decide(session_id: String, accept: Vec<String>) -> Result<()> {
    with_compat(move |a| Box::pin(async move { a.decide(&session_id, accept).await }))
}

fn addr_of(to: &str) -> Result<std::net::SocketAddr> {
    to.parse().context("not a device address")
}

/// Sends files to a `LocalSend` device at `to` (`ip:port` from its `seen` event). The answer is a `sent` or
/// `send_failed` event with `request_id`.
pub(crate) fn compat_send(
    request_id: u64,
    to: String,
    https: bool,
    paths: Vec<String>,
) -> Result<()> {
    let to = addr_of(&to)?;
    let paths = paths.into_iter().map(std::path::PathBuf::from).collect();
    with_compat(move |a| Box::pin(async move { a.send(request_id, to, https, paths, None).await }))
}

/// Sends files to a KDE Connect device (its `seen` event's address and `device_id`). It must have paired with
/// this phone first (from the KDE Connect app; this phone answers the `pair_request`).
pub(crate) fn compat_kde_send(
    request_id: u64,
    to: String,
    device_id: String,
    paths: Vec<String>,
) -> Result<()> {
    let to = addr_of(&to)?;
    let cmd = vutric_compat_localsend::ipc::ToAdapter::KdeSend {
        request_id,
        to,
        device_id,
        paths: paths.into_iter().map(std::path::PathBuf::from).collect(),
    };
    with_compat(move |a| Box::pin(async move { a.kde_send(cmd).await }))
}

/// magic-wormhole goes through public internet servers (ADR-0067): refused when the route keeps everything on
/// this network.
fn wormhole_allowed() -> Result<()> {
    anyhow::ensure!(load_settings().route.uses_internet(), "wormhole.lan_only");
    Ok(())
}

/// Sends one file or a text by magic-wormhole (C7). The code to read out arrives as `wormhole_code`, the end as
/// `sent` or `send_failed`.
pub(crate) fn compat_wormhole_send(
    request_id: u64,
    paths: Vec<String>,
    text: Option<String>,
) -> Result<()> {
    wormhole_allowed()?;
    let cmd = vutric_compat_localsend::ipc::ToAdapter::WormholeSend {
        request_id,
        paths: paths.into_iter().map(std::path::PathBuf::from).collect(),
        text,
        servers: vutric_compat_localsend::ipc::WormholeServers::default(),
    };
    with_compat(move |a| Box::pin(async move { a.wormhole(cmd).await }))
}

/// Receives by a magic-wormhole code the user typed (C7): a file arrives as an `offer` (session
/// `wh-<request_id>`, asked about like any other), a text as `text`, a failure as `wormhole_failed`.
pub(crate) fn compat_wormhole_receive(request_id: u64, code: String) -> Result<()> {
    wormhole_allowed()?;
    let cmd = vutric_compat_localsend::ipc::ToAdapter::WormholeReceive {
        request_id,
        code: code.trim().to_owned(),
        servers: vutric_compat_localsend::ipc::WormholeServers::default(),
    };
    with_compat(move |a| Box::pin(async move { a.wormhole(cmd).await }))
}

/// Stops the adapters; the `:compat` process ends its loop and the service stops itself.
pub(crate) fn compat_stop() {
    runtime().block_on(async {
        if let Some(a) = compat_slot().lock().await.take() {
            let _ = a.stop().await;
        }
    });
}
