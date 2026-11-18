use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use vutric_core::{Gesture, SendZone};
use vutric_host_linux::gnome::SURFACE_APP_ID;
use vutric_host_linux::proto::{self, Decoder, FrameStats, FromSurface, Hit, Image, ToSurface};
use vutric_host_linux::stats::Recorder;
use vutric_host_wlr::session::{Backend, Session};
use vutric_host_wlr::{Host, Mouse, Surface};
use vutric_overlay::{Halqa, HalqaConfig, Palette, Renderer};

/// GPU kept this long after the last effect (ADR-0031 `GPU_LINGER`).
const GPU_LINGER: Duration = Duration::from_secs(30);

struct Args {
    socket: Option<String>,
    backend: Option<Backend>,
    output: Option<String>,
    self_test: u32,
    cards: u32,
    out: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        socket: None,
        backend: None,
        output: None,
        self_test: 0,
        cards: 0,
        out: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = || it.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--socket" => a.socket = Some(val()?),
            "--output" => a.output = Some(val()?),
            "--out" => a.out = Some(val()?),
            "--backend" => {
                a.backend = match val()?.as_str() {
                    "auto" => None,
                    "layer" => Some(Backend::LayerShell),
                    "gnome" => Some(Backend::GnomeShell),
                    other => return Err(format!("unknown backend {other}")),
                };
            }
            "--self-test" => a.self_test = val()?.parse().map_err(|e| format!("{e}"))?,
            "--cards" => a.cards = val()?.parse().map_err(|e| format!("{e}"))?,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if a.socket.is_none() && a.self_test == 0 {
        return Err("give --socket PATH or --self-test N".into());
    }
    Ok(a)
}

fn backend_name(b: Backend) -> &'static str {
    match b {
        Backend::LayerShell => "layer-shell",
        Backend::GnomeShell => "gnome-shell",
        Backend::Unsupported(_) => "unsupported",
    }
}

struct Gpu {
    surf: Arc<Surface>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    adapter: String,
}

impl Gpu {
    fn new(host: &Host, backend: Backend, output: Option<&str>) -> Result<Self, String> {
        let surf = Arc::new(
            match backend {
                Backend::GnomeShell => host.overlay_toplevel(SURFACE_APP_ID, "Vutric", output),
                _ => host.overlay_layer(output),
            }
            .map_err(|e| e.to_string())?,
        );
        let mut desc =
            wgpu::InstanceDescriptor::new_with_display_handle(Box::new(Arc::clone(&surf)))
                .with_env();
        if std::env::var_os("WGPU_BACKEND").is_none() {
            desc.backends = wgpu::Backends::VULKAN | wgpu::Backends::GL;
        }
        let instance = wgpu::Instance::new(desc);
        let surface = instance
            .create_surface(Arc::clone(&surf))
            .map_err(|e| e.to_string())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("no GPU adapter: {e}"))?;
        let info = adapter.get_info();
        let label = format!("{} ({:?}, {})", info.name, info.backend, info.driver);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("vutric-surface"),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;
        let caps = surface.get_capabilities(&adapter);
        let Some(alpha_mode) = [
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::PostMultiplied,
            wgpu::CompositeAlphaMode::Inherit,
        ]
        .into_iter()
        .find(|m| caps.alpha_modes.contains(m)) else {
            return Err(format!(
                "adapter offers no transparent composition (alpha modes: {:?})",
                caps.alpha_modes
            ));
        };
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| caps.formats.first().copied())
            .ok_or("surface offers no format")?;
        let (width, height) = surf.pixel_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        let renderer = Renderer::new(&device, &queue, config.format, Palette::vutric());
        let gpu = Self {
            surf,
            surface,
            device,
            queue,
            config,
            renderer,
            adapter: label,
        };
        gpu.clear();
        Ok(gpu)
    }

    /// Follows a new size from the compositor (the GNOME extension moves and resizes the window after mapping).
    fn follow_size(&mut self) {
        let (w, h) = self.surf.pixel_size();
        if (w, h) != (self.config.width, self.config.height) {
            self.config.width = w;
            self.config.height = h;
            self.surface.configure(&self.device, &self.config);
        }
    }

    fn acquire(&self) -> Option<wgpu::SurfaceTexture> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                None
            }
            _ => None,
        }
    }

    fn clear(&self) {
        let Some(texture) = self.acquire() else {
            return;
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
    }

    fn set_image(&mut self, img: &Image) {
        if img.is_valid() {
            self.renderer
                .set_image(&self.device, &self.queue, &img.rgba, img.width, img.height);
        }
    }

    /// Plays one effect to the end. `poll` runs once per frame for commands that may arrive meanwhile.
    fn play(
        &mut self,
        zone: SendZone,
        gesture: Gesture,
        image: &Image,
        arrived: Instant,
        mut on_first: impl FnMut(Duration),
        mut poll: impl FnMut(&mut Self),
    ) -> FrameStats {
        self.follow_size();
        self.set_image(image);
        let screen = [self.config.width as f32, self.config.height as f32];
        let aspect = image.width.max(1) as f32 / image.height.max(1) as f32;
        let halqa = Halqa::new(HalqaConfig::default(), screen, zone, gesture, aspect);
        let started = Instant::now();
        let mut rec = Recorder::default();
        let mut last: Option<Instant> = None;
        let mut misses = 0u32;
        loop {
            let t = started.elapsed().as_secs_f32();
            let finished = halqa.is_finished(t);
            let frame = halqa.sample(t.min(halqa.duration()));
            let Some(texture) = self.acquire() else {
                misses += 1;
                if misses > 600 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
                continue;
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
                &frame,
                t,
            );
            self.queue.submit([encoder.finish()]);
            self.queue.present(texture);
            let now = Instant::now();
            match last {
                None => {
                    let d = now - arrived;
                    rec.first = Some(d);
                    on_first(d);
                }
                Some(prev) => rec.intervals.push(now - prev),
            }
            last = Some(now);
            if finished {
                break;
            }
            poll(self);
        }
        self.clear();
        rec.summary()
    }
}

/// A procedural photo so the self-test needs no asset.
fn sample_image(width: u32, height: u32) -> Image {
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let v = y as f32 / height as f32;
            let u = x as f32 / width as f32;
            rgba.extend_from_slice(&[
                (30.0 + 200.0 * v) as u8,
                (60.0 + 120.0 * (1.0 - v)) as u8,
                (120.0 + 100.0 * u) as u8,
                255,
            ]);
        }
    }
    Image {
        width,
        height,
        rgba,
    }
}

/// A synthetic card raster (premultiplied BGRA) for the self-test: a rounded panel with a button area.
fn sample_card(width: u32, height: u32) -> Vec<u8> {
    let mut out = vec![0u8; (width * height * 4) as usize];
    for (i, px) in out.chunks_exact_mut(4).enumerate() {
        let x = (i as u32) % width;
        let y = (i as u32) / width;
        let edge = x.min(width - 1 - x).min(y).min(height - 1 - y);
        let a: u8 = if edge < 2 { 120 } else { 240 };
        let c = |v: u32| (v * u32::from(a) / 255) as u8;
        px.copy_from_slice(&[c(40), c(30), c(24), a]);
    }
    out
}

type Writer = Arc<Mutex<Option<UnixStream>>>;

fn send(w: &Writer, msg: &FromSurface) {
    let Ok(bytes) = proto::encode(msg) else {
        return;
    };
    let mut g = w.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(s) = g.as_mut()
        && s.write_all(&bytes).is_err()
    {
        *g = None;
    }
}

fn reader(mut stream: UnixStream) -> Receiver<ToSurface> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut dec = Decoder::default();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            dec.push(&buf[..n]);
            loop {
                match dec.next_message::<ToSurface>() {
                    Ok(Some(m)) => {
                        if tx.send(m).is_err() {
                            return;
                        }
                    }
                    Ok(None) => break,
                    Err(_) => {
                        let _ = tx.send(ToSurface::Quit);
                        return;
                    }
                }
            }
        }
        let _ = tx.send(ToSurface::Quit);
    });
    rx
}

fn hit_test(hits: &[Hit], x: f32, y: f32) -> Option<String> {
    hits.iter()
        .find(|h| x >= h.rect[0] && x < h.rect[2] && y >= h.rect[1] && y < h.rect[3])
        .map(|h| h.id.clone())
}

#[allow(
    clippy::too_many_lines,
    reason = "one command loop, each arm a few lines"
)]
fn serve(args: &Args, socket: &str) -> Result<(), String> {
    let stream = UnixStream::connect(socket).map_err(|e| format!("connect {socket}: {e}"))?;
    let writer: Writer = Arc::new(Mutex::new(Some(
        stream.try_clone().map_err(|e| e.to_string())?,
    )));
    let hits: Arc<Mutex<Vec<Hit>>> = Arc::default();
    let host = {
        let (w, hits) = (Arc::clone(&writer), Arc::clone(&hits));
        Host::connect(move |m| {
            if let Mouse::Up(x, y) = m
                && let Some(id) =
                    hit_test(&hits.lock().unwrap_or_else(PoisonError::into_inner), x, y)
            {
                send(&w, &FromSurface::Clicked { id });
            }
        })
        .map_err(|e| e.to_string())?
    };
    let backend = args
        .backend
        .unwrap_or_else(|| Session::from_env().backend(host.has_layer_shell()));
    if let Backend::Unsupported(why) = backend {
        send(
            &writer,
            &FromSurface::Error {
                message: format!("no overlay on this session: {why:?}"),
            },
        );
        return Ok(());
    }
    let output = args.output.as_deref();
    send(
        &writer,
        &FromSurface::Ready {
            version: proto::VERSION,
            backend: backend_name(backend).into(),
            scale: host.card_scale(output),
        },
    );
    let rx = reader(stream);
    let mut gpu: Option<Gpu> = None;
    let mut last_effect = Instant::now();
    let ensure = |gpu: &mut Option<Gpu>| -> Result<(), String> {
        if gpu.is_none() {
            let t = Instant::now();
            let g = Gpu::new(&host, backend, output)?;
            send(
                &writer,
                &FromSurface::Warmed {
                    adapter: g.adapter.clone(),
                    width: g.config.width,
                    height: g.config.height,
                    ms: t.elapsed().as_secs_f32() * 1000.0,
                },
            );
            *gpu = Some(g);
        }
        Ok(())
    };
    let mut pending: Vec<ToSurface> = Vec::new();
    loop {
        let msg = match pending.pop() {
            Some(m) => m,
            None => match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(m) => m,
                Err(RecvTimeoutError::Timeout) => {
                    if gpu.is_some() && last_effect.elapsed() > GPU_LINGER {
                        gpu = None;
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            },
        };
        match msg {
            ToSurface::Warm => {
                if let Err(message) = ensure(&mut gpu) {
                    send(&writer, &FromSurface::Error { message });
                }
                last_effect = Instant::now();
            }
            ToSurface::Play {
                id,
                zone,
                gesture,
                image,
            } => {
                let arrived = Instant::now();
                if let Err(message) = ensure(&mut gpu) {
                    send(&writer, &FromSurface::Error { message });
                    continue;
                }
                let Some(g) = gpu.as_mut() else { continue };
                let w = Arc::clone(&writer);
                let stats = g.play(
                    zone,
                    gesture,
                    &image,
                    arrived,
                    |d| {
                        send(
                            &w,
                            &FromSurface::FirstFrame {
                                id,
                                after_ms: d.as_secs_f32() * 1000.0,
                            },
                        );
                    },
                    |g| {
                        while let Ok(m) = rx.try_recv() {
                            match m {
                                ToSurface::SetImage { id: i, image } if i == id => {
                                    g.set_image(&image);
                                }
                                other => pending.insert(0, other),
                            }
                        }
                    },
                );
                send(&writer, &FromSurface::Played { id, stats });
                last_effect = Instant::now();
            }
            ToSurface::SetImage { .. } => {}
            ToSurface::Card {
                width,
                height,
                scale,
                bgra,
                hits: h,
            } => {
                *hits.lock().unwrap_or_else(PoisonError::into_inner) = h;
                if let Err(e) = host.show_card(&bgra, width, height, scale, output) {
                    send(
                        &writer,
                        &FromSurface::Error {
                            message: format!("card: {e}"),
                        },
                    );
                }
            }
            ToSurface::HideCard => {
                hits.lock().unwrap_or_else(PoisonError::into_inner).clear();
                host.hide_card();
            }
            ToSurface::Release => gpu = None,
            ToSurface::Quit => break,
        }
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct Report {
    backend: &'static str,
    session: String,
    adapter: String,
    width: u32,
    height: u32,
    scale: i32,
    warm_ms: f32,
    runs: Vec<FrameStats>,
    card_show_ms: Vec<f32>,
    outputs: Vec<String>,
}

fn self_test(args: &Args) -> Result<bool, String> {
    let host = Host::connect(|_| {}).map_err(|e| e.to_string())?;
    let session = Session::from_env();
    let backend = args
        .backend
        .unwrap_or_else(|| session.backend(host.has_layer_shell()));
    if let Backend::Unsupported(why) = backend {
        return Err(format!("no overlay backend on this session: {why:?}"));
    }
    let output = args.output.as_deref();
    let t = Instant::now();
    let mut gpu = Gpu::new(&host, backend, output)?;
    let warm_ms = t.elapsed().as_secs_f32() * 1000.0;
    let image = sample_image(640, 400);
    let mut runs = Vec::new();
    for i in 0..args.self_test {
        let gesture = Gesture {
            speed: 2400.0,
            angle: 0.0,
            exit_x: 0.35 + 0.15 * (i % 3) as f32,
        };
        runs.push(gpu.play(
            SendZone::Right,
            gesture,
            &image,
            Instant::now(),
            |_| {},
            |_| {},
        ));
        std::thread::sleep(Duration::from_millis(300));
    }
    let mut card_show_ms = Vec::new();
    if backend == Backend::LayerShell && args.cards > 0 {
        let scale = host.card_scale(output);
        let (w, h) = (400 * scale.cast_unsigned(), 176 * scale.cast_unsigned());
        let card = sample_card(w, h);
        for i in 0..args.cards {
            let t = Instant::now();
            host.show_card(&card, w, h, scale, output)
                .map_err(|e| e.to_string())?;
            card_show_ms.push(t.elapsed().as_secs_f32() * 1000.0);
            if i % 10 == 9 {
                host.hide_card();
            }
        }
        host.hide_card();
    }
    let report = Report {
        backend: backend_name(backend),
        session: format!("{session:?}"),
        adapter: gpu.adapter.clone(),
        width: gpu.config.width,
        height: gpu.config.height,
        scale: gpu.surf.scale(),
        warm_ms,
        runs,
        card_show_ms,
        outputs: host
            .outputs()
            .iter()
            .map(|o| format!("{:?} {:?} x{}", o.name, o.logical_size, o.scale))
            .collect(),
    };
    println!(
        "vutric-surface self-test: {} on {} ({}x{} px, scale {}), GPU warm {:.1} ms",
        report.backend, report.adapter, report.width, report.height, report.scale, report.warm_ms
    );
    println!("run  frames    fps  first(ms)  p50(ms)  p95(ms)  p99(ms)  max(ms)  hitches");
    for (i, r) in report.runs.iter().enumerate() {
        println!(
            "{:>3}  {:>6}  {:>5.1}  {:>9.2}  {:>7.2}  {:>7.2}  {:>7.2}  {:>7.2}  {:>7}",
            i + 1,
            r.frames,
            r.fps,
            r.first_ms,
            r.p50_ms,
            r.p95_ms,
            r.p99_ms,
            r.max_ms,
            r.hitches
        );
    }
    if !report.card_show_ms.is_empty() {
        let mut s = report.card_show_ms.clone();
        s.sort_by(f32::total_cmp);
        let p = |q: f32| s[((s.len() - 1) as f32 * q).round() as usize];
        println!(
            "cards: {} shown, show p50 {:.2} ms, p99 {:.2} ms, max {:.2} ms",
            s.len(),
            p(0.5),
            p(0.99),
            s[s.len() - 1]
        );
    }
    if let Some(path) = &args.out {
        let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(report.runs.iter().all(|r| r.frames > 1))
}

pub fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("vutric-surface: {e}");
            return ExitCode::from(2);
        }
    };
    let result = if args.self_test > 0 {
        self_test(&args).map(|ok| u8::from(!ok))
    } else {
        let socket = args.socket.clone().unwrap_or_default();
        serve(&args, &socket).map(|()| 0)
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("vutric-surface: {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_are_half_open() {
        let h = [Hit {
            id: "accept".into(),
            rect: [10.0, 10.0, 20.0, 20.0],
        }];
        assert_eq!(hit_test(&h, 10.0, 10.0).as_deref(), Some("accept"));
        assert_eq!(hit_test(&h, 20.0, 15.0), None);
        assert_eq!(hit_test(&h, 5.0, 15.0), None);
    }

    #[test]
    fn sample_assets_have_the_right_sizes() {
        assert!(sample_image(8, 4).is_valid());
        assert_eq!(sample_card(8, 4).len(), 8 * 4 * 4);
    }
}
