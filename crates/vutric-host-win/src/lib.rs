//! Turns an ordinary top-level window into a Vutric overlay on Windows 10/11:
//!
//! - `WS_EX_LAYERED | WS_EX_TRANSPARENT`: every mouse event falls through to the windows below.
//! - `WS_EX_NOACTIVATE`: clicking, showing or updating it never takes keyboard focus.
//! - `WS_EX_TOOLWINDOW`: no taskbar button, no Alt+Tab entry.
//! - `HWND_TOPMOST`: above normal windows, so the effect is visible over whatever the user is doing.
//! - DWM transitions disabled so showing the window cannot flash a fade-in.
//!
//! Pixels themselves are presented by wgpu through a `DirectComposition` visual (per-pixel alpha); see
//! `docs/decisions/0004-windows-overlay-composition.md`.

#[cfg(windows)]
pub mod card;
#[cfg(windows)]
pub mod desk;

#[cfg(windows)]
mod imp {
    use core::ffi::c_void;

    use windows::Win32::Foundation::{COLORREF, HWND};
    use windows::Win32::Graphics::Dwm::{
        DWMWA_EXCLUDED_FROM_PEEK, DWMWA_TRANSITIONS_FORCEDISABLED, DwmSetWindowAttribute,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetForegroundWindow, GetWindowLongPtrW, HWND_TOPMOST, LWA_ALPHA,
        SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, ShowWindow, WS_EX_APPWINDOW,
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    };

    /// Extended styles applied by [`make_overlay`].
    pub const OVERLAY_EX_STYLE: u32 = WS_EX_LAYERED.0
        | WS_EX_TRANSPARENT.0
        | WS_EX_NOACTIVATE.0
        | WS_EX_TOOLWINDOW.0
        | WS_EX_TOPMOST.0;

    fn hwnd(raw: isize) -> HWND {
        HWND(raw as *mut c_void)
    }

    /// Applies the overlay styles. Call before the window is first shown.
    ///
    /// # Errors
    /// Returns the Win32 error text if any call fails; the window is then not safe to show.
    pub fn make_overlay(raw_hwnd: isize) -> Result<(), String> {
        let h = hwnd(raw_hwnd);
        // SAFETY: `h` is a live top-level window owned by this thread (guaranteed by the caller via winit).
        unsafe {
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
            let ex = (ex | OVERLAY_EX_STYLE) & !WS_EX_APPWINDOW.0;
            // Style words are 32-bit bit sets; reinterpret, never sign-extend arithmetic.
            SetWindowLongPtrW(h, GWL_EXSTYLE, ex.cast_signed() as isize);

            // A layered window is invisible until its attributes are set; 255 = fully opaque *window*,
            // per-pixel transparency still comes from the composition swapchain.
            SetLayeredWindowAttributes(h, COLORREF(0), 255, LWA_ALPHA)
                .map_err(|e| e.to_string())?;

            let on: i32 = 1;
            let on_ptr = (&raw const on).cast::<c_void>();
            let size = size_of::<i32>() as u32;
            DwmSetWindowAttribute(h, DWMWA_TRANSITIONS_FORCEDISABLED, on_ptr, size)
                .map_err(|e| e.to_string())?;
            DwmSetWindowAttribute(h, DWMWA_EXCLUDED_FROM_PEEK, on_ptr, size)
                .map_err(|e| e.to_string())?;

            SetWindowPos(
                h,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Shows the window without activating it. `winit::Window::set_visible(true)` would activate it.
    pub fn show_without_activating(raw_hwnd: isize) {
        // SAFETY: see `make_overlay`. The return value is the previous visibility, not an error.
        unsafe {
            let _ = ShowWindow(hwnd(raw_hwnd), SW_SHOWNOACTIVATE);
        }
    }

    /// The window that currently has keyboard focus, used by the spike to prove we never take it.
    #[must_use]
    pub fn foreground_window() -> isize {
        // SAFETY: no preconditions.
        unsafe { GetForegroundWindow().0 as isize }
    }

    /// Current extended style bits, for diagnostics.
    #[must_use]
    pub fn ex_style(raw_hwnd: isize) -> u32 {
        // SAFETY: see `make_overlay`.
        unsafe { GetWindowLongPtrW(hwnd(raw_hwnd), GWL_EXSTYLE) as u32 }
    }
}

#[cfg(windows)]
pub use imp::*;
