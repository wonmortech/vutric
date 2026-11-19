//! `vutric-mac-compat`: foreground `LocalSend` and Quick Share for the macOS menu-bar agent (ADR-0117).
//!
//! Started by the agent while the user has the switch on; ends when its stdin closes or on `stop`. Speaks
//! [`vutric_host_mac::compat::Command`] on stdin and [`vutric_host_mac::compat::Event`] on stdout.
//!
//! ```text
//! vutric-mac-compat --alias NAME [--adapter PATH] [--port 53317] [--quickshare true|false] [--downloads DIR]
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use tokio::io::{AsyncWriteExt as _, BufReader};
use vutric_compat_localsend::host::{Adapter, read_line};
use vutric_compat_localsend::ipc::{self, FromAdapter, Start};
use vutric_compat_localsend::proto::DeviceType;
use vutric_host_mac::compat::{Bridge, Command, Event, place};

struct Args {
    alias: String,
    adapter: PathBuf,
    port: u16,
    quickshare: bool,
    downloads: PathBuf,
    support: PathBuf,
    caches: PathBuf,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(std::env::temp_dir, PathBuf::from)
}

impl Args {
    fn parse() -> Result<Self> {
        let here = std::env::current_exe()?
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let home = home();
        let mut a = Self {
            alias: "Vutric".into(),
            adapter: here.join("vutric-compat"),
            port: 53317,
            quickshare: true,
            downloads: home.join("Downloads"),
            support: home.join("Library/Application Support/Vutric"),
            caches: home.join("Library/Caches/io.wonmor.vutric"),
        };
        let mut it = std::env::args().skip(1);
        while let Some(flag) = it.next() {
            let mut value = || it.next().with_context(|| format!("{flag} needs a value"));
            match flag.as_str() {
                "--alias" => a.alias = vutric_core::companion::shown(&value()?, 64, false),
                "--adapter" => a.adapter = value()?.into(),
                "--port" => a.port = value()?.parse()?,
                "--quickshare" => a.quickshare = value()?.parse()?,
                "--downloads" => a.downloads = value()?.into(),
                other => bail!("unknown flag {other}"),
            }
        }
        if a.alias.is_empty() {
            a.alias = "Vutric".into();
        }
        Ok(a)
    }
}

async fn emit(out: &mut tokio::io::Stdout, e: &Event) -> Result<()> {
    out.write_all(&ipc::line(e)).await?;
    out.flush().await?;
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse()?;
    let start = Start {
        alias: args.alias.clone(),
        device_type: DeviceType::Desktop,
        port: args.port,
        bind: "0.0.0.0".parse()?,
        staging: args.caches.join("compat-staging"),
        identity: args.support.join("compat-identity"),
        pin: None,
        multicast: true,
        decide_timeout_secs: 60,
        quickshare: args.quickshare,
        kdeconnect: false,
    };
    std::fs::create_dir_all(&args.support)?;
    let mut out = tokio::io::stdout();
    let (mut adapter, mut rx) = match Adapter::spawn(&args.adapter, start).await {
        Ok(x) => x,
        Err(e) => {
            emit(
                &mut out,
                &Event::Error {
                    message: format!("{e:#}"),
                },
            )
            .await?;
            return Err(e);
        }
    };
    let mut bridge = Bridge::default();
    // Lines are read on their own task: a `read_line` cancelled by `select!` would lose half a line.
    let (cmd_tx, mut commands) = tokio::sync::mpsc::channel::<Option<Command>>(16);
    tokio::spawn(async move {
        let mut stdin = BufReader::new(tokio::io::stdin());
        let mut buf = Vec::new();
        while let Ok(true) = read_line(&mut stdin, &mut buf, ipc::MAX_LINE).await {
            if cmd_tx.send(ipc::parse::<Command>(&buf)).await.is_err() {
                return;
            }
        }
    });
    loop {
        tokio::select! {
            ev = rx.recv() => {
                let Some(ev) = ev else { break };
                let told = match ev {
                    FromAdapter::Received { session_id, path, .. } => Some(
                        match adapter.received_file(&path).map(|p| place(&p, &args.downloads)) {
                            Some(Ok(to)) => Event::Received {
                                session: session_id,
                                name: to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                                path: to,
                            },
                            Some(Err(e)) => Event::Error { message: format!("could not save a received file: {e}") },
                            None => Event::Error { message: "the adapter reported a file outside its folder".into() },
                        },
                    ),
                    other => bridge.on_adapter(other),
                };
                if let Some(e) = told {
                    emit(&mut out, &e).await?;
                }
            }
            cmd = commands.recv() => {
                // The agent closed our stdin (or sent a line too long to read): stop.
                let Some(cmd) = cmd else { break };
                let Some(cmd) = cmd else {
                    emit(&mut out, &Event::Error { message: "unknown command".into() }).await?;
                    continue;
                };
                match cmd {
                    Command::Decide { session, accept } => {
                        let ids = bridge.accept(&session, accept);
                        adapter.decide(&session, ids).await?;
                    }
                    Command::Send { request_id, device, paths } => match bridge.target(&device) {
                        Some((to, https)) => adapter.send(request_id, to, https, paths, None).await?,
                        None => emit(&mut out, &Event::SendFailed {
                            request_id,
                            reason: "the device is no longer on the network".into(),
                        }).await?,
                    },
                    Command::Announce => adapter.announce().await?,
                    Command::Stop => break,
                }
            }
        }
    }
    adapter.stop().await
}
