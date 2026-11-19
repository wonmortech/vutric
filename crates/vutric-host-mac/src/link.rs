//! Frame pacing from the display itself (ADR-0114): a `CADisplayLink` made by the overlay's view (macOS 14+),
//! asking for 80–120 Hz so a `ProMotion` panel runs the effect at 120 Hz instead of whatever rate the system picks for
//! a window that only presents.

use core::cell::RefCell;
use core::ffi::c_void;
use core::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::NSView;
use objc2_foundation::{NSObject, NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange};

/// One display-link callback: `timestamp` is when the last frame was shown, `target` when the next one will be,
/// both in seconds of `CACurrentMediaTime`. `target - timestamp` is the refresh period the system gives us.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tick {
    pub timestamp: f64,
    pub target: f64,
}

type Callback = Box<dyn FnMut(Tick)>;

#[derive(Default)]
struct Ivars {
    callback: RefCell<Option<Callback>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "VutricDisplayLinkTarget"]
    #[ivars = Ivars]
    struct Target;

    impl Target {
        #[unsafe(method(tick:))]
        fn tick(&self, link: &CADisplayLink) {
            let t = Tick {
                timestamp: link.timestamp(),
                target: link.targetTimestamp(),
            };
            if let Some(cb) = self.ivars().callback.borrow_mut().as_mut() {
                cb(t);
            }
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker, callback: Callback) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            callback: RefCell::new(Some(callback)),
        });
        // SAFETY: `NSObject`'s designated initialiser on a freshly allocated instance.
        unsafe { msg_send![super(this), init] }
    }
}

/// A running display link. Paused while nothing animates, so an idle overlay costs no wake-ups; invalidated on drop.
pub struct DisplayLink {
    link: Retained<CADisplayLink>,
    _target: Retained<Target>,
}

impl std::fmt::Debug for DisplayLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DisplayLink").finish_non_exhaustive()
    }
}

/// Frame-rate range asked for: 80–120 Hz, preferring 120. On a 60 Hz display the system clamps it to 60.
pub const PREFERRED_RANGE: (f32, f32, f32) = (80.0, 120.0, 120.0);

impl DisplayLink {
    /// Creates a paused link for the screen `ns_view` is on, calling `callback` on the main thread once per frame
    /// while running. `None` before macOS 14, where `NSView.displayLink(target:selector:)` does not exist; the caller
    /// then keeps Fifo pacing.
    ///
    /// # Safety
    /// `ns_view` must point to a live `NSView`.
    pub unsafe fn new(
        mtm: MainThreadMarker,
        ns_view: NonNull<c_void>,
        callback: impl FnMut(Tick) + 'static,
    ) -> Option<Self> {
        if !objc2::available!(macos = 14.0) {
            return None;
        }
        // SAFETY: the caller guarantees a live NSView.
        let view: &NSView = unsafe { ns_view.cast::<NSView>().as_ref() };
        let target = Target::new(mtm, Box::new(callback));
        let any: &AnyObject = &target;
        // SAFETY: `tick:` is defined on `Target` with one object argument, as the selector requires.
        let link = unsafe { view.displayLinkWithTarget_selector(any, sel!(tick:)) };
        let (minimum, maximum, preferred) = PREFERRED_RANGE;
        link.setPreferredFrameRateRange(CAFrameRateRange {
            minimum,
            maximum,
            preferred,
        });
        link.setPaused(true);
        // SAFETY: the main run loop and a system run-loop mode constant; we are on the main thread.
        unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
        Some(Self {
            link,
            _target: target,
        })
    }

    pub fn set_running(&self, running: bool) {
        self.link.setPaused(!running);
    }
}

impl Drop for DisplayLink {
    fn drop(&mut self) {
        self.link.invalidate();
    }
}
