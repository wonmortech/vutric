use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use objc2::MainThreadMarker;
use serde::Serialize;
use vutric_core::{EffectId, Gesture, SendZone};
use vutric_host_mac::link::{DisplayLink, Tick};
use vutric_host_mac::pacing::{self, Run, RunReport};
use vutric_host_mac::{OverlayLevel, OverlayPanel, ScreenInfo, WindowInfo};
use vutric_overlay::effects::{Sample, Setup};
use vutric_overlay::{Effects, Fling, HalqaConfig, Palette};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS as _};
use winit::window::{Window, WindowId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    /// Our own `NSPanel` (ADR-0110).
    Panel,
    /// winit's `NSWindow`, restyled by `make_overlay`: what the daemon gets without its own panel.
    Winit,
}

/// What paces the frames (ADR-0114).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Pacing {
    /// Draw as fast as the event loop polls; Fifo's drawable wait paces it (round 1).
    Fifo,
    /// Draw once per `CADisplayLink` callback, asking for 80–120 Hz.
    Link,
}

#[derive(Debug)]
struct Args {
    pacing: Pacing,
    runs: u32,
    level: OverlayLevel,
    mode: Mode,
    json: Option<std::path::PathBuf>,
    interval: f32,
    max_seconds: f32,
    /// Which effect each run plays (W5 effects E2-E4 next to W2's Halqa).
    effect: EffectId,
    /// Files per fling; more than one adds the stack (E5).
    files: u32,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut a = Self {
            pacing: Pacing::Fifo,
            runs: 5,
            level: OverlayLevel::default(),
            mode: Mode::Panel,
            json: None,
            interval: 0.6,
            max_seconds: 90.0,
            effect: EffectId::Halqa,
            files: 1,
        };
        let mut it = std::env::args().skip(1);
        while let Some(flag) = it.next() {
            let mut value = || it.next().with_context(|| format!("{flag} needs a value"));
            match flag.as_str() {
                "--runs" => a.runs = value()?.parse()?,
                "--level" => a.level = value()?.parse().map_err(anyhow::Error::msg)?,
                "--window" => {
                    a.mode = match value()?.as_str() {
                        "panel" => Mode::Panel,
                        "winit" => Mode::Winit,
                        other => bail!("--window panel|winit, not {other}"),
                    }
                }
                "--pacing" => {
                    a.pacing = match value()?.as_str() {
                        "fifo" => Pacing::Fifo,
                        "link" => Pacing::Link,
                        other => bail!("--pacing fifo|link, not {other}"),
                    }
                }
                "--json" => a.json = Some(value()?.into()),
                "--interval" => a.interval = value()?.parse()?,
                "--max-seconds" => a.max_seconds = value()?.parse()?,
                "--effect" => {
                    let key = value()?;
                    a.effect = EffectId::from_key(&key)
                        .with_context(|| format!("--effect halqa|kometa|pufak|oddiy, not {key}"))?;
                }
                "--files" => a.files = value()?.parse()?,
                other => bail!("unknown flag {other}"),
            }
        }
        Ok(a)
    }
}

#[derive(Debug, Default, Serialize)]
struct Report {
    schema: u32,
    tool: &'static str,
    os: String,
    arch: &'static str,
    mode: Option<Mode>,
    pacing: Option<Pacing>,
    effect: &'static str,
    files: u32,
    /// The system "Reduce motion" setting; a host would play `Fling::still` instead of `effect` when set.
    reduce_motion: bool,
    /// Median rate the display link delivered, from `1 / (targetTimestamp - timestamp)`.
    link_hz: Option<f64>,
    frontmost_before: Option<String>,
    screens: Vec<ScreenInfo>,
    refresh_hz: f64,
    window: Option<WindowInfo>,
    adapter: Option<String>,
    backend: Option<String>,
    alpha_modes: Vec<String>,
    alpha_mode: Option<String>,
    surface_px: [u32; 2],
    runs: Vec<RunReport>,
    passed: bool,
    error: Option<String>,
}

struct Gpu {
    // Field order is drop order: the surface goes before the window it draws into.
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Effects,
    panel: Option<OverlayPanel>,
    window: Option<Arc<Window>>,
}

type Surfaced = (
    wgpu::Surface<'static>,
    [u32; 2],
    Option<OverlayPanel>,
    Option<Arc<Window>>,
);

struct Active {
    fling: Fling,
    triggered: Instant,
    last: Option<Instant>,
    run: Run,
}

/// Shared with the display-link callback, which runs on the main thread between winit's callbacks.
#[derive(Default)]
struct Ticks {
    pending: bool,
    periods: Vec<f64>,
}

struct Probe {
    args: Args,
    link: Option<DisplayLink>,
    ticks: Rc<RefCell<Ticks>>,
    report: Report,
    gpu: Option<Gpu>,
    active: Option<Active>,
    done: u32,
    next: Instant,
    started: Instant,
    front_pid: i32,
}

fn sample_image(w: u32, h: u32) -> Vec<u8> {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let fx = x as f32 / w as f32;
            let fy = y as f32 / h as f32;
            px.extend_from_slice(&[
                (40.0 + 200.0 * fx) as u8,
                (60.0 + 120.0 * fy) as u8,
                (200.0 - 120.0 * fx * fy) as u8,
                255,
            ]);
        }
    }
    px
}

impl Probe {
    fn make_surface(
        &mut self,
        event_loop: &ActiveEventLoop,
        mtm: MainThreadMarker,
        screen: &objc2_app_kit::NSScreen,
        instance: &wgpu::Instance,
    ) -> Result<Surfaced> {
        Ok(match self.args.mode {
            Mode::Panel => {
                let panel = OverlayPanel::new(mtm, screen, self.args.level);
                let (display, win) = panel.raw_handles();
                // SAFETY: the handles point into `panel`, which `Gpu` keeps alive after the surface (field order).
                let surface = unsafe {
                    instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                        raw_display_handle: Some(display),
                        raw_window_handle: win,
                    })
                }?;
                self.report.window = Some(panel.info());
                (surface, panel.pixel_size(), Some(panel), None)
            }
            Mode::Winit => {
                use winit::dpi::{LogicalPosition, LogicalSize};
                let f = screen.frame();
                let attrs = Window::default_attributes()
                    .with_title("Vutric overlay")
                    .with_decorations(false)
                    .with_transparent(true)
                    .with_resizable(false)
                    .with_visible(false)
                    .with_active(false)
                    .with_position(LogicalPosition::new(0.0, 0.0))
                    .with_inner_size(LogicalSize::new(f.size.width, f.size.height));
                let window = Arc::new(event_loop.create_window(attrs)?);
                let view = ns_view(&window)?;
                // SAFETY: `view` belongs to `window`, alive here, and this is the main thread.
                let info = unsafe { vutric_host_mac::make_overlay(view, self.args.level) }
                    .map_err(anyhow::Error::msg)?;
                self.report.window = Some(info);
                let surface = instance.create_surface(window.clone())?;
                let s = window.inner_size();
                (surface, [s.width, s.height], None, Some(window))
            }
        })
    }

    fn create_gpu(&mut self, event_loop: &ActiveEventLoop) -> Result<Gpu> {
        let mtm = MainThreadMarker::new().context("not on the main thread")?;
        let screen = vutric_host_mac::main_screen(mtm).context("no screen")?;
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(desc);

        let (surface, size, panel, window) =
            self.make_surface(event_loop, mtm, &screen, &instance)?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        }))
        .context("no Metal adapter")?;
        let info = adapter.get_info();
        self.report.adapter = Some(format!("{} ({:?})", info.name, info.device_type));
        self.report.backend = Some(format!("{:?}", info.backend));
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("vutric-mac-probe"),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                ..Default::default()
            }))?;
        let caps = surface.get_capabilities(&adapter);
        self.report.alpha_modes = caps.alpha_modes.iter().map(|m| format!("{m:?}")).collect();
        let Some(alpha_mode) = [
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::PostMultiplied,
        ]
        .into_iter()
        .find(|m| caps.alpha_modes.contains(m)) else {
            bail!(
                "no transparent composite alpha mode: {:?}",
                caps.alpha_modes
            );
        };
        self.report.alpha_mode = Some(format!("{alpha_mode:?}"));
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size[0].max(1),
            height: size[1].max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        self.report.surface_px = [config.width, config.height];
        let mut renderer = Effects::new(&device, &queue, config.format, Palette::vutric());
        renderer.set_image(&device, &queue, &sample_image(480, 360), 480, 360);

        let gpu = Gpu {
            surface,
            device,
            queue,
            config,
            renderer,
            panel,
            window,
        };
        gpu.present(None, 0.0)?;
        if let Some(p) = &gpu.panel {
            p.show();
        }
        if let Some(w) = &gpu.window {
            // SAFETY: as above.
            unsafe { vutric_host_mac::show_without_activating(ns_view(w)?) };
        }
        Ok(gpu)
    }

    fn start(&mut self) {
        let Some(gpu) = &self.gpu else { return };
        let screen = [gpu.config.width as f32, gpu.config.height as f32];
        let gesture = Gesture {
            speed: 2400.0,
            angle: 0.0,
            exit_x: 0.35 + 0.15 * (self.done % 3) as f32,
        };
        self.active = Some(Active {
            fling: Fling::new(
                self.args.effect,
                Setup {
                    cfg: HalqaConfig::default(),
                    screen,
                    zone: SendZone::Right,
                    gesture,
                    image_aspect: 4.0 / 3.0,
                },
            )
            .with_files(self.args.files),
            triggered: Instant::now(),
            last: None,
            run: Run::new(self.report.refresh_hz),
        });
        if let Some(l) = &self.link {
            self.ticks.borrow_mut().pending = true;
            l.set_running(true);
        }
    }

    fn make_link(&mut self) -> Result<()> {
        let mtm = MainThreadMarker::new().context("not on the main thread")?;
        let gpu = self.gpu.as_ref().context("no GPU")?;
        let view = match (&gpu.panel, &gpu.window) {
            (Some(p), _) => p.ns_view(),
            (None, Some(w)) => ns_view(w)?,
            (None, None) => bail!("no view"),
        };
        let ticks = self.ticks.clone();
        // SAFETY: the view belongs to the panel or window in `gpu`; the link is dropped before it (see `run`).
        let link = unsafe {
            DisplayLink::new(mtm, view, move |t: Tick| {
                let mut s = ticks.borrow_mut();
                s.pending = true;
                if t.target > t.timestamp {
                    s.periods.push(t.target - t.timestamp);
                }
            })
        }
        .context("CADisplayLink needs macOS 14")?;
        self.link = Some(link);
        Ok(())
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let (Some(gpu), Some(a)) = (&mut self.gpu, &mut self.active) else {
            return Ok(());
        };
        let t = a.triggered.elapsed().as_secs_f32();
        let finished = a.fling.is_finished(t);
        let frame = a.fling.sample(t.min(a.fling.duration()));
        if !gpu.present(Some(&frame), t)? {
            return Ok(());
        }
        let now = Instant::now();
        match a.last {
            None => a.run.first_frame(now - a.triggered),
            Some(prev) => a.run.interval(now - prev),
        }
        a.last = Some(now);
        let mtm = MainThreadMarker::new().context("not on the main thread")?;
        let key = match (&gpu.panel, &gpu.window) {
            (Some(p), _) => p.is_key(),
            (None, Some(w)) => w.has_focus(),
            (None, None) => false,
        };
        let front = vutric_host_mac::frontmost_pid();
        a.run.focus(
            front == std::process::id().cast_signed(),
            front == self.front_pid,
            vutric_host_mac::app_is_active(mtm),
            key,
        );
        if finished {
            gpu.present(None, t)?;
            if let Some(l) = &self.link {
                l.set_running(false);
            }
            let done = self.active.take().expect("active run");
            self.report.runs.push(done.run.report());
            self.done += 1;
            self.next = Instant::now() + Duration::from_secs_f32(self.args.interval);
            if self.done >= self.args.runs {
                event_loop.exit();
            }
        }
        Ok(())
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, e: &anyhow::Error) {
        self.report.error = Some(format!("{e:#}"));
        event_loop.exit();
    }
}

impl Gpu {
    /// Draws one frame (or a clear one with `None`). `Ok(false)` = no drawable this time; try again.
    fn present(&self, frame: Option<&Sample>, t: f32) -> Result<bool> {
        let texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(tex)
            | wgpu::CurrentSurfaceTexture::Suboptimal(tex) => tex,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Validation => bail!("surface validation error"),
        };
        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        if let Some(frame) = frame {
            self.renderer.encode(
                &self.queue,
                &mut encoder,
                &view,
                [self.config.width, self.config.height],
                frame,
                t,
            );
        } else {
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(texture);
        Ok(true)
    }
}

fn ns_view(window: &Window) -> Result<std::ptr::NonNull<core::ffi::c_void>> {
    use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    match window.window_handle()?.as_raw() {
        RawWindowHandle::AppKit(h) => Ok(h.ns_view),
        other => bail!("not an AppKit window: {other:?}"),
    }
}

impl ApplicationHandler for Probe {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if self.started.elapsed().as_secs_f32() > self.args.max_seconds {
            self.fail(
                event_loop,
                &anyhow::anyhow!("timed out after {} s", self.args.max_seconds),
            );
            return;
        }
        if matches!(cause, StartCause::ResumeTimeReached { .. }) && self.active.is_none() {
            self.start();
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        match self.create_gpu(event_loop) {
            Ok(gpu) => {
                self.gpu = Some(gpu);
                if self.args.pacing == Pacing::Link
                    && let Err(e) = self.make_link()
                {
                    self.fail(event_loop, &e);
                    return;
                }
                self.next = Instant::now() + Duration::from_millis(900);
            }
            Err(e) => self.fail(event_loop, &e),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::CloseRequested) {
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let due = match self.args.pacing {
            Pacing::Fifo => true,
            Pacing::Link => std::mem::take(&mut self.ticks.borrow_mut().pending),
        };
        if self.active.is_some()
            && due
            && let Err(e) = self.frame(event_loop)
        {
            self.fail(event_loop, &e);
            return;
        }
        if self.active.is_some() && self.args.pacing == Pacing::Link {
            // The display link wakes the run loop once per refresh; winit then calls us again.
            event_loop.set_control_flow(ControlFlow::Wait);
        } else if self.active.is_some() {
            // Fifo: acquiring the next drawable waits for the display, so polling is paced by the screen.
            event_loop.set_control_flow(ControlFlow::Poll);
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next));
        }
    }
}

pub fn main() -> ExitCode {
    let args = match Args::parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e:#}");
            return ExitCode::from(2);
        }
    };
    let json = args.json.clone();
    watchdog(
        json.clone(),
        args.mode,
        args.pacing,
        args.max_seconds + 15.0,
    );
    let report = run(args);
    let text = serde_json::to_string_pretty(&report).expect("report serialises");
    println!("{text}");
    if let Some(path) = json
        && let Err(e) = std::fs::write(&path, &text)
    {
        eprintln!("write {}: {e}", path.display());
    }
    if report.error.is_some() || report.runs.is_empty() {
        ExitCode::from(2)
    } else if report.passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// A blocked drawable acquisition never returns to the event loop, so the in-loop timeout cannot fire. This thread
/// writes an error report and ends the process instead, so CI always gets its JSON.
fn watchdog(json: Option<std::path::PathBuf>, mode: Mode, pacing: Pacing, after: f32) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs_f32(after));
        let report = Report {
            schema: 1,
            tool: "vutric-mac-probe",
            arch: std::env::consts::ARCH,
            mode: Some(mode),
            pacing: Some(pacing),
            error: Some(format!(
                "hung: no exit after {after} s (blocked in the event loop or the GPU)"
            )),
            ..Default::default()
        };
        let text = serde_json::to_string_pretty(&report).expect("report serialises");
        println!("{text}");
        if let Some(path) = json {
            let _ = std::fs::write(path, text);
        }
        std::process::exit(2);
    });
}

fn run(args: Args) -> Report {
    let mut report = Report {
        schema: 1,
        tool: "vutric-mac-probe",
        arch: std::env::consts::ARCH,
        mode: Some(args.mode),
        pacing: Some(args.pacing),
        effect: args.effect.key(),
        files: args.files,
        ..Default::default()
    };
    report.os = vutric_host_mac::os_version();
    let mut builder = EventLoop::builder();
    builder
        .with_activation_policy(ActivationPolicy::Accessory)
        .with_default_menu(false)
        .with_activate_ignoring_other_apps(false);
    let event_loop = match builder.build() {
        Ok(l) => l,
        Err(e) => {
            report.error = Some(format!("event loop: {e}"));
            return report;
        }
    };
    let Some(mtm) = MainThreadMarker::new() else {
        report.error = Some("not on the main thread".into());
        return report;
    };
    report.screens = vutric_host_mac::screens(mtm);
    report.refresh_hz = vutric_host_mac::main_screen(mtm)
        .map_or(60, |s| s.maximumFramesPerSecond())
        .max(1) as f64;
    report.frontmost_before = vutric_host_mac::frontmost_name();
    report.reduce_motion = vutric_host_mac::reduce_motion();
    let front_pid = vutric_host_mac::frontmost_pid();
    let now = Instant::now();
    let mut probe = Probe {
        args,
        link: None,
        ticks: Rc::default(),
        report,
        gpu: None,
        active: None,
        done: 0,
        next: now + Duration::from_secs(3600),
        started: now,
        front_pid,
    };
    if let Err(e) = event_loop.run_app(&mut probe) {
        probe.report.error.get_or_insert(format!("event loop: {e}"));
    }
    drop(probe.link.take());
    drop(probe.gpu.take());
    let mut periods = std::mem::take(&mut probe.ticks.borrow_mut().periods);
    if !periods.is_empty() {
        periods.sort_by(f64::total_cmp);
        probe.report.link_hz = Some(1.0 / periods[periods.len() / 2]);
    }
    let hz = probe.report.refresh_hz;
    probe.report.passed = probe.report.error.is_none()
        && !probe.report.runs.is_empty()
        && probe.report.runs.iter().all(|r| pacing::passes(r, hz));
    probe.report
}
