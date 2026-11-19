//! The `AppKit` calls. Everything here runs on the main thread; [`MainThreadMarker`] proves it at compile time.

use core::ffi::c_void;
use core::ptr::NonNull;

use objc2::rc::Retained;
use objc2::{ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSPanel, NSScreen,
    NSView, NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowStyleMask,
    NSWorkspace,
};
use objc2_core_graphics::{CGWindowLevelForKey, CGWindowLevelKey};
use objc2_foundation::{NSObjectProtocol, NSProcessInfo, NSRect};
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use serde::Serialize;

use crate::policy::{self, OverlayLevel};

/// The window level the system uses for `level`.
#[must_use]
pub fn resolve_level(level: OverlayLevel) -> isize {
    CGWindowLevelForKey(CGWindowLevelKey(level.cg_key())) as isize
}

/// Makes this process a background agent: no Dock icon, no menu bar of its own, never the frontmost app unless the
/// user clicks one of its normal windows. The menu-bar agent sets the same through `LSUIElement`.
pub fn become_accessory(mtm: MainThreadMarker) {
    NSApplication::sharedApplication(mtm)
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
}

fn style_overlay(window: &NSWindow, level: OverlayLevel) {
    window.setLevel(resolve_level(level));
    window.setCollectionBehavior(NSWindowCollectionBehavior(policy::behavior::OVERLAY));
    window.setIgnoresMouseEvents(true);
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    window.setHasShadow(false);
    window.setHidesOnDeactivate(false);
    window.setExcludedFromWindowsMenu(true);
    window.setAnimationBehavior(NSWindowAnimationBehavior(policy::ANIMATION_NONE));
}

/// A full-screen, borderless, transparent, click-through `NSPanel` that never activates the app (ADR-0110).
///
/// The content view gets a layer; wgpu's Metal backend adds its `CAMetalLayer` to it through
/// [`OverlayPanel::raw_handles`].
#[derive(Debug)]
pub struct OverlayPanel {
    panel: Retained<NSPanel>,
    view: Retained<NSView>,
    level: OverlayLevel,
}

impl OverlayPanel {
    /// Creates the panel over the whole of `screen` (menu bar and Dock area included), hidden.
    #[must_use]
    pub fn new(mtm: MainThreadMarker, screen: &NSScreen, level: OverlayLevel) -> Self {
        let frame = screen.frame();
        let style =
            NSWindowStyleMask(policy::style::BORDERLESS | policy::style::NONACTIVATING_PANEL);
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        // SAFETY: the panel is owned by `Retained` here, so AppKit must not release it a second time on close.
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setFloatingPanel(true);
        panel.setBecomesKeyOnlyIfNeeded(true);
        style_overlay(&panel, level);
        panel.setFrame_display(frame, false);
        let view = panel.contentView().expect("a new panel has a content view");
        view.setWantsLayer(true);
        Self { panel, view, level }
    }

    /// Display and window handles for `wgpu::SurfaceTargetUnsafe::RawHandle`. They stay valid while `self` lives.
    #[must_use]
    pub fn raw_handles(&self) -> (RawDisplayHandle, RawWindowHandle) {
        let ptr = NonNull::from(&*self.view).cast::<c_void>();
        (
            RawDisplayHandle::AppKit(AppKitDisplayHandle::new()),
            RawWindowHandle::AppKit(AppKitWindowHandle::new(ptr)),
        )
    }

    /// The content view, for [`crate::link::DisplayLink::new`].
    #[must_use]
    pub fn ns_view(&self) -> NonNull<c_void> {
        NonNull::from(&*self.view).cast::<c_void>()
    }

    /// Orders the panel in without activating the app or making the panel key.
    pub fn show(&self) {
        self.panel.orderFrontRegardless();
    }

    pub fn hide(&self) {
        self.panel.orderOut(None);
    }

    /// The panel's size in pixels (points × backing scale).
    #[must_use]
    pub fn pixel_size(&self) -> [u32; 2] {
        let f = self.panel.frame();
        let s = self.panel.backingScaleFactor();
        [
            (f.size.width * s).round() as u32,
            (f.size.height * s).round() as u32,
        ]
    }

    #[must_use]
    pub fn is_key(&self) -> bool {
        self.panel.isKeyWindow()
    }

    #[must_use]
    pub fn info(&self) -> WindowInfo {
        WindowInfo::of(&self.panel, self.level)
    }
}

/// Styles a window that someone else created (winit's `NSWindow` in the daemon) the same way as
/// [`OverlayPanel`], minus the non-activating panel bit, which only `NSPanel` has. `ns_view` is the
/// `AppKitWindowHandle::ns_view` pointer. Call before the window is first shown.
///
/// # Errors
/// When the view is not in a window.
///
/// # Safety
/// `ns_view` must point to a live `NSView` and this must run on the main thread.
pub unsafe fn make_overlay(
    ns_view: NonNull<c_void>,
    level: OverlayLevel,
) -> Result<WindowInfo, String> {
    // SAFETY: the caller guarantees a live NSView.
    let view: &NSView = unsafe { ns_view.cast::<NSView>().as_ref() };
    let window = view.window().ok_or("the view is not in a window")?;
    style_overlay(&window, level);
    Ok(WindowInfo::of(&window, level))
}

/// Orders the window of `ns_view` in without activating the app. winit's `set_visible(true)` calls
/// `makeKeyAndOrderFront:`, which would take focus.
///
/// # Safety
/// As [`make_overlay`].
pub unsafe fn show_without_activating(ns_view: NonNull<c_void>) {
    // SAFETY: the caller guarantees a live NSView.
    let view: &NSView = unsafe { ns_view.cast::<NSView>().as_ref() };
    if let Some(w) = view.window() {
        w.orderFrontRegardless();
    }
}

/// Process id of the frontmost app (the one with keyboard focus), or -1 when there is none.
#[must_use]
pub fn frontmost_pid() -> i32 {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map_or(-1, |a| a.processIdentifier())
}

/// Name of the frontmost app, for reports.
#[must_use]
pub fn frontmost_name() -> Option<String> {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .and_then(|a| a.localizedName())
        .map(|n| n.to_string())
}

/// System Settings → Accessibility → Display → "Reduce motion". When it is on, the host plays
/// `vutric_overlay::effects::Fling::still` instead of the chosen effect (docs/effects.md).
#[must_use]
pub fn reduce_motion() -> bool {
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

/// Whether this process is the active app. The overlay must keep this false.
#[must_use]
pub fn app_is_active(mtm: MainThreadMarker) -> bool {
    NSApplication::sharedApplication(mtm).isActive()
}

/// The settings a window really has, read back from `AppKit` (the macOS counterpart of `ex_style` on Windows).
#[derive(Debug, Clone, Serialize)]
pub struct WindowInfo {
    pub requested_level: &'static str,
    pub level: isize,
    pub style_mask: usize,
    pub collection_behavior: usize,
    pub ignores_mouse_events: bool,
    pub opaque: bool,
    pub is_panel: bool,
}

impl WindowInfo {
    fn of(window: &NSWindow, level: OverlayLevel) -> Self {
        Self {
            requested_level: level.name(),
            level: window.level(),
            style_mask: window.styleMask().0,
            collection_behavior: window.collectionBehavior().0,
            ignores_mouse_events: window.ignoresMouseEvents(),
            opaque: window.isOpaque(),
            is_panel: window.isKindOfClass(NSPanel::class()),
        }
    }
}

/// One display as `AppKit` reports it.
#[derive(Debug, Clone, Serialize)]
pub struct ScreenInfo {
    pub name: String,
    pub points: [f64; 2],
    pub scale: f64,
    /// `maximumFramesPerSecond`: 120 on `ProMotion` panels, else usually 60.
    pub max_fps: i64,
}

#[must_use]
pub fn screens(mtm: MainThreadMarker) -> Vec<ScreenInfo> {
    NSScreen::screens(mtm)
        .iter()
        .map(|s| {
            let f: NSRect = s.frame();
            ScreenInfo {
                name: s.localizedName().to_string(),
                points: [f.size.width, f.size.height],
                scale: s.backingScaleFactor(),
                max_fps: s.maximumFramesPerSecond() as i64,
            }
        })
        .collect()
}

/// "Version 15.4 (Build 24E248)" style string from `NSProcessInfo`.
#[must_use]
pub fn os_version() -> String {
    NSProcessInfo::processInfo()
        .operatingSystemVersionString()
        .to_string()
}

/// The screen with the menu bar.
#[must_use]
pub fn main_screen(mtm: MainThreadMarker) -> Option<Retained<NSScreen>> {
    NSScreen::mainScreen(mtm).or_else(|| NSScreen::screens(mtm).firstObject())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_bits_match_the_sdk() {
        assert_eq!(
            NSWindowStyleMask(policy::style::NONACTIVATING_PANEL),
            NSWindowStyleMask::NonactivatingPanel
        );
        assert_eq!(
            NSWindowStyleMask(policy::style::BORDERLESS),
            NSWindowStyleMask::Borderless
        );
        let b = NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::CanJoinAllApplications;
        assert_eq!(NSWindowCollectionBehavior(policy::behavior::OVERLAY), b);
        assert_eq!(
            NSWindowAnimationBehavior(policy::ANIMATION_NONE),
            NSWindowAnimationBehavior::None
        );
    }

    #[test]
    fn levels_match_core_graphics() {
        for l in OverlayLevel::ALL {
            assert_eq!(resolve_level(l), l.documented_level(), "{l}");
        }
    }
}
