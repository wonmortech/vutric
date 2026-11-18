//! Drives `vutric-surface` over the real socket protocol, the way the daemon will (ADR-0090 §3). CI runs it under
//! headless sway (the driver spawns the surface) and headless GNOME Shell (the extension spawns it).
//!
//! ```text
//! surface_driver --spawn target/release/vutric-surface --runs 3 --out drive.json
//! surface_driver --gnome --runs 3 --out drive.json
//! ```

#[cfg(target_os = "linux")]
mod imp {
    use std::io::{Read as _, Write as _};
    use std::os::unix::fs::PermissionsExt as _;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use vutric_core::{Gesture, SendZone};
    use vutric_host_linux::proto::{self, Decoder, FromSurface, Image, ToSurface};

    #[derive(serde::Serialize, Default)]
    struct Report {
        mode: String,
        extension: Option<String>,
        ready: Option<String>,
        warmed: Option<String>,
        first_frame_ms: Vec<f32>,
        played: Vec<proto::FrameStats>,
        card: Option<String>,
        errors: Vec<String>,
        total_ms: f32,
    }

    fn recv(s: &mut UnixStream, dec: &mut Decoder) -> Result<FromSurface, String> {
        let mut buf = [0u8; 4096];
        loop {
            if let Some(m) = dec
                .next_message::<FromSurface>()
                .map_err(|e| e.to_string())?
            {
                return Ok(m);
            }
            let n = s.read(&mut buf).map_err(|e| format!("read: {e}"))?;
            if n == 0 {
                return Err("surface closed the socket".into());
            }
            dec.push(&buf[..n]);
        }
    }

    fn send(s: &mut UnixStream, m: &ToSurface) -> Result<(), String> {
        s.write_all(&proto::encode(m).map_err(|e| e.to_string())?)
            .map_err(|e| format!("write: {e}"))
    }

    fn image() -> Image {
        let (width, height) = (320u32, 200u32);
        let rgba = (0..width * height)
            .flat_map(|i| [(i % 251) as u8, (i % 241) as u8, 180, 255])
            .collect();
        Image {
            width,
            height,
            rgba,
        }
    }

    fn accept(l: &UnixListener, within: Duration) -> Result<UnixStream, String> {
        l.set_nonblocking(true).map_err(|e| e.to_string())?;
        let until = Instant::now() + within;
        loop {
            match l.accept() {
                Ok((s, _)) => {
                    s.set_nonblocking(false).map_err(|e| e.to_string())?;
                    return Ok(s);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > until {
                        return Err("the surface never connected".into());
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    async fn gnome(socket: &str, report: &mut Report) -> Result<(), String> {
        use vutric_host_linux::gnome::ShellProxy;
        let conn = zbus::Connection::session()
            .await
            .map_err(|e| e.to_string())?;
        let shell = ShellProxy::new(&conn).await.map_err(|e| e.to_string())?;
        let (api, version) = shell.ping().await.map_err(|e| format!("Ping: {e}"))?;
        report.extension = Some(format!("api {api}, GNOME Shell {version}"));
        let (w, h) = (400u32, 176u32);
        let bgra: Vec<u8> = (0..w * h).flat_map(|_| [40u8, 30, 24, 240]).collect();
        let hits = vec![("accept".to_owned(), 200.0, 120.0, 400.0, 176.0)];
        let t = Instant::now();
        shell
            .show_card(w, h, 1, &bgra, &hits)
            .await
            .map_err(|e| format!("ShowCard: {e}"))?;
        let shown = t.elapsed();
        shell
            .hide_card()
            .await
            .map_err(|e| format!("HideCard: {e}"))?;
        shell
            .set_status(false, "uz")
            .await
            .map_err(|e| format!("SetStatus: {e}"))?;
        report.card = Some(format!(
            "ShowCard round trip {:.2} ms",
            shown.as_secs_f64() * 1000.0
        ));
        let pid = shell
            .spawn_surface(socket)
            .await
            .map_err(|e| format!("SpawnSurface: {e}"))?;
        println!("extension spawned vutric-surface, pid {pid}");
        Ok(())
    }

    #[allow(clippy::too_many_lines, reason = "a linear test script")]
    pub fn main() -> std::process::ExitCode {
        let mut spawn: Option<String> = None;
        let mut use_gnome = false;
        let mut runs = 3u32;
        let mut out: Option<String> = None;
        let mut it = std::env::args().skip(1);
        while let Some(a) = it.next() {
            match a.as_str() {
                "--spawn" => spawn = it.next(),
                "--gnome" => use_gnome = true,
                "--runs" => runs = it.next().and_then(|v| v.parse().ok()).unwrap_or(3),
                "--out" => out = it.next(),
                _ => {}
            }
        }
        let started = Instant::now();
        let mut report = Report {
            mode: if use_gnome { "gnome" } else { "spawn" }.into(),
            ..Report::default()
        };
        let result = (|| -> Result<(), String> {
            let rt = std::env::var("XDG_RUNTIME_DIR").map_err(|_| "XDG_RUNTIME_DIR unset")?;
            let dir = PathBuf::from(rt).join("vutric");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
            let path = dir.join(format!("surface-{}.sock", std::process::id()));
            let _ = std::fs::remove_file(&path);
            let listener = UnixListener::bind(&path).map_err(|e| e.to_string())?;
            let socket = path.to_string_lossy().into_owned();
            let mut child = None;
            if use_gnome {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| e.to_string())?
                    .block_on(gnome(&socket, &mut report))?;
            } else {
                let exe = spawn.clone().ok_or("--spawn PATH or --gnome")?;
                child = Some(
                    std::process::Command::new(exe)
                        .args(["--socket", &socket])
                        .spawn()
                        .map_err(|e| e.to_string())?,
                );
            }
            let mut s = accept(&listener, Duration::from_secs(20))?;
            s.set_read_timeout(Some(Duration::from_secs(30)))
                .map_err(|e| e.to_string())?;
            let mut dec = Decoder::default();
            report.ready = Some(format!("{:?}", recv(&mut s, &mut dec)?));
            send(&mut s, &ToSurface::Warm)?;
            report.warmed = Some(format!("{:?}", recv(&mut s, &mut dec)?));
            for id in 0..u64::from(runs) {
                send(
                    &mut s,
                    &ToSurface::Play {
                        id,
                        zone: SendZone::Right,
                        gesture: Gesture {
                            speed: 2400.0,
                            angle: 0.0,
                            exit_x: 0.5,
                        },
                        image: image(),
                    },
                )?;
                loop {
                    match recv(&mut s, &mut dec)? {
                        FromSurface::FirstFrame { after_ms, .. } => {
                            report.first_frame_ms.push(after_ms);
                        }
                        FromSurface::Played { stats, .. } => {
                            report.played.push(stats);
                            break;
                        }
                        FromSurface::Error { message } => {
                            report.errors.push(message);
                            break;
                        }
                        _ => {}
                    }
                }
            }
            send(&mut s, &ToSurface::Quit)?;
            if let Some(mut c) = child {
                let _ = c.wait();
            }
            let _ = std::fs::remove_file(&path);
            Ok(())
        })();
        if let Err(e) = result {
            report.errors.push(e);
        }
        report.total_ms = started.elapsed().as_secs_f32() * 1000.0;
        let json = serde_json::to_string_pretty(&report).unwrap_or_default();
        println!("{json}");
        if let Some(p) = out {
            let _ = std::fs::write(p, &json);
        }
        if report.errors.is_empty() && report.played.len() == runs as usize {
            std::process::ExitCode::SUCCESS
        } else {
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    imp::main()
}

#[cfg(not(target_os = "linux"))]
fn main() {}
