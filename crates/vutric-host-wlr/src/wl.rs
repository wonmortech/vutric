//! The Wayland side: one connection, one dispatch thread, and the two kinds of surface Vutric needs.
//!
//! - [`Surface`] is the effect surface. It hands wgpu a raw Wayland handle, never takes input (empty input region)
//!   and never takes keyboard focus. On layer-shell compositors it is a layer surface on the `overlay` layer
//!   anchored to every edge; on GNOME it is an undecorated xdg toplevel that the Vutric extension retypes
//!   (ADR-0090).
//! - The card is a small layer surface at the bottom-right of the output, drawn from a CPU raster through `wl_shm`.
//!   It takes clicks but never keyboard focus (`keyboard_interactivity = none`).
//!
//! The dispatch thread owns the event queue and polls the socket; protocol objects are created from the caller's
//! thread under the same lock. wgpu's Vulkan WSI reads the same socket through its own queue, which libwayland
//! supports.

use std::collections::HashMap;
use std::os::fd::AsRawFd as _;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface as _;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shell::xdg::XdgShell;
use smithay_client_toolkit::shell::xdg::window::{
    Window, WindowConfigure, WindowDecorations, WindowHandler,
};
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm, delegate_xdg_shell, delegate_xdg_window, registry_handlers,
};
use wayland_client::backend::{ObjectId, WaylandError};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface};
use wayland_client::{Connection, EventQueue, Proxy as _, QueueHandle};

use crate::geometry::{CARD_MARGIN, buffer_scale, pad_to_scale, to_buffer};

const CONFIGURE_TIMEOUT: Duration = Duration::from_secs(2);
const BTN_LEFT: u32 = 0x110;

/// A pointer event on the card, in buffer pixels (the same enum as `vutric-host-win::card::Mouse`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mouse {
    Move(f32, f32),
    Up(f32, f32),
    Leave,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Connect(String),
    NoCompositor,
    NoLayerShell,
    NoXdgShell,
    NoShm,
    Protocol(String),
    Timeout(&'static str),
    Closed,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(e) => write!(f, "cannot connect to the Wayland compositor: {e}"),
            Self::NoCompositor => f.write_str("the compositor offers no wl_compositor"),
            Self::NoLayerShell => f.write_str("the compositor offers no zwlr_layer_shell_v1"),
            Self::NoXdgShell => f.write_str("the compositor offers no xdg_wm_base"),
            Self::NoShm => f.write_str("the compositor offers no wl_shm"),
            Self::Protocol(e) => write!(f, "Wayland protocol error: {e}"),
            Self::Timeout(what) => write!(f, "the compositor did not configure the {what} in time"),
            Self::Closed => f.write_str("the Wayland connection or surface was closed"),
        }
    }
}

impl std::error::Error for Error {}

/// One output as the compositor describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputSummary {
    pub name: Option<String>,
    pub description: Option<String>,
    pub logical_size: Option<(i32, i32)>,
    pub scale: i32,
}

struct Card {
    layer: LayerSurface,
    pool: SlotPool,
    logical: (u32, u32),
    scale: i32,
}

struct State {
    registry: RegistryState,
    outputs: OutputState,
    seats: SeatState,
    compositor: CompositorState,
    layer_shell: Option<LayerShell>,
    xdg: Option<XdgShell>,
    shm: Shm,
    pointer: Option<wl_pointer::WlPointer>,
    configured: HashMap<ObjectId, (u32, u32)>,
    closed: Vec<ObjectId>,
    card: Option<Card>,
    mouse: Vec<Mouse>,
    dead: bool,
}

impl State {
    fn pick_output(&self, name: Option<&str>) -> Option<wl_output::WlOutput> {
        let mut all = self.outputs.outputs();
        match name {
            Some(n) => self
                .outputs
                .outputs()
                .find(|o| self.outputs.info(o).and_then(|i| i.name).as_deref() == Some(n))
                .or_else(|| all.next()),
            None => all.next(),
        }
    }

    fn output_scale(&self, o: Option<&wl_output::WlOutput>) -> i32 {
        o.and_then(|o| self.outputs.info(o))
            .map_or(1, |i| buffer_scale(i.scale_factor))
    }

    fn output_size(&self, o: Option<&wl_output::WlOutput>) -> Option<(u32, u32)> {
        let (w, h) = o.and_then(|o| self.outputs.info(o))?.logical_size?;
        Some((w.cast_unsigned(), h.cast_unsigned()))
    }
}

struct Shared {
    state: Mutex<State>,
    configured: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn wait_configured(&self, id: &ObjectId, what: &'static str) -> Result<(u32, u32), Error> {
        let st = self.lock();
        let (st, _) = self
            .configured
            .wait_timeout_while(st, CONFIGURE_TIMEOUT, |s| {
                !s.dead && !s.configured.contains_key(id) && !s.closed.contains(id)
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if st.dead || st.closed.contains(id) {
            return Err(Error::Closed);
        }
        st.configured.get(id).copied().ok_or(Error::Timeout(what))
    }
}

/// The Wayland connection plus its dispatch thread. Dropping it stops the thread.
pub struct Host {
    conn: Connection,
    qh: QueueHandle<State>,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host").finish_non_exhaustive()
    }
}

impl Host {
    /// Connects to `$WAYLAND_DISPLAY`. `on_mouse` runs on the dispatch thread for pointer events on the card;
    /// it must not call back into this `Host`.
    pub fn connect(on_mouse: impl FnMut(Mouse) + Send + 'static) -> Result<Self, Error> {
        let conn = Connection::connect_to_env().map_err(|e| Error::Connect(e.to_string()))?;
        let (globals, mut queue) =
            registry_queue_init::<State>(&conn).map_err(|e| Error::Connect(e.to_string()))?;
        let qh = queue.handle();
        let compositor = CompositorState::bind(&globals, &qh).map_err(|_| Error::NoCompositor)?;
        let shm = Shm::bind(&globals, &qh).map_err(|_| Error::NoShm)?;
        let mut state = State {
            registry: RegistryState::new(&globals),
            outputs: OutputState::new(&globals, &qh),
            seats: SeatState::new(&globals, &qh),
            compositor,
            layer_shell: LayerShell::bind(&globals, &qh).ok(),
            xdg: XdgShell::bind(&globals, &qh).ok(),
            shm,
            pointer: None,
            configured: HashMap::new(),
            closed: Vec::new(),
            card: None,
            mouse: Vec::new(),
            dead: false,
        };
        // Two round trips: wl_output, then xdg_output names and logical sizes.
        for _ in 0..2 {
            queue
                .roundtrip(&mut state)
                .map_err(|e| Error::Protocol(e.to_string()))?;
        }
        let shared = Arc::new(Shared {
            state: Mutex::new(state),
            configured: Condvar::new(),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (conn, shared, stop) = (conn.clone(), Arc::clone(&shared), Arc::clone(&stop));
            std::thread::Builder::new()
                .name("vutric-wayland".into())
                .spawn(move || pump(&conn, &mut queue, &shared, &stop, Box::new(on_mouse)))
                .map_err(|e| Error::Connect(e.to_string()))?
        };
        Ok(Self {
            conn,
            qh,
            shared,
            stop,
            thread: Some(thread),
        })
    }

    #[must_use]
    pub fn has_layer_shell(&self) -> bool {
        self.shared.lock().layer_shell.is_some()
    }

    #[must_use]
    pub fn outputs(&self) -> Vec<OutputSummary> {
        let st = self.shared.lock();
        st.outputs
            .outputs()
            .filter_map(|o| st.outputs.info(&o))
            .map(|i| OutputSummary {
                name: i.name,
                description: i.description,
                logical_size: i.logical_size,
                scale: i.scale_factor,
            })
            .collect()
    }

    /// The buffer scale to rasterise the card at, for the output named `output` (else the first one).
    #[must_use]
    pub fn card_scale(&self, output: Option<&str>) -> i32 {
        let st = self.shared.lock();
        st.output_scale(st.pick_output(output).as_ref())
    }

    fn flush(&self) -> Result<(), Error> {
        self.conn
            .flush()
            .map_err(|e| Error::Protocol(e.to_string()))
    }

    /// The effect surface on a layer-shell compositor: layer `overlay`, every edge, no exclusive zone, no
    /// keyboard, empty input region. Blocks until the compositor has sent its size.
    pub fn overlay_layer(&self, output: Option<&str>) -> Result<Surface, Error> {
        let mut st = self.shared.lock();
        let out = st.pick_output(output);
        let scale = st.output_scale(out.as_ref());
        let st_ref = &mut *st;
        let layer_shell = st_ref.layer_shell.as_ref().ok_or(Error::NoLayerShell)?;
        let wl = st_ref.compositor.create_surface(&self.qh);
        let empty = Region::new(&st_ref.compositor).map_err(|e| Error::Protocol(e.to_string()))?;
        wl.set_input_region(Some(empty.wl_region()));
        wl.set_buffer_scale(scale);
        let layer = layer_shell.create_layer_surface(
            &self.qh,
            wl.clone(),
            Layer::Overlay,
            Some("vutric-overlay"),
            out.as_ref(),
        );
        layer.set_anchor(Anchor::all());
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(0, 0);
        layer.commit();
        drop(st);
        self.flush()?;
        self.shared.wait_configured(&wl.id(), "overlay")?;
        Ok(Surface {
            conn: self.conn.clone(),
            shared: Arc::clone(&self.shared),
            wl,
            role: Role::Layer(layer),
            scale,
        })
    }

    /// The effect surface on GNOME: an undecorated xdg toplevel with `app_id`, an empty input region and the
    /// size of the chosen output. The Vutric Shell extension recognises it, retypes it and moves it over the
    /// monitor (ADR-0090); without the extension it would be an ordinary window, so callers only use this when
    /// the extension answered on D-Bus.
    pub fn overlay_toplevel(
        &self,
        app_id: &str,
        title: &str,
        output: Option<&str>,
    ) -> Result<Surface, Error> {
        let mut st = self.shared.lock();
        let out = st.pick_output(output);
        let scale = st.output_scale(out.as_ref());
        let size = st.output_size(out.as_ref());
        let st_ref = &mut *st;
        let xdg = st_ref.xdg.as_ref().ok_or(Error::NoXdgShell)?;
        let wl = st_ref.compositor.create_surface(&self.qh);
        let empty = Region::new(&st_ref.compositor).map_err(|e| Error::Protocol(e.to_string()))?;
        wl.set_input_region(Some(empty.wl_region()));
        wl.set_buffer_scale(scale);
        let window = xdg.create_window(wl.clone(), WindowDecorations::None, &self.qh);
        window.set_app_id(app_id);
        window.set_title(title);
        window.commit();
        if let Some(sz) = size {
            st.configured.entry(wl.id()).or_insert(sz);
        }
        drop(st);
        self.flush()?;
        self.shared.wait_configured(&wl.id(), "overlay window")?;
        Ok(Surface {
            conn: self.conn.clone(),
            shared: Arc::clone(&self.shared),
            wl,
            role: Role::Toplevel(window),
            scale,
        })
    }

    /// Shows a card raster (premultiplied BGRA, `width * height * 4` bytes, rendered at `scale`) at the
    /// bottom-right of `output`. A card of the same size reuses its surface; a new size replaces it.
    pub fn show_card(
        &self,
        bgra: &[u8],
        width: u32,
        height: u32,
        scale: i32,
        output: Option<&str>,
    ) -> Result<(), Error> {
        let scale = buffer_scale(scale);
        let (pixels, w, h) = pad_to_scale(bgra, width, height, scale);
        let logical = (w / scale.cast_unsigned(), h / scale.cast_unsigned());
        let mut st = self.shared.lock();
        let fresh = st
            .card
            .as_ref()
            .is_none_or(|c| c.logical != logical || c.scale != scale);
        if fresh {
            st.card = None;
            let out = st.pick_output(output);
            let layer_shell = st.layer_shell.as_ref().ok_or(Error::NoLayerShell)?;
            let wl = st.compositor.create_surface(&self.qh);
            wl.set_buffer_scale(scale);
            let layer = layer_shell.create_layer_surface(
                &self.qh,
                wl.clone(),
                Layer::Overlay,
                Some("vutric-card"),
                out.as_ref(),
            );
            layer.set_anchor(Anchor::BOTTOM | Anchor::RIGHT);
            layer.set_margin(0, CARD_MARGIN, CARD_MARGIN, 0);
            layer.set_size(logical.0, logical.1);
            layer.set_exclusive_zone(0);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            layer.commit();
            let pool =
                SlotPool::new(pixels.len(), &st.shm).map_err(|e| Error::Protocol(e.to_string()))?;
            let id = wl.id();
            st.card = Some(Card {
                layer,
                pool,
                logical,
                scale,
            });
            drop(st);
            self.flush()?;
            self.shared.wait_configured(&id, "card")?;
            st = self.shared.lock();
        }
        let card = st.card.as_mut().ok_or(Error::Closed)?;
        let stride = (w * 4).cast_signed();
        let (buffer, canvas) = card
            .pool
            .create_buffer(
                w.cast_signed(),
                h.cast_signed(),
                stride,
                wl_shm::Format::Argb8888,
            )
            .map_err(|e| Error::Protocol(e.to_string()))?;
        canvas.copy_from_slice(&pixels);
        let wl = card.layer.wl_surface();
        wl.damage_buffer(0, 0, w.cast_signed(), h.cast_signed());
        buffer
            .attach_to(wl)
            .map_err(|e| Error::Protocol(format!("{e:?}")))?;
        card.layer.commit();
        drop(st);
        self.flush()
    }

    /// Removes the card surface. A hidden card costs nothing on the compositor.
    pub fn hide_card(&self) {
        let old = self.shared.lock().card.take();
        drop(old);
        let _ = self.flush();
    }

    /// True while a card surface exists.
    #[must_use]
    pub fn card_visible(&self) -> bool {
        self.shared.lock().card.is_some()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn pump(
    conn: &Connection,
    queue: &mut EventQueue<State>,
    shared: &Shared,
    stop: &AtomicBool,
    mut on_mouse: Box<dyn FnMut(Mouse) + Send>,
) {
    let mut alive = true;
    while alive && !stop.load(Ordering::Relaxed) {
        let mouse = {
            let mut st = shared.lock();
            alive = queue.dispatch_pending(&mut st).is_ok();
            shared.configured.notify_all();
            std::mem::take(&mut st.mouse)
        };
        for m in mouse {
            on_mouse(m);
        }
        if !alive {
            break;
        }
        if conn.flush().is_err() {
            break;
        }
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let mut pfd = libc::pollfd {
            fd: guard.connection_fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd for the duration of the call.
        let ready = unsafe { libc::poll(&raw mut pfd, 1, 100) };
        if ready > 0 {
            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => alive = false,
            }
        }
    }
    shared.lock().dead = true;
    shared.configured.notify_all();
}

#[allow(
    dead_code,
    reason = "held only to keep the role object (and so the mapped surface) alive"
)]
enum Role {
    Layer(LayerSurface),
    Toplevel(Window),
}

/// The effect surface. Pass it to `wgpu::Instance::create_surface` (it implements the raw-window-handle traits);
/// keep it alive for as long as the wgpu surface.
pub struct Surface {
    conn: Connection,
    shared: Arc<Shared>,
    wl: wl_surface::WlSurface,
    role: Role,
    scale: i32,
}

impl std::fmt::Debug for Surface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Surface")
            .field("scale", &self.scale)
            .field(
                "kind",
                &match self.role {
                    Role::Layer(_) => "layer",
                    Role::Toplevel(_) => "toplevel",
                },
            )
            .finish_non_exhaustive()
    }
}

impl Surface {
    /// The latest size from the compositor, in logical pixels.
    #[must_use]
    pub fn logical_size(&self) -> (u32, u32) {
        self.shared
            .lock()
            .configured
            .get(&self.wl.id())
            .copied()
            .unwrap_or((1, 1))
    }

    /// Buffer pixels: what the wgpu surface must be configured with.
    #[must_use]
    pub fn pixel_size(&self) -> (u32, u32) {
        let (w, h) = self.logical_size();
        let s = self.scale.cast_unsigned();
        ((w * s).max(1), (h * s).max(1))
    }

    #[must_use]
    pub fn scale(&self) -> i32 {
        self.scale
    }

    /// The compositor closed the surface (output unplugged, or the extension went away).
    #[must_use]
    pub fn is_closed(&self) -> bool {
        let st = self.shared.lock();
        st.dead || st.closed.contains(&self.wl.id())
    }

    /// True for a layer surface, false for the GNOME toplevel.
    #[must_use]
    pub fn is_layer(&self) -> bool {
        matches!(self.role, Role::Layer(_))
    }
}

impl HasWindowHandle for Surface {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let ptr = NonNull::new(self.wl.id().as_ptr().cast()).ok_or(HandleError::Unavailable)?;
        let raw = RawWindowHandle::Wayland(WaylandWindowHandle::new(ptr));
        // SAFETY: the wl_surface lives as long as `self`.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for Surface {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let ptr = NonNull::new(self.conn.backend().display_ptr().cast())
            .ok_or(HandleError::Unavailable)?;
        let raw = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(ptr));
        // SAFETY: the connection (and its wl_display) lives as long as `self`.
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }
}

impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        let id = layer.wl_surface().id();
        if self
            .card
            .as_ref()
            .is_some_and(|c| c.layer.wl_surface().id() == id)
        {
            self.card = None;
        }
        self.configured.remove(&id);
        self.closed.push(id);
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let (w, h) = configure.new_size;
        let id = layer.wl_surface().id();
        let size = if w == 0 || h == 0 {
            self.card
                .as_ref()
                .filter(|c| c.layer.wl_surface().id() == id)
                .map_or((1, 1), |c| c.logical)
        } else {
            (w, h)
        };
        self.configured.insert(id, size);
    }
}

impl WindowHandler for State {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, window: &Window) {
        // The overlay is never closed by the user; the request comes from the compositor shutting down.
        self.closed.push(window.wl_surface().id());
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        window: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        let id = window.wl_surface().id();
        if let (Some(w), Some(h)) = configure.new_size {
            self.configured.insert(id, (w.get(), h.get()));
        } else {
            let fallback = self.configured.get(&id).copied().unwrap_or((1, 1));
            self.configured.insert(id, fallback);
        }
    }
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seats.get_pointer(qh, &seat).ok();
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && let Some(p) = self.pointer.take()
        {
            p.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for State {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let Some(card) = &self.card else { return };
        let surface = card.layer.wl_surface();
        let scale = card.scale;
        for e in events.iter().filter(|e| &e.surface == surface) {
            let (x, y) = to_buffer(e.position.0, e.position.1, scale);
            match e.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.mouse.push(Mouse::Move(x, y));
                }
                PointerEventKind::Release {
                    button: BTN_LEFT, ..
                } => self.mouse.push(Mouse::Up(x, y)),
                PointerEventKind::Leave { .. } => self.mouse.push(Mouse::Leave),
                _ => {}
            }
        }
    }
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(State);
delegate_output!(State);
delegate_shm!(State);
delegate_seat!(State);
delegate_pointer!(State);
delegate_layer!(State);
delegate_xdg_shell!(State);
delegate_xdg_window!(State);
delegate_registry!(State);
