//! The daemon's end of the `vutric-surface` socket (ADR-0090 §3), as the daemon will use it: bind a socket in
//! `$XDG_RUNTIME_DIR/vutric/`, have the surface started (spawned directly on layer-shell desktops, launched by the
//! GNOME extension on GNOME), accept its connection, then exchange [`proto`] frames through two channels.
//!
//! `examples/surface_driver.rs` is the same flow as a blocking test script; CI runs that one under headless sway
//! and GNOME Shell.
//!
//! The GNOME extension kills the surface when the bus name that asked for it goes away, so the caller must keep
//! the `zbus::Connection` it passes to [`SurfaceLink::start`] for as long as it wants the surface.

use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::UnixListener;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use crate::proto::{self, Decoder, FromSurface, ToSurface};

/// How the surface process gets started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    /// Run this `vutric-surface` binary ourselves (KDE Plasma, wlroots, COSMIC).
    Spawn(PathBuf),
    /// Ask the GNOME extension to launch it as a trusted Wayland client.
    Gnome,
}

/// How long the surface gets to connect after being started.
pub const CONNECT_WITHIN: Duration = Duration::from_secs(20);

/// Read buffer for frames from the surface (they are small: replies and stats).
const READ_CHUNK: usize = 16 * 1024;

/// `$XDG_RUNTIME_DIR/vutric/surface-<pid>.sock`.
#[must_use]
pub fn socket_path(runtime_dir: &Path, pid: u32) -> PathBuf {
    runtime_dir
        .join("vutric")
        .join(format!("surface-{pid}.sock"))
}

/// A connected surface process.
#[derive(Debug)]
pub struct SurfaceLink {
    tx: mpsc::UnboundedSender<ToSurface>,
    events: mpsc::UnboundedReceiver<FromSurface>,
    /// Kept so the process dies with the link (`kill_on_drop`); `None` when the extension launched it.
    _child: Option<Child>,
}

impl SurfaceLink {
    /// Starts the surface and waits for it to connect. `bus` is required for [`Launch::Gnome`].
    ///
    /// # Errors
    /// No runtime folder, the socket cannot be bound, the process cannot be started, the extension refuses, or
    /// the surface does not connect within `within`.
    pub async fn start(
        launch: Launch,
        bus: Option<&zbus::Connection>,
        within: Duration,
    ) -> io::Result<Self> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR is not set"))?;
        let socket = socket_path(&runtime, std::process::id());
        if let Some(dir) = socket.parent() {
            std::fs::create_dir_all(dir)?;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket)?;
        let socket_str = socket
            .to_str()
            .ok_or_else(|| io::Error::other("runtime folder is not UTF-8"))?
            .to_owned();

        let child = match launch {
            Launch::Spawn(exe) => Some(
                Command::new(exe)
                    .arg("--socket")
                    .arg(&socket_str)
                    .kill_on_drop(true)
                    .spawn()?,
            ),
            Launch::Gnome => {
                let bus =
                    bus.ok_or_else(|| io::Error::other("GNOME launch needs the session bus"))?;
                let shell = crate::gnome::ShellProxy::new(bus)
                    .await
                    .map_err(io::Error::other)?;
                shell
                    .spawn_surface(&socket_str)
                    .await
                    .map_err(io::Error::other)?;
                None
            }
        };

        let accepted = tokio::time::timeout(within, listener.accept()).await;
        let _ = std::fs::remove_file(&socket);
        let (stream, _) = accepted.map_err(|_| {
            io::Error::new(io::ErrorKind::TimedOut, "vutric-surface did not connect")
        })??;
        let (mut reader, mut writer) = stream.into_split();

        let (tx, mut outgoing) = mpsc::unbounded_channel::<ToSurface>();
        tokio::spawn(async move {
            while let Some(msg) = outgoing.recv().await {
                let Ok(bytes) = proto::encode(&msg) else {
                    continue;
                };
                if writer.write_all(&bytes).await.is_err() {
                    break;
                }
                if matches!(msg, ToSurface::Quit) {
                    break;
                }
            }
        });

        let (incoming, events) = mpsc::unbounded_channel::<FromSurface>();
        tokio::spawn(async move {
            let mut dec = Decoder::default();
            let mut buf = vec![0u8; READ_CHUNK];
            loop {
                let n = match reader.read(&mut buf).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => n,
                };
                dec.push(&buf[..n]);
                loop {
                    match dec.next_message::<FromSurface>() {
                        Ok(Some(msg)) => {
                            if incoming.send(msg).is_err() {
                                return;
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            let _ = incoming.send(FromSurface::Error {
                                message: e.to_string(),
                            });
                            return;
                        }
                    }
                }
            }
        });

        Ok(Self {
            tx,
            events,
            _child: child,
        })
    }

    /// Queues a message; `false` once the surface is gone.
    #[must_use = "false means the surface is gone and must be started again"]
    pub fn send(&self, msg: ToSurface) -> bool {
        self.tx.send(msg).is_ok()
    }

    /// The next message from the surface; `None` once it has disconnected.
    pub async fn next(&mut self) -> Option<FromSurface> {
        self.events.recv().await
    }

    /// A message if one is waiting.
    pub fn try_next(&mut self) -> Option<FromSurface> {
        self.events.try_recv().ok()
    }
}

impl Drop for SurfaceLink {
    fn drop(&mut self) {
        let _ = self.tx.send(ToSurface::Quit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_lives_in_the_runtime_folder() {
        assert_eq!(
            socket_path(Path::new("/run/user/1000"), 42),
            PathBuf::from("/run/user/1000/vutric/surface-42.sock")
        );
    }
}
