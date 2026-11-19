//! The small window that shows an instant card (ADR-0009 §1).
//!
//! Unlike the effect surface it takes clicks, but it still never takes focus: `WS_EX_NOACTIVATE` plus
//! `MA_NOACTIVATE` on `WM_MOUSEACTIVATE`, so pressing "Qabul qilish" leaves the caret where the user was typing.
//! Pixels are pushed with `UpdateLayeredWindow` from a premultiplied BGRA buffer: per-pixel alpha, no GPU, no
//! redraw messages, and a hidden card costs one idle `HWND`.
//!
//! The window lives on the thread that created it; that thread's message loop (winit's, in the daemon) dispatches
//! its messages, so the mouse callback runs on the UI thread.

use core::ffi::c_void;
use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_TRANSITIONS_FORCEDISABLED, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC,
    GetMonitorInfoW, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, ReleaseDC,
    SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetCursorPos, GetWindowLongPtrW,
    HTCLIENT, HWND_TOPMOST, IDC_ARROW, IDC_HAND, LoadCursorW, MA_NOACTIVATE, RegisterClassExW,
    SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetCursor,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_NCDESTROY, WM_SETCURSOR, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

/// A pointer event in window pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mouse {
    Move(f32, f32),
    Up(f32, f32),
    Leave,
}

type Callback = Box<dyn FnMut(Mouse) -> bool>;

struct State {
    on_mouse: RefCell<Callback>,
    hand: Cell<bool>,
    tracking: Cell<bool>,
}

/// One card window. Hidden until [`CardWindow::show`]; destroyed on drop.
pub struct CardWindow {
    hwnd: HWND,
    state: *mut State,
}

impl std::fmt::Debug for CardWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CardWindow")
            .field("hwnd", &self.hwnd.0)
            .finish_non_exhaustive()
    }
}

/// Where a card sits: bottom-right of the work area (taskbar excluded) of the monitor under the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
}

fn cursor_monitor() -> (RECT, u32) {
    // SAFETY: plain queries with out-parameters that live on this stack frame.
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&raw mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(mon, &raw mut info);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &raw mut dx, &raw mut dy);
        (info.rcWork, dx.max(48))
    }
}

/// The scale to render the next card at: DPI of the monitor under the cursor / 96.
#[must_use]
pub fn target_scale() -> f32 {
    cursor_monitor().1 as f32 / 96.0
}

/// Bottom-right corner of the cursor's monitor work area for a `w`×`h` card.
#[must_use]
pub fn placement(w: u32, h: u32) -> Placement {
    let (work, _) = cursor_monitor();
    Placement {
        x: work.right - w.cast_signed(),
        y: work.bottom - h.cast_signed(),
    }
}

/// `WinUser.h`; the `windows` crate files it under `UI::Controls`, a feature we do not otherwise need.
const WM_MOUSELEAVE: u32 = 0x02A3;

const CLASS: PCWSTR = w!("VutricCard");

/// `GET_X_LPARAM` / `GET_Y_LPARAM`: the signed 16-bit halves.
fn lparam_xy(l: LPARAM) -> (f32, f32) {
    let v = l.0;
    (
        f32::from((v as u16).cast_signed()),
        f32::from(((v >> 16) as u16).cast_signed()),
    )
}

fn emit(s: &State, m: Mouse) -> bool {
    s.on_mouse.try_borrow_mut().is_ok_and(|mut cb| cb(m))
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: USERDATA is 0 or the `State` installed by `new`, freed only in `Drop` after `DestroyWindow`
    // (whose WM_NCDESTROY resets it to 0).
    let state = unsafe { (GetWindowLongPtrW(h, GWLP_USERDATA) as *const State).as_ref() };
    match (msg, state) {
        (WM_MOUSEACTIVATE, _) => LRESULT(MA_NOACTIVATE.cast_signed() as isize),
        (WM_MOUSEMOVE, Some(s)) => {
            if !s.tracking.get() {
                let mut t = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: h,
                    dwHoverTime: 0,
                };
                // SAFETY: an initialised struct for a window we own.
                s.tracking
                    .set(unsafe { TrackMouseEvent(&raw mut t) }.is_ok());
            }
            let (x, y) = lparam_xy(lp);
            s.hand.set(emit(s, Mouse::Move(x, y)));
            LRESULT(0)
        }
        (WM_LBUTTONUP, Some(s)) => {
            let (x, y) = lparam_xy(lp);
            emit(s, Mouse::Up(x, y));
            LRESULT(0)
        }
        (WM_MOUSELEAVE, Some(s)) => {
            s.tracking.set(false);
            s.hand.set(false);
            emit(s, Mouse::Leave);
            LRESULT(0)
        }
        (WM_SETCURSOR, Some(s)) if (lp.0 & 0xFFFF) as u32 == HTCLIENT => {
            let id = if s.hand.get() { IDC_HAND } else { IDC_ARROW };
            // SAFETY: system cursors need no module handle and are never freed.
            unsafe {
                if let Ok(c) = LoadCursorW(None, id) {
                    SetCursor(Some(c));
                }
            }
            LRESULT(1)
        }
        (WM_NCDESTROY, _) => {
            // SAFETY: clearing our own window's USERDATA, then forwarding unchanged arguments.
            unsafe {
                SetWindowLongPtrW(h, GWLP_USERDATA, 0);
                DefWindowProcW(h, msg, wp, lp)
            }
        }
        // SAFETY: forwarding unchanged arguments.
        _ => unsafe { DefWindowProcW(h, msg, wp, lp) },
    }
}

impl CardWindow {
    /// Creates the hidden card window on the calling thread. `on_mouse` returns whether the pointer is over a
    /// button (the cursor then becomes a hand).
    ///
    /// # Errors
    /// The Win32 error text when the window cannot be created.
    pub fn new(on_mouse: impl FnMut(Mouse) -> bool + 'static) -> Result<Self, String> {
        // SAFETY: standard class registration and window creation; every pointer outlives its call.
        unsafe {
            let module = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: module.into(),
                lpszClassName: CLASS,
                ..Default::default()
            };
            // A second registration fails with "class already exists", which is fine.
            let _ = RegisterClassExW(&raw const wc);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                CLASS,
                w!("Vutric"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(module.into()),
                None,
            )
            .map_err(|e| e.to_string())?;
            let on: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_TRANSITIONS_FORCEDISABLED,
                (&raw const on).cast::<c_void>(),
                size_of::<i32>() as u32,
            );
            let state = Box::into_raw(Box::new(State {
                on_mouse: RefCell::new(Box::new(on_mouse)),
                hand: Cell::new(false),
                tracking: Cell::new(false),
            }));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
            Ok(Self { hwnd, state })
        }
    }

    /// The raw `HWND`, for tests and diagnostics.
    #[must_use]
    pub fn raw(&self) -> isize {
        self.hwnd.0 as isize
    }

    /// Shows (or replaces) the card: `bgra` is `w`×`h` premultiplied BGRA, top row first. Never activates.
    ///
    /// # Errors
    /// The Win32 error text when the bitmap cannot be created or pushed.
    pub fn show(&self, w: u32, h: u32, bgra: &[u8], at: Placement) -> Result<(), String> {
        if w == 0 || h == 0 || bgra.len() != (w as usize) * (h as usize) * 4 {
            return Err(format!(
                "card buffer is {} bytes, expected {w}x{h}x4",
                bgra.len()
            ));
        }
        // SAFETY: a DIB section of exactly `w`×`h`×32 bpp is created and filled from `bgra` (length checked
        // above), selected into a memory DC for the duration of `UpdateLayeredWindow`, then released in reverse.
        unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w.cast_signed(),
                    biHeight: -h.cast_signed(),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = core::ptr::null_mut();
            let result = CreateDIBSection(
                Some(mem),
                &raw const info,
                DIB_RGB_COLORS,
                &raw mut bits,
                None,
                0,
            )
            .map_err(|e| e.to_string())
            .and_then(|bmp| {
                core::ptr::copy_nonoverlapping(bgra.as_ptr(), bits.cast::<u8>(), bgra.len());
                let old = SelectObject(mem, HGDIOBJ(bmp.0));
                let blend = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                let pos = POINT { x: at.x, y: at.y };
                let size = SIZE {
                    cx: w.cast_signed(),
                    cy: h.cast_signed(),
                };
                let src = POINT::default();
                let r = UpdateLayeredWindow(
                    self.hwnd,
                    Some(screen),
                    Some(&raw const pos),
                    Some(&raw const size),
                    Some(mem),
                    Some(&raw const src),
                    COLORREF(0),
                    Some(&raw const blend),
                    ULW_ALPHA,
                )
                .map_err(|e| e.to_string());
                SelectObject(mem, old);
                let _ = DeleteObject(HGDIOBJ(bmp.0));
                r
            });
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            result?;
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        Ok(())
    }

    /// Hides the card; the window stays for the next one.
    pub fn hide(&self) {
        // SAFETY: our own live window.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
}

/// Measurement support: moves the cursor to screen point (`x`, `y`) and clicks the left button there, through the
/// same input queue a real mouse uses. Returns whether both button events were accepted.
#[must_use]
pub fn synthetic_click(x: i32, y: i32) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT,
        SendInput,
    };
    use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;
    let button = |flags| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    let inputs = [button(MOUSEEVENTF_LEFTDOWN), button(MOUSEEVENTF_LEFTUP)];
    // SAFETY: a stack array of initialised INPUT structs with its exact element size.
    unsafe {
        if SetCursorPos(x, y).is_err() {
            return false;
        }
        SendInput(
            &inputs,
            i32::try_from(size_of::<INPUT>()).unwrap_or(i32::MAX),
        ) == 2
    }
}

/// Measurement support: dispatches this thread's window messages for `ms` milliseconds (a card window created
/// outside an event loop still needs its clicks delivered).
pub fn pump_messages(ms: u64) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
    };
    let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < until {
        let mut msg = MSG::default();
        // SAFETY: `msg` lives on this frame; messages are only those of windows this thread owns.
        unsafe {
            while PeekMessageW(&raw mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&raw const msg);
                DispatchMessageW(&raw const msg);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

impl Drop for CardWindow {
    fn drop(&mut self) {
        // SAFETY: after `DestroyWindow` (its WM_NCDESTROY clears USERDATA) no message can reach the state, so it is
        // freed exactly once.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            drop(Box::from_raw(self.state));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lparam_coordinates_are_signed() {
        let pack =
            |x: i16, y: i16| LPARAM(((i32::from(y) << 16) | i32::from(x.cast_unsigned())) as isize);
        assert_eq!(lparam_xy(pack(12, 300)), (12.0, 300.0));
        assert_eq!(lparam_xy(pack(-5, -1)), (-5.0, -1.0));
    }

    #[test]
    fn card_window_never_activates_and_rejects_bad_buffers() {
        let w = CardWindow::new(|_| false).expect("window");
        let ex = crate::ex_style(w.raw());
        assert_ne!(ex & WS_EX_NOACTIVATE.0, 0);
        assert_ne!(ex & WS_EX_LAYERED.0, 0);
        assert_ne!(ex & WS_EX_TOOLWINDOW.0, 0);
        assert!(w.show(4, 4, &[0; 10], placement(4, 4)).is_err());
        assert!(target_scale() >= 0.5);
    }
}
