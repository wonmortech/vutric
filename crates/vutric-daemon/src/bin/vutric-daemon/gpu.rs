//! The overlay surface: one borderless, transparent, topmost, click-through, never-activating window per chosen
//! monitor, created at start and kept warm (ADR-0004). The GPU only works while something animates.

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use vutric_overlay::{Frame, Palette, Renderer};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::monitor::MonitorHandle;
use winit::window::{Window, WindowLevel};

pub struct Gpu {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
}

/// The monitor named `name`, else the primary one.
pub fn pick_monitor(event_loop: &ActiveEventLoop, name: Option<&str>) -> Option<MonitorHandle> {
    name.and_then(|n| {
        event_loop
            .available_monitors()
            .find(|m| m.name().as_deref() == Some(n))
    })
    .or_else(|| event_loop.primary_monitor())
    .or_else(|| event_loop.available_monitors().next())
}

fn create_window(
    event_loop: &ActiveEventLoop,
    monitor: &MonitorHandle,
) -> Result<(Arc<Window>, isize)> {
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
    #[cfg(target_os = "macos")]
    {
        let view = ns_view(&window)?;
        // SAFETY: the view belongs to `window`, alive here; winit runs this on the main thread.
        unsafe { vutric_host_mac::make_overlay(view, vutric_host_mac::OverlayLevel::default()) }
            .map_err(|e| anyhow::anyhow!("overlay styles: {e}"))?;
    }
    Ok((window, hwnd))
}

/// The `NSView` of a winit window, for `vutric-host-mac` (ADR-0110).
#[cfg(target_os = "macos")]
fn ns_view(window: &Window) -> Result<std::ptr::NonNull<core::ffi::c_void>> {
    use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    match window.window_handle()?.as_raw() {
        RawWindowHandle::AppKit(h) => Ok(h.ns_view),
        _ => bail!("not an AppKit window"),
    }
}

fn surface_config(
    caps: &wgpu::SurfaceCapabilities,
    size: PhysicalSize<u32>,
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
        // 2, not 1: with 1 the UHD 730 at 1440p misses vblanks (ADR-0004, Phase 0.1 report).
        desired_maximum_frame_latency: 2,
        alpha_mode,
        color_space: wgpu::SurfaceColorSpace::Auto,
        view_formats: vec![],
    })
}

impl Gpu {
    pub fn new(
        event_loop: &ActiveEventLoop,
        monitor: &MonitorHandle,
        image: &image::RgbaImage,
    ) -> Result<Self> {
        #[cfg_attr(not(windows), allow(unused_variables))]
        let (window, hwnd) = create_window(event_loop, monitor)?;
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        if cfg!(windows) {
            desc.backends = wgpu::Backends::DX12;
            desc.backend_options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
        } else if cfg!(target_os = "macos") {
            desc.backends = wgpu::Backends::METAL;
        }
        let instance = wgpu::Instance::new(desc);
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        }))
        .context("no GPU adapter")?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("vutric"),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                ..Default::default()
            }))?;
        let config = surface_config(&surface.get_capabilities(&adapter), window.inner_size())?;
        surface.configure(&device, &config);
        let mut renderer = Renderer::new(&device, &queue, config.format, Palette::vutric());
        renderer.set_image(
            &device,
            &queue,
            image.as_raw(),
            image.width(),
            image.height(),
        );
        let gpu = Self {
            window,
            surface,
            device,
            queue,
            config,
            renderer,
        };
        gpu.clear()?;
        #[cfg(windows)]
        vutric_host_win::show_without_activating(hwnd);
        #[cfg(target_os = "macos")]
        {
            let view = ns_view(&gpu.window)?;
            // SAFETY: as in `create_window`; `set_visible(true)` would make the window key and take focus.
            unsafe { vutric_host_mac::show_without_activating(view) };
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        gpu.window.set_visible(true);
        Ok(gpu)
    }

    pub fn set_image(&mut self, img: &image::RgbaImage) {
        self.renderer.set_image(
            &self.device,
            &self.queue,
            img.as_raw(),
            img.width(),
            img.height(),
        );
    }

    pub fn screen(&self) -> [f32; 2] {
        [self.config.width as f32, self.config.height as f32]
    }

    /// Presents a fully transparent frame (after an effect, so nothing lingers on screen).
    pub fn clear(&self) -> Result<()> {
        let wgpu::CurrentSurfaceTexture::Success(texture) = self.surface.get_current_texture()
        else {
            bail!("could not acquire a surface texture");
        };
        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
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
        self.queue.submit([encoder.finish()]);
        self.queue.present(texture);
        Ok(())
    }

    /// Draws one effect frame. `Ok(false)` = the surface was busy or rebuilt; ask for another redraw.
    pub fn draw(&mut self, frame: &Frame, t: f32) -> Result<bool> {
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
        self.renderer.encode(
            &self.queue,
            &mut encoder,
            &view,
            [self.config.width, self.config.height],
            frame,
            t,
        );
        self.queue.submit([encoder.finish()]);
        self.queue.present(texture);
        Ok(true)
    }
}

fn raw_hwnd(window: &Window) -> Result<isize> {
    use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    match window.window_handle()?.as_raw() {
        RawWindowHandle::Win32(h) => Ok(h.hwnd.get()),
        _ => Ok(0),
    }
}
