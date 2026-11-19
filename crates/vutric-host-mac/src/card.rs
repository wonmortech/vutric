//! The instant cards on macOS (ADR-0118, W2 #30): W2's CPU-drawn cards (`vutric_overlay::card`, ADR-0032) shown in a
//! small non-activating `NSPanel` that takes clicks without making Vutric the active app or the panel key.
//!
//! The overlay panel (ADR-0110) stays click-through and GPU-only during effects; a card is a separate panel whose
//! layer shows the BGRA pixels the card renderer produced, so no GPU is touched to show it.

use core::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSPanel, NSResponder, NSScreen, NSView, NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGRect};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGEvent, CGEventTapLocation,
    CGEventType, CGMouseButton,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::OverlayLevel;
use crate::policy::{ANIMATION_NONE, behavior, style};

/// A click in card pixels (top-left origin, backing pixels: the same space as `Rendered::buttons`).
pub type OnClick = Box<dyn FnMut(f32, f32)>;

#[derive(Default)]
struct Ivars {
    on_click: RefCell<Option<OnClick>>,
    scale: core::cell::Cell<f64>,
}

define_class!(
    #[unsafe(super(NSView, NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "VutricCardView"]
    #[ivars = Ivars]
    struct CardView;

    impl CardView {
        /// The first click on an inactive app's window is a click, not "activate the window first".
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {}

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let p = self.convertPoint_fromView(event.locationInWindow(), None);
            let s = self.ivars().scale.get().max(1.0);
            if let Some(cb) = self.ivars().on_click.borrow_mut().as_mut() {
                cb((p.x * s) as f32, (p.y * s) as f32);
            }
        }
    }
);

/// `kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little`: B, G, R, A in memory, premultiplied, which is
/// what `vutric_overlay::card::render` writes.
pub const BGRA_PREMULTIPLIED: u32 = 2 | (2 << 12);

/// Where a card goes: the bottom-right of the visible frame (menu bar and Dock excluded) of the screen under the
/// pointer, inset by `margin` points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Bottom-left corner, `AppKit` screen points (origin bottom-left of the main screen).
    pub origin: (f64, f64),
    /// Backing scale of that screen (2.0 on Retina): render the card at this scale.
    pub scale: f64,
}

/// The screen under the pointer, else the main screen.
fn pointer_screen(mtm: MainThreadMarker) -> Option<Retained<NSScreen>> {
    let at = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    screens
        .iter()
        .find(|s| {
            let f = s.frame();
            at.x >= f.origin.x
                && at.x < f.origin.x + f.size.width
                && at.y >= f.origin.y
                && at.y < f.origin.y + f.size.height
        })
        .or_else(|| NSScreen::mainScreen(mtm))
}

/// The scale to render the next card at.
#[must_use]
pub fn target_scale(mtm: MainThreadMarker) -> f64 {
    pointer_screen(mtm).map_or(2.0, |s| s.backingScaleFactor())
}

/// Bottom-right of the pointer's screen for a card of `w`×`h` pixels rendered at `scale`.
#[must_use]
pub fn placement(mtm: MainThreadMarker, w: u32, h: u32, scale: f64, margin: f64) -> Placement {
    let Some(screen) = pointer_screen(mtm) else {
        return Placement {
            origin: (margin, margin),
            scale,
        };
    };
    let v = screen.visibleFrame();
    let (pw, _ph) = (f64::from(w) / scale, f64::from(h) / scale);
    Placement {
        origin: (v.origin.x + v.size.width - pw - margin, v.origin.y + margin),
        scale,
    }
}

/// One card panel, hidden until [`CardPanel::show`].
pub struct CardPanel {
    panel: Retained<NSPanel>,
    view: Retained<CardView>,
}

impl std::fmt::Debug for CardPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CardPanel").finish_non_exhaustive()
    }
}

impl CardPanel {
    /// A borderless, non-activating panel at `level` that accepts clicks (`on_click` gets card pixels).
    #[must_use]
    pub fn new(
        mtm: MainThreadMarker,
        level: OverlayLevel,
        on_click: impl FnMut(f32, f32) + 'static,
    ) -> Self {
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 124.0));
        let mask = NSWindowStyleMask(style::BORDERLESS | style::NONACTIVATING_PANEL);
        // SAFETY: standard designated initialiser on a fresh allocation, on the main thread.
        let panel: Retained<NSPanel> = unsafe {
            msg_send![NSPanel::alloc(mtm), initWithContentRect: rect, styleMask: mask,
                backing: NSBackingStoreType::Buffered, defer: false]
        };
        let view = CardView::alloc(mtm).set_ivars(Ivars {
            on_click: RefCell::new(Some(Box::new(on_click))),
            scale: core::cell::Cell::new(2.0),
        });
        // SAFETY: NSView's designated initialiser.
        let view: Retained<CardView> = unsafe { msg_send![super(view), initWithFrame: rect] };
        view.setWantsLayer(true);
        panel.setContentView(Some(&view));
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(false); // the card draws its own shadow
        panel.setIgnoresMouseEvents(false);
        panel.setHidesOnDeactivate(false);
        panel.setFloatingPanel(true);
        panel.setBecomesKeyOnlyIfNeeded(true);
        // SAFETY: plain property setters with values from the SDK enums (checked in `imp` tests).
        unsafe {
            panel.setReleasedWhenClosed(false);
            let _: () = msg_send![&*panel, setAnimationBehavior: ANIMATION_NONE];
            let _: () = msg_send![&*panel, setCollectionBehavior: behavior::OVERLAY];
        }
        panel.setLevel(crate::resolve_level(level));
        Self { panel, view }
    }

    /// Shows `bgra` (premultiplied, `w`×`h` pixels at `at.scale`) at `at` without activating anything.
    ///
    /// # Errors
    /// When Core Graphics cannot make an image of the pixels.
    pub fn show(&self, w: u32, h: u32, bgra: &[u8], at: Placement) -> Result<(), String> {
        let (wu, hu) = (w as usize, h as usize);
        if bgra.len() < wu * hu * 4 || w == 0 || h == 0 {
            return Err(format!("{} bytes for {w}x{h}", bgra.len()));
        }
        let space = CGColorSpace::new_device_rgb().ok_or("no RGB colour space")?;
        let mut copy = bgra[..wu * hu * 4].to_vec();
        // SAFETY: `copy` holds w*h*4 bytes with rows of w*4; the context only lives inside this block and
        // `CGBitmapContextCreateImage` copies the pixels out before `copy` is dropped.
        let image = unsafe {
            let ctx = CGBitmapContextCreate(
                copy.as_mut_ptr().cast(),
                wu,
                hu,
                8,
                wu * 4,
                Some(&space),
                BGRA_PREMULTIPLIED,
            )
            .ok_or("CGBitmapContextCreate failed")?;
            CGBitmapContextCreateImage(Some(&ctx)).ok_or("CGBitmapContextCreateImage failed")?
        };
        let size = NSSize::new(f64::from(w) / at.scale, f64::from(h) / at.scale);
        self.view.ivars().scale.set(at.scale);
        let frame = NSRect::new(NSPoint::new(at.origin.0, at.origin.1), size);
        self.panel.setFrame_display(frame, false);
        let layer = self.view.layer().ok_or("view has no layer")?;
        layer.setContentsScale(at.scale);
        let any: &AnyObject = (&*image as &objc2_core_graphics::CGImage).as_ref();
        // SAFETY: a layer's contents may be a CGImage.
        unsafe { layer.setContents(Some(any)) };
        self.panel.orderFrontRegardless();
        Ok(())
    }

    pub fn hide(&self) {
        self.panel.orderOut(None);
    }

    #[must_use]
    pub fn is_key(&self) -> bool {
        self.panel.isKeyWindow()
    }

    #[must_use]
    pub fn is_visible(&self) -> bool {
        self.panel.isVisible()
    }

    /// The panel's frame in `AppKit` screen points.
    #[must_use]
    pub fn frame(&self) -> CGRect {
        self.panel.frame()
    }
}

/// Posts a real left click through the window server at `AppKit` screen point `at` (origin bottom-left of the main
/// screen). `false` when this process may not post events (macOS asks for Accessibility) or nothing was posted.
#[must_use]
pub fn synthetic_click(mtm: MainThreadMarker, at: (f64, f64)) -> bool {
    if !objc2_core_graphics::CGPreflightPostEventAccess() {
        return false;
    }
    let main_h = NSScreen::mainScreen(mtm).map_or(0.0, |s| s.frame().size.height);
    let p = CGPoint::new(at.0, main_h - at.1);
    let mut ok = true;
    for kind in [CGEventType::LeftMouseDown, CGEventType::LeftMouseUp] {
        match CGEvent::new_mouse_event(None, kind, p, CGMouseButton::Left) {
            Some(e) => CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&e)),
            None => ok = false,
        }
    }
    ok
}

/// Runs the main run loop for `ms`, handing every event to `NSApp`, so a probe without `NSApplication.run` still
/// gets its clicks and its windows drawn.
pub fn pump(mtm: MainThreadMarker, ms: u64) {
    use objc2_app_kit::{NSApplication, NSEventMask};
    use objc2_foundation::{NSDate, NSDefaultRunLoopMode};
    let app = NSApplication::sharedApplication(mtm);
    let until = NSDate::dateWithTimeIntervalSinceNow(ms as f64 / 1000.0);
    loop {
        // SAFETY: the default run-loop mode constant; main thread.
        let ev = unsafe {
            app.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&until),
                NSDefaultRunLoopMode,
                true,
            )
        };
        match ev {
            Some(e) => app.sendEvent(&e),
            None => break,
        }
    }
}
