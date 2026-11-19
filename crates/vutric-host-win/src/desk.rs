//! Small desktop services the daemon needs from Windows: the clipboard (ADR-0065) and the ring sound (ADR-0069).

use core::ffi::c_void;

use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Media::Audio::{
    PlaySoundW, SND_ALIAS, SND_ASYNC, SND_FLAGS, SND_LOOP, SND_NODEFAULT,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::core::{PCWSTR, w};

/// `CF_UNICODETEXT`.
const UNICODE_TEXT: u32 = 13;

/// Puts `text` on the clipboard as Unicode text.
///
/// # Errors
/// The Win32 error text when the clipboard is held by another program or memory runs out.
pub fn set_clipboard_text(text: &str) -> Result<(), String> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * size_of::<u16>();
    // SAFETY: the clipboard is opened for this thread and always closed below; the global block is `bytes` long
    // and written only through the pointer `GlobalLock` returned for it.
    unsafe {
        OpenClipboard(Some(HWND::default())).map_err(|e| e.to_string())?;
        let r = (|| {
            EmptyClipboard().map_err(|e| e.to_string())?;
            let mem = GlobalAlloc(GMEM_MOVEABLE, bytes).map_err(|e| e.to_string())?;
            let dst = GlobalLock(mem).cast::<u16>();
            if dst.is_null() {
                return Err("GlobalLock failed".to_owned());
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
            let _ = GlobalUnlock(mem);
            // On success the clipboard owns the block; it must not be freed here.
            SetClipboardData(UNICODE_TEXT, Some(HANDLE(mem.0.cast::<c_void>())))
                .map(|_| ())
                .map_err(|e| e.to_string())
        })();
        let _ = CloseClipboard();
        r
    }
}

/// Starts (`true`) or stops the looping ring sound. A start while ringing restarts it.
pub fn ring_sound(on: bool) {
    // SAFETY: the alias is a static wide string; a null sound stops whatever this process is playing.
    unsafe {
        if on {
            let _ = PlaySoundW(
                w!("SystemExclamation"),
                None,
                SND_ALIAS | SND_ASYNC | SND_LOOP | SND_NODEFAULT,
            );
        } else {
            let _ = PlaySoundW(PCWSTR::null(), None, SND_FLAGS(0));
        }
    }
}
