//! Phase 0.1 spike: plays the Halqa effect over the real desktop and reports frame pacing, first-frame latency
//! and whether keyboard focus was ever taken.
//!
//! ```text
//! cargo run -p vutric-overlay-spike --release -- --runs 5 --zone right --image photo.jpg
//! ```
//!
//! Keep typing in another window while it runs: the report fails the focus check if a single keystroke would
//! have been lost.
//!
//! `--listen` turns it into the Phase 0 end-to-end check: it waits for a phone on the LAN and plays the effect
//! with every photo flung from it (see `listen.rs`).

mod args;
mod listen;
mod sample_image;
mod stats;

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use vutric_core::Gesture;
use vutric_overlay::{Halqa, HalqaConfig, Palette, Renderer};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId, WindowLevel};

use crate::args::Args;
use crate::stats::{RunStats, Summary};

fn main() -> Result<()> {
    let args = Args::parse(std::env::args().skip(1))?;
    let image = match &args.image {
        Some(path) => {
            let img = image::open(path).with_context(|| format!("open {}", path.display()))?;
            img.thumbnail(1024, 1024).to_rgba8()
        }
        None => sample_image::dusk(960, 720),
    };

    let event_loop = EventLoop::<listen::Incoming>::with_user_event().build()?;
    if args.listen {
        listen::spawn(args.name.clone(), event_loop.create_proxy())?;
    }
    let mut app = Spike::new(args, image);
    event_loop.run_app(&mut app)?;
    if let Some(err) = app.fatal.take() {
        return Err(err);
    }
    app.summary.print();
    if app.summary.passed() {
        Ok(())
    } else {
        bail!("exit criteria not met")
    }
}

struct Gpu {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    hwnd: isize,
}

struct Run {
    halqa: Halqa,
    triggered: Instant,
    started: Instant,
    last_present: Option<Instant>,
    stats: RunStats,
    /// `--listen`: told when the first frame of this fling is on screen.
    notify: Option<tokio::sync::oneshot::Sender<Instant>>,
}

struct Spike {
    args: Args,
    image: image::RgbaImage,
    gpu: Option<Gpu>,
    run: Option<Run>,
    runs_done: u32,
    next_trigger: Instant,
    summary: Summary,
    fatal: Option<anyhow::Error>,
}

impl Spike {
    fn new(args: Args, image: image::RgbaImage) -> Self {
        Self {
            args,
            image,
            gpu: None,
            run: None,
            runs_done: 0,
            next_trigger: Instant::now() + Duration::from_millis(900),
            summary: Summary::default(),
            fatal: None,
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, err: anyhow::Error) {
        self.fatal = Some(err);
        event_loop.exit();
    }

    fn scripted_gesture(&self) -> Gesture {
        Gesture {
            speed: self.args.speed,
            angle: self.args.angle,
            exit_x: 0.35 + 0.3 * (self.runs_done % 3) as f32 / 2.0,
        }
    }

    fn start_run(
        &mut self,
        gesture: Gesture,
        triggered: Instant,
        notify: Option<tokio::sync::oneshot::Sender<Instant>>,
    ) {
        let Some(gpu) = &self.gpu else { return };
        let screen = [gpu.config.width as f32, gpu.config.height as f32];
        let aspect = self.image.width() as f32 / self.image.height() as f32;
        let now = Instant::now();
        self.run = Some(Run {
            halqa: Halqa::new(
                HalqaConfig::default(),
                screen,
                self.args.zone,
                gesture,
                aspect,
            ),
            triggered,
            started: now,
            last_present: None,
            stats: RunStats::new(platform_foreground()),
            notify,
        });
        gpu.window.request_redraw();
    }

    fn draw(&mut self, event_loop: &ActiveEventLoop) {
        let (Some(gpu), Some(run)) = (&mut self.gpu, &mut self.run) else {
            return;
        };
        let t = run.started.elapsed().as_secs_f32();
        let finished = run.halqa.is_finished(t);
        let frame = run.halqa.sample(t.min(run.halqa.duration()));

        let texture = match gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(tex)
            | wgpu::CurrentSurfaceTexture::Suboptimal(tex) => tex,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                gpu.surface.configure(&gpu.device, &gpu.config);
                gpu.window.request_redraw();
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                gpu.window.request_redraw();
                return;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                self.fatal = Some(anyhow::anyhow!("surface validation error"));
                event_loop.exit();
                return;
            }
        };
        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        gpu.renderer.encode(
            &gpu.queue,
            &mut encoder,
            &view,
            [gpu.config.width, gpu.config.height],
            &frame,
            t,
        );
        gpu.queue.submit([encoder.finish()]);
        gpu.queue.present(texture);

        let now = Instant::now();
        match run.last_present {
            None => {
                run.stats.first_frame = Some(now - run.triggered);
                if let Some(tx) = run.notify.take() {
                    let _ = tx.send(now);
                }
            }
            Some(prev) => run.stats.intervals.push(now - prev),
        }
        run.last_present = Some(now);
        let foreground = platform_foreground();
        run.stats.focus_ok &= foreground != gpu.hwnd;

        if finished {
            run.stats.foreground_changed = foreground != run.stats.foreground_before;
            let done = self.run.take().expect("run present");
            self.runs_done += 1;
            if !self.args.listen {
                self.next_trigger = Instant::now() + Duration::from_secs_f32(self.args.interval);
                if self.runs_done >= self.args.runs {
                    event_loop.exit();
                }
            }
            self.summary.runs.push(done.stats);
        } else {
            gpu.window.request_redraw();
        }
    }
}

impl ApplicationHandler<listen::Incoming> for Spike {
    fn new_events(&mut self, _: &ActiveEventLoop, cause: StartCause) {
        if !self.args.listen
            && matches!(cause, StartCause::ResumeTimeReached { .. })
            && self.run.is_none()
            && self.gpu.is_some()
        {
            self.start_run(self.scripted_gesture(), Instant::now(), None);
        }
    }

    /// A photo flung from a phone (`--listen`): swap the texture and play the effect at once.
    fn user_event(&mut self, _: &ActiveEventLoop, fling: listen::Incoming) {
        let Some(gpu) = &mut self.gpu else { return };
        gpu.renderer.set_image(
            &gpu.device,
            &gpu.queue,
            fling.image.as_raw(),
            fling.image.width(),
            fling.image.height(),
        );
        self.image = fling.image;
        if let Some(mut previous) = self.run.take()
            && let Some(tx) = previous.notify.take()
        {
            let _ = tx.send(Instant::now());
        }
        self.start_run(fling.gesture, fling.arrived, Some(fling.first_frame));
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        match create_gpu(event_loop, &self.image, self.args.frame_latency) {
            Ok(gpu) => {
                self.summary.adapter = gpu_label(&gpu);
                self.gpu = Some(gpu);
            }
            Err(err) => self.fail(event_loop, err),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::RedrawRequested => self.draw(event_loop),
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.run.is_some() || self.args.listen {
            event_loop.set_control_flow(ControlFlow::Wait);
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_trigger));
        }
    }
}

/// A borderless, hidden, full-monitor window already styled as an overlay (not yet shown).
fn create_overlay_window(event_loop: &ActiveEventLoop) -> Result<(Arc<Window>, isize)> {
    let monitor = event_loop
        .primary_monitor()
        .or_else(|| event_loop.available_monitors().next())
        .context("no monitor")?;
    let size: PhysicalSize<u32> = monitor.size();
    let pos: PhysicalPosition<i32> = monitor.position();

    let attrs = Window::default_attributes()
        .with_title("Vutric overlay")
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_visible(false)
        .with_active(false)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_position(pos)
        .with_inner_size(size);
    #[cfg(windows)]
    let attrs = {
        use winit::platform::windows::WindowAttributesExtWindows as _;
        attrs
            .with_skip_taskbar(true)
            .with_no_redirection_bitmap(true)
            .with_undecorated_shadow(false)
    };
    let window = Arc::new(event_loop.create_window(attrs)?);
    let hwnd = raw_hwnd(&window)?;
    #[cfg(windows)]
    vutric_host_win::make_overlay(hwnd).map_err(|e| anyhow::anyhow!("overlay styles: {e}"))?;
    Ok((window, hwnd))
}

/// Picks a transparent composite mode and an sRGB format. Refuses to run without transparency: an opaque
/// full-screen topmost window would black out the desktop.
fn surface_config(
    caps: &wgpu::SurfaceCapabilities,
    size: PhysicalSize<u32>,
    frame_latency: u32,
) -> Result<wgpu::SurfaceConfiguration> {
    let Some(alpha_mode) = [
        wgpu::CompositeAlphaMode::PreMultiplied,
        wgpu::CompositeAlphaMode::PostMultiplied,
    ]
    .into_iter()
    .find(|m| caps.alpha_modes.contains(m)) else {
        bail!(
            "adapter offers no transparent composition (alpha modes: {:?})",
            caps.alpha_modes
        );
    };
    let format = caps
        .formats
        .iter()
        .copied()
        .find(wgpu::TextureFormat::is_srgb)
        .unwrap_or(caps.formats[0]);
    Ok(wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: size.width.max(1),
        height: size.height.max(1),
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: frame_latency.clamp(1, 3),
        alpha_mode,
        color_space: wgpu::SurfaceColorSpace::Auto,
        view_formats: vec![],
    })
}

fn create_gpu(
    event_loop: &ActiveEventLoop,
    image: &image::RgbaImage,
    frame_latency: u32,
) -> Result<Gpu> {
    let (window, hwnd) = create_overlay_window(event_loop)?;

    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    if cfg!(windows) {
        desc.backends = wgpu::Backends::DX12;
        desc.backend_options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
    }
    let instance = wgpu::Instance::new(desc);
    let surface = instance.create_surface(window.clone())?;
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        compatible_surface: Some(&surface),
        apply_limit_buckets: false,
    }))?;
    let info = adapter.get_info();
    println!(
        "adapter: {} ({:?}, {:?}, driver {})",
        info.name, info.backend, info.device_type, info.driver_info
    );
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("vutric"),
        ..Default::default()
    }))?;

    let config = surface_config(
        &surface.get_capabilities(&adapter),
        window.inner_size(),
        frame_latency,
    )?;
    surface.configure(&device, &config);

    let mut renderer = Renderer::new(&device, &queue, config.format, Palette::vutric());
    renderer.set_image(
        &device,
        &queue,
        image.as_raw(),
        image.width(),
        image.height(),
    );

    let gpu = Gpu {
        window,
        surface,
        device,
        queue,
        config,
        renderer,
        hwnd,
    };
    present_clear(&gpu)?;
    #[cfg(windows)]
    vutric_host_win::show_without_activating(hwnd);
    #[cfg(not(windows))]
    gpu.window.set_visible(true);
    Ok(gpu)
}

/// Presents one fully transparent frame so the window never shows uninitialised pixels.
fn present_clear(gpu: &Gpu) -> Result<()> {
    let wgpu::CurrentSurfaceTexture::Success(texture) = gpu.surface.get_current_texture() else {
        bail!("could not acquire the first surface texture");
    };
    let view = texture
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
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
    gpu.queue.submit([encoder.finish()]);
    gpu.queue.present(texture);
    Ok(())
}

fn gpu_label(gpu: &Gpu) -> String {
    format!(
        "{}x{} {:?} {:?} latency={}",
        gpu.config.width,
        gpu.config.height,
        gpu.config.format,
        gpu.config.alpha_mode,
        gpu.config.desired_maximum_frame_latency
    )
}

fn raw_hwnd(window: &Window) -> Result<isize> {
    use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    match window.window_handle()?.as_raw() {
        RawWindowHandle::Win32(h) => Ok(h.hwnd.get()),
        _ => Ok(0),
    }
}

fn platform_foreground() -> isize {
    #[cfg(windows)]
    {
        vutric_host_win::foreground_window()
    }
    #[cfg(not(windows))]
    {
        0
    }
}
