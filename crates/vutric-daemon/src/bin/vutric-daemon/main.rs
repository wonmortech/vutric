//! `vutric-daemon`: starts with the session, stays in the tray, receives, and plays effects on the overlay.
//!
//! ```text
//! vutric-daemon [--autostart] [--portable] [--lan-only] [--name "Ofis kompyuteri"]
//! vutric-daemon ctl '{"op":"status"}'
//! ```
//!
//! A second launch hands over to the running daemon (`Show`) and exits.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cardwin;
mod desk;
mod gpu;
mod net;
#[cfg(windows)]
mod tray;
#[cfg(not(windows))]
#[path = "tray_none.rs"]
mod tray;

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use tokio::sync::{broadcast, oneshot};
use vutric_core::Gesture;
use vutric_daemon::cards::{self, Deck, Question};
use vutric_daemon::i18n::{Lang, t, tf};
use vutric_daemon::ipc::{self, Event, Reply, Request, Status};
use vutric_daemon::prefs::{self, DesktopPrefs};
use vutric_daemon::{autostart, server};
use vutric_overlay::card::Button;
use vutric_overlay::{Halqa, HalqaConfig};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::net::{Cmd, Incoming, Shared};
use crate::tray::MenuId;

#[derive(Debug)]
pub enum UiEvent {
    Fling(Incoming),
    Offer {
        device: String,
        files: u32,
        bytes: u64,
        answer: oneshot::Sender<bool>,
    },
    Progress {
        done: u64,
        total: u64,
    },
    Received {
        device: String,
        files: Vec<PathBuf>,
        folder: PathBuf,
    },
    Failed,
    /// A button on the instant card.
    Card(Button),
    #[cfg_attr(not(windows), allow(dead_code, reason = "no tray menu before D10"))]
    Menu(MenuId),
    TrayOpen,
    ShowPairing,
    Preview,
    Paused(bool),
    /// A phone connected (`true`) or went away.
    Peer(bool),
    /// A yes/no question for the card; the network side waits on `answer`.
    Ask {
        q: Question,
        device: String,
        body: String,
        answer: oneshot::Sender<bool>,
    },
    /// A device was paired (QR or six digits).
    Paired {
        device: String,
    },
    /// A notice mirrored from a trusted phone, already cleaned (ADR-0065).
    Notice {
        device: String,
        app: String,
        title: String,
        text: String,
    },
    /// Text a trusted phone sent for this computer's clipboard, cleaned; copied only when the user says so.
    Clipboard {
        device: String,
        text: String,
    },
    /// A trusted phone rings this computer for `length` (ADR-0069).
    Ring {
        device: String,
        id: String,
        length: Duration,
    },
    /// The phone we rang was found (its `Ring::Stop`), or the phone that rang us gave up.
    Found {
        device: String,
    },
    /// "Telefonni topish" from the tray or the IPC: ring `device` (or every connected paired phone).
    FindPhone {
        device: Option<String>,
        secs: u16,
    },
    Quit,
}

struct Args {
    /// Never touch the registry or autostart entries (measurements, tests, a copy on a USB stick).
    portable: bool,
    lan_only: bool,
    name: String,
}

impl Args {
    fn parse() -> Self {
        let mut a = Self {
            portable: false,
            lan_only: false,
            name: default_name(),
        };
        let mut it = std::env::args().skip(1);
        while let Some(arg) = it.next() {
            match arg.as_str() {
                autostart::FLAG => {}
                "--portable" => a.portable = true,
                "--lan-only" => a.lan_only = true,
                "--name" => {
                    if let Some(n) = it.next() {
                        a.name = n;
                    }
                }
                _ => {}
            }
        }
        a
    }
}

fn default_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "Vutric".into())
}

fn user() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
}

fn lang() -> Lang {
    let locale = std::env::var("VUTRIC_LANG").unwrap_or_else(|_| os_locale());
    Lang::from_locale(&locale)
}

#[cfg(windows)]
fn os_locale() -> String {
    // Uzbek is the product default (CLAUDE.md); English only when the OS explicitly asks for it.
    let mut buf = [0u16; 85];
    // SAFETY: the buffer length is passed; the call writes at most that many UTF-16 units.
    let n = unsafe { windows_sys_locale(&mut buf) };
    let os = String::from_utf16_lossy(&buf[..n.saturating_sub(1)]);
    if os.to_ascii_lowercase().starts_with("en") {
        os
    } else {
        "uz".into()
    }
}

#[cfg(windows)]
unsafe fn windows_sys_locale(buf: &mut [u16; 85]) -> usize {
    use windows::Win32::Globalization::GetUserDefaultLocaleName;
    // SAFETY: forwarded from the caller.
    unsafe { GetUserDefaultLocaleName(buf).max(0) as usize }
}

#[cfg(not(windows))]
fn os_locale() -> String {
    let l = std::env::var("LANG").unwrap_or_default();
    if l.to_ascii_lowercase().starts_with("en") {
        l
    } else {
        "uz".into()
    }
}

/// How long the GPU stays warm after the last effect when no phone is connected: back-to-back flings stay
/// instant, an idle desktop gives the memory back.
const GPU_LINGER: Duration = Duration::from_secs(30);

/// Appends a line to the file named by `VUTRIC_TRACE` (measurements); a no-op otherwise.
fn trace(msg: &str) {
    use std::io::Write as _;
    if let Some(path) = std::env::var_os("VUTRIC_TRACE")
        && let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let _ = writeln!(f, "{ms} {msg}");
    }
}

fn config_dir() -> Result<PathBuf> {
    vutric_net::settings::desktop_dir()
}

/// The IPC handler: reads and writes the shared state, forwards UI work to the event loop.
struct Daemon {
    shared: Arc<Shared>,
    proxy: EventLoopProxy<UiEvent>,
    exe: PathBuf,
    /// `--portable`: the autostart choice is saved but never written to the registry.
    portable: bool,
}

impl server::Handler for Daemon {
    fn handle(&self, req: Request) -> Reply {
        let ok_or = |r: Result<()>| match r {
            Ok(()) => Reply::Ok,
            Err(e) => Reply::Error {
                message: format!("{e:#}"),
            },
        };
        match req {
            Request::Status => Reply::Status(Status {
                paused: self.shared.paused.load(Ordering::Relaxed),
                id: self.shared.id.get().cloned().unwrap_or_default(),
                name: self.shared.name.clone(),
                online: self.shared.online.load(Ordering::Relaxed),
                transfers: self.shared.transfers.load(Ordering::Relaxed),
                visibility: self.shared.visibility_name().to_owned(),
            }),
            Request::GetSettings => match config_dir() {
                Ok(dir) => Reply::Settings {
                    settings: Box::new(vutric_net::settings::load(
                        &dir.join(vutric_net::settings::FILE),
                    )),
                    prefs: prefs::load(&dir.join(prefs::FILE)),
                },
                Err(e) => Reply::Error {
                    message: format!("{e:#}"),
                },
            },
            Request::SetSettings { settings, prefs: p } => ok_or((|| {
                let dir = config_dir()?;
                let p = p.sanitized();
                vutric_net::settings::save(
                    &dir.join(vutric_net::settings::FILE),
                    &settings.migrate(),
                )?;
                prefs::save(&dir.join(prefs::FILE), &p)?;
                if !self.portable {
                    set_autostart(p.autostart, &self.exe)?;
                }
                Ok(())
            })()),
            Request::Pause { paused } => {
                let _ = self.proxy.send_event(UiEvent::Paused(paused));
                Reply::Ok
            }
            Request::Pairing => match self.shared.open_pairing() {
                Some(link) => Reply::Pairing { link },
                None => Reply::Error {
                    message: "network is still starting".into(),
                },
            },
            Request::Devices => Reply::Devices {
                devices: self.shared.devices(),
            },
            Request::Forget { id } => match self.shared.forget(&id) {
                Ok(true) => {
                    let _ = self.shared.events.send(Event::Devices);
                    Reply::Ok
                }
                Ok(false) => Reply::Error {
                    message: "no such paired device".into(),
                },
                Err(e) => Reply::Error {
                    message: format!("{e:#}"),
                },
            },
            Request::Ring { device, secs } => self.forward(UiEvent::FindPhone { device, secs }),
            Request::StopRing { device } => {
                let sent = match device {
                    Some(id) => usize::from(self.shared.to_device(&id, Cmd::StopRing)),
                    None => self.shared.to_trusted(&Cmd::StopRing),
                };
                sent_or_none(sent)
            }
            Request::SendText { text, device } => {
                let text = net::clip_bytes(&text, vutric_core::message::MAX_CLIPBOARD_BYTES);
                let sent = match device {
                    Some(id) => usize::from(self.shared.to_device(&id, Cmd::Clipboard(text))),
                    None => self.shared.to_trusted(&Cmd::Clipboard(text)),
                };
                sent_or_none(sent)
            }
            Request::SendFiles { .. } => Reply::Error {
                message: "sending files from the desktop is not wired yet".into(),
            },
            Request::OpenEveryone => {
                self.shared.open_everyone();
                Reply::Ok
            }
            Request::PreviewEffect => self.forward(UiEvent::Preview),
            Request::Show => self.forward(UiEvent::ShowPairing),
            Request::Quit => self.forward(UiEvent::Quit),
            Request::Hello { .. } | Request::Subscribe => Reply::Ok,
        }
    }

    fn events(&self) -> broadcast::Receiver<Event> {
        self.shared.events.subscribe()
    }
}

/// `Ok` when at least one device got the command.
fn sent_or_none(sent: usize) -> Reply {
    if sent > 0 {
        Reply::Ok
    } else {
        Reply::Error {
            message: "no connected paired device".into(),
        }
    }
}

impl Daemon {
    fn forward(&self, ev: UiEvent) -> Reply {
        match self.proxy.send_event(ev) {
            Ok(()) => Reply::Ok,
            Err(_) => Reply::Error {
                message: "daemon is closing".into(),
            },
        }
    }
}

fn set_autostart(on: bool, exe: &std::path::Path) -> Result<()> {
    autostart::set(on, exe).context("autostart")
}

struct Run {
    halqa: Halqa,
    started: Instant,
    notify: Option<oneshot::Sender<Instant>>,
}

struct App {
    shared: Arc<Shared>,
    proxy: EventLoopProxy<UiEvent>,
    lang: Lang,
    prefs: DesktopPrefs,
    tray: Option<tray::Tray>,
    gpu: Option<gpu::Gpu>,
    /// The GPU failed to start: retried on the next fling instead of giving up for the session (D9).
    gpu_failed: bool,
    /// Connected phones; the GPU stays warm while there is one.
    peers: u32,
    /// When the last effect ended; the GPU is released `GPU_LINGER` later if no phone is connected.
    gpu_idle_since: Option<Instant>,
    run: Option<Run>,
    pending: Option<Incoming>,
    sample: OnceLock<image::RgbaImage>,
    deck: Deck,
    /// Created with the first card; `None` until then or when the platform has no card window.
    cards: Option<cardwin::CardWin>,
    offer: Option<oneshot::Sender<bool>>,
    /// The network side waiting for the six-digit check.
    check: Option<oneshot::Sender<bool>>,
    /// The phone ringing us (endpoint id), told "found" when the user presses Topdim.
    ringing: Option<String>,
    /// Clipboard text waiting for the user's "Nusxalash".
    clip: Option<String>,
    /// When the ring sound stops by itself.
    ring_until: Option<Instant>,
}

impl App {
    fn ensure_gpu(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let Some(monitor) = gpu::pick_monitor(event_loop, self.prefs.monitor.as_deref()) else {
            return;
        };
        let img = self
            .sample
            .get_or_init(|| image::RgbaImage::from_pixel(4, 3, image::Rgba([40, 60, 140, 255])));
        let started = Instant::now();
        match gpu::Gpu::new(event_loop, &monitor, img) {
            Ok(g) => {
                trace(&format!(
                    "gpu ready in {:.1} ms",
                    started.elapsed().as_secs_f64() * 1000.0
                ));
                self.gpu_idle_since = Some(Instant::now());
                self.gpu = Some(g);
                self.gpu_failed = false;
            }
            Err(e) => {
                vutric_net::say(format!("overlay unavailable: {e:#}"));
                self.gpu_failed = true;
            }
        }
    }

    fn config(&self) -> HalqaConfig {
        let k = self.prefs.travel / HalqaConfig::default().travel;
        let d = HalqaConfig::default();
        HalqaConfig {
            travel: d.travel * k,
            hold: d.hold,
            dock: d.dock * k.sqrt(),
        }
    }

    fn play(&mut self, event_loop: &ActiveEventLoop, fling: Incoming) {
        self.ensure_gpu(event_loop);
        let cfg = self.config();
        let zone = self.prefs.zone;
        let Some(gpu) = &mut self.gpu else {
            self.pending = Some(fling);
            return;
        };
        gpu.set_image(&fling.image);
        let aspect = fling.image.width() as f32 / fling.image.height().max(1) as f32;
        self.run = Some(Run {
            halqa: Halqa::new(cfg, gpu.screen(), zone, fling.gesture, aspect),
            started: Instant::now(),
            notify: Some(fling.first_frame),
        });
        gpu.window.request_redraw();
    }

    fn draw(&mut self) {
        let (Some(gpu), Some(run)) = (&mut self.gpu, &mut self.run) else {
            return;
        };
        let t = run.started.elapsed().as_secs_f32();
        let done = run.halqa.is_finished(t);
        let frame = run.halqa.sample(t.min(run.halqa.duration()));
        match gpu.draw(&frame, t) {
            Ok(true) => {
                if let Some(tx) = run.notify.take() {
                    let _ = tx.send(Instant::now());
                }
            }
            Ok(false) => {}
            Err(e) => vutric_net::say(format!("overlay draw failed: {e:#}")),
        }
        if done {
            self.run = None;
            self.gpu_idle_since = Some(Instant::now());
            let _ = gpu.clear();
        } else {
            gpu.window.request_redraw();
        }
    }

    fn set_paused(&mut self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Relaxed);
        let _ = self.shared.events.send(Event::Paused { paused });
        if let Some(t) = &self.tray {
            t.set_paused(paused);
        }
    }

    fn open_settings() {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let app = exe.with_file_name(if cfg!(windows) {
            "vutric-app.exe"
        } else {
            "vutric-app"
        });
        if let Err(e) = std::process::Command::new(&app).spawn() {
            vutric_net::say(format!("settings window {}: {e}", app.display()));
        }
    }

    /// When the warm GPU may be dropped: no effect running, no phone connected, `GPU_LINGER` after the last use.
    fn gpu_release_at(&self) -> Option<Instant> {
        if self.gpu.is_none() || self.run.is_some() || self.peers > 0 || self.pending.is_some() {
            return None;
        }
        self.gpu_idle_since.map(|t| t + GPU_LINGER)
    }

    /// Redraws the instant card after the deck changed.
    fn sync_card(&mut self) {
        if self.cards.is_none() && self.deck.is_visible() {
            match cardwin::CardWin::new(self.proxy.clone()) {
                Ok(c) => self.cards = Some(c),
                Err(e) => vutric_net::say(format!("cards unavailable: {e}")),
            }
        }
        if let Some(c) = &mut self.cards {
            let took = c.sync(&self.deck);
            trace(&format!(
                "card shown in {:.2} ms",
                took.as_secs_f64() * 1000.0
            ));
        }
    }

    fn answer(&mut self, a: cards::Answer) {
        match a {
            cards::Answer::None => {}
            cards::Answer::Offer(yes) => {
                if let Some(tx) = self.offer.take() {
                    let _ = tx.send(yes);
                }
            }
            cards::Answer::PairClosed => self.shared.close_pairing(),
            cards::Answer::Ask(Question::Check, yes) => {
                if let Some(tx) = self.check.take() {
                    let _ = tx.send(yes);
                }
            }
            cards::Answer::Ask(Question::Ringing, yes) => {
                desk::ring(false);
                if let Some(id) = self.ringing.take()
                    && yes
                {
                    let _ = self.shared.to_device(&id, Cmd::StopRing);
                }
            }
            cards::Answer::Ask(Question::FindPhone, yes) => {
                if yes {
                    let _ = self.shared.to_trusted(&Cmd::StopRing);
                }
            }
            cards::Answer::Ask(Question::Clipboard, yes) => {
                if let Some(text) = self.clip.take()
                    && yes
                {
                    if desk::copy(&text) {
                        self.deck
                            .note(t(self.lang, "card.clip.copied"), "", Instant::now());
                    } else {
                        vutric_net::say("clipboard: could not copy".into());
                    }
                }
            }
        }
    }

    /// Puts `q` on the card; an older question or offer it replaces is answered "no".
    fn ask(&mut self, q: Question, device: &str, body: &str) {
        if let Some(old) = self.deck.ask(q, device, body, Instant::now()) {
            self.answer(old);
        }
        self.sync_card();
    }

    fn preview(&mut self, event_loop: &ActiveEventLoop) {
        let (tx, _) = oneshot::channel();
        let image = vutric_daemon_sample();
        self.play(
            event_loop,
            Incoming {
                image,
                gesture: Gesture {
                    speed: 2400.0,
                    angle: -0.2,
                    exit_x: 0.5,
                },
                arrived: Instant::now(),
                first_frame: tx,
            },
        );
    }
}

/// A warm dusk gradient, the card used when no real photo is at hand (settings preview).
fn vutric_daemon_sample() -> image::RgbaImage {
    image::RgbaImage::from_fn(480, 360, |x, y| {
        let (u, v) = (x as f32 / 480.0, y as f32 / 360.0);
        let lerp = |a: f32, b: f32, k: f32| (a + (b - a) * k) as u8;
        let k = (v * 1.1 - u * 0.2).clamp(0.0, 1.0);
        image::Rgba([
            lerp(255.0, 21.0, k),
            lerp(214.0, 22.0, k),
            lerp(150.0, 52.0, k),
            255,
        ])
    })
}

impl ApplicationHandler<UiEvent> for App {
    fn resumed(&mut self, _: &ActiveEventLoop) {
        if self.tray.is_none() {
            match tray::Tray::new(self.lang, &self.proxy) {
                Ok(t) => self.tray = Some(t),
                Err(e) => vutric_net::say(format!("tray unavailable: {e:#}")),
            }
        }
        // No GPU at rest: it costs ~120 MB of committed memory (measured, ADR-0031). It is warmed
        // when a phone connects, before its first fling arrives.
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, ev: UiEvent) {
        match ev {
            UiEvent::Fling(f) => self.play(event_loop, f),
            UiEvent::Offer {
                device,
                files,
                bytes,
                answer,
            } => {
                // A newer offer supersedes an unanswered one: that one is declined, never left hanging.
                if let Some(old) = self.offer.replace(answer) {
                    let _ = old.send(false);
                }
                self.deck.offer(&device, files, bytes, Instant::now());
                self.sync_card();
            }
            UiEvent::Progress { done, total } => {
                if self.deck.progress(done, total) {
                    self.sync_card();
                }
            }
            UiEvent::Failed => {
                self.answer(cards::Answer::Offer(false));
                self.deck.failed();
                self.sync_card();
            }
            UiEvent::Card(b) => {
                let a = self.deck.press(b);
                self.answer(a);
                self.sync_card();
            }
            UiEvent::Received {
                device,
                files,
                folder,
            } => {
                if files.is_empty() {
                    return;
                }
                self.deck.received(&device, files.len(), Instant::now());
                self.sync_card();
                #[cfg(windows)]
                {
                    let xml = vutric_daemon::toast::received_xml(
                        self.lang,
                        &vutric_daemon::toast::Received {
                            device: &device,
                            files: &files,
                            folder: &folder,
                        },
                    );
                    if let Err(e) = vutric_daemon::toast::show(&xml) {
                        vutric_net::say(format!("toast: {e}"));
                    }
                }
                #[cfg(not(windows))]
                let _ = (device, folder);
            }
            UiEvent::Menu(id) => match self.tray.as_ref().and_then(|t| t.action(&id)) {
                Some(tray::Action::Pair) => self.user_event(event_loop, UiEvent::ShowPairing),
                Some(tray::Action::FindPhone) => self.user_event(
                    event_loop,
                    UiEvent::FindPhone {
                        device: None,
                        secs: vutric_core::ring::DEFAULT_RING_SECS,
                    },
                ),
                Some(tray::Action::Everyone) => self.shared.open_everyone(),
                Some(tray::Action::Settings) => Self::open_settings(),
                Some(tray::Action::TogglePause) => {
                    let p = !self.shared.paused.load(Ordering::Relaxed);
                    self.set_paused(p);
                }
                Some(tray::Action::Quit) => event_loop.exit(),
                None => {}
            },
            UiEvent::TrayOpen => Self::open_settings(),
            UiEvent::ShowPairing => {
                if let Some(link) = self.shared.open_pairing() {
                    self.deck.pair(&link);
                    self.sync_card();
                }
            }
            UiEvent::Ask {
                q,
                device,
                body,
                answer,
            } => {
                if q == Question::Check
                    && let Some(old) = self.check.replace(answer)
                {
                    let _ = old.send(false);
                }
                self.ask(q, &device, &body);
            }
            UiEvent::Paired { device } => {
                let _ = self.shared.events.send(Event::Devices);
                self.deck
                    .note(t(self.lang, "card.paired.title"), &device, Instant::now());
                self.sync_card();
            }
            UiEvent::Notice {
                device,
                app,
                title,
                text,
            } => {
                let head = if app.is_empty() { device } else { app };
                let line = if title.is_empty() {
                    head
                } else {
                    format!("{head} · {title}")
                };
                self.deck.note(&line, &text, Instant::now());
                self.sync_card();
            }
            UiEvent::Clipboard { device, text } => {
                let preview: String = text.chars().take(80).collect();
                self.clip = Some(text);
                self.ask(Question::Clipboard, &device, &preview);
            }
            UiEvent::Ring { device, id, length } => {
                if self.shared.paused.load(Ordering::Relaxed) {
                    return;
                }
                desk::ring(true);
                self.ringing = Some(id);
                self.ask(Question::Ringing, &device, "");
                self.ring_until = Some(Instant::now() + length);
            }
            UiEvent::Found { device } => {
                if self.ringing.take().is_some() {
                    desk::ring(false);
                    self.deck.dismiss(Question::Ringing);
                } else {
                    self.deck.dismiss(Question::FindPhone);
                }
                self.deck
                    .note(t(self.lang, "card.found.title"), &device, Instant::now());
                self.sync_card();
            }
            UiEvent::FindPhone { device, secs } => {
                let cmd = Cmd::Ring(secs.clamp(1, vutric_core::ring::MAX_RING_SECS));
                let sent = match &device {
                    Some(id) => usize::from(self.shared.to_device(id, cmd)),
                    None => self.shared.to_trusted(&cmd),
                };
                if sent == 0 {
                    self.deck
                        .note(t(self.lang, "card.find.none"), "", Instant::now());
                    self.sync_card();
                } else {
                    let body = if sent > 1 {
                        tf(self.lang, "card.find.many", &[("count", &sent.to_string())])
                    } else {
                        String::new()
                    };
                    self.ask(Question::FindPhone, "", &body);
                }
            }
            UiEvent::Preview => self.preview(event_loop),
            UiEvent::Paused(p) => self.set_paused(p),
            UiEvent::Peer(up) => {
                if up {
                    self.peers += 1;
                    self.ensure_gpu(event_loop);
                } else {
                    self.peers = self.peers.saturating_sub(1);
                    self.gpu_idle_since = Some(Instant::now());
                }
            }
            UiEvent::Quit => event_loop.exit(),
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let WindowEvent::RedrawRequested = event {
            self.draw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(f) = self.pending.take() {
            if self.gpu.is_some() || !self.gpu_failed {
                self.play(event_loop, f);
            } else {
                self.pending = Some(f);
            }
        }
        let now = Instant::now();
        if self.ring_until.is_some_and(|at| now >= at) {
            self.ring_until = None;
            desk::ring(false);
        }
        if let Some(a) = self.deck.tick(now) {
            self.answer(a);
            self.sync_card();
        }
        if self.gpu_release_at().is_some_and(|at| now >= at) {
            self.gpu = None;
            self.gpu_idle_since = None;
            trace("gpu released");
        }
        let next = [self.deck.deadline(), self.gpu_release_at(), self.ring_until]
            .into_iter()
            .flatten()
            .min();
        event_loop.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}

/// `vutric-daemon ctl '{"op":"status"}'`: sends one request to the running daemon and prints the reply line.
fn ctl(json: &str, pipe: &str) -> Result<()> {
    use std::io::Write as _;
    // `ctl status` is short for `ctl '{"op":"status"}'`: no quoting games in PowerShell or cmd.
    let json = if json.trim_start().starts_with('{') {
        json.to_owned()
    } else {
        format!(r#"{{"op":"{json}"}}"#)
    };
    let req: Request = ipc::decode(json.as_bytes())?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let reply = rt
        .block_on(server::call(pipe, &req))
        .context("no daemon is running")?;
    std::io::stdout().write_all(&ipc::encode(&reply))?;
    Ok(())
}

fn main() -> Result<()> {
    let pipe = ipc::endpoint_name(&user());
    let mut raw = std::env::args().skip(1);
    if raw.next().as_deref() == Some("ctl") {
        return ctl(
            &raw.next().unwrap_or_else(|| r#"{"op":"status"}"#.into()),
            &pipe,
        );
    }
    let args = Args::parse();
    let probe = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    if probe.block_on(server::call(&pipe, &Request::Show)).is_ok() {
        return Ok(());
    }
    drop(probe);

    let lang = lang();
    #[cfg(windows)]
    let _ = vutric_daemon::toast::register("Vutric", None);
    let dir = config_dir()?;
    let prefs = prefs::load(&dir.join(prefs::FILE));
    let exe = std::env::current_exe()?;
    // Keeps the Run entry pointing at this exe after a move or an update.
    if !args.portable && prefs.autostart && !autostart::is_enabled(&exe) {
        let _ = set_autostart(true, &exe);
    }

    let event_loop = EventLoop::<UiEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let (events, _) = broadcast::channel(64);
    let shared = Arc::new(Shared::new(args.name.clone(), events));
    let rt = net::spawn(Arc::clone(&shared), args.lan_only, proxy.clone())?;
    let daemon = Arc::new(Daemon {
        shared: Arc::clone(&shared),
        proxy: proxy.clone(),
        exe,
        portable: args.portable,
    });
    rt.spawn(async move {
        if let Err(e) = server::listen(&pipe, daemon).await {
            vutric_net::say(format!("ipc stopped: {e:#}"));
        }
    });

    let mut app = App {
        shared,
        proxy,
        lang,
        prefs,
        tray: None,
        gpu: None,
        gpu_failed: false,
        peers: 0,
        gpu_idle_since: None,
        run: None,
        pending: None,
        sample: OnceLock::new(),
        deck: Deck::new(lang),
        cards: None,
        offer: None,
        check: None,
        ringing: None,
        clip: None,
        ring_until: None,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}
