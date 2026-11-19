//! The clipboard and the ring sound through [`Desktop`], one implementation per platform. Linux (W4) and macOS (W5)
//! return [`NoDesktop`] until their host crates offer the calls (`docs/streams/w2-notes.md`, needs-w4/needs-w5).

use vutric_daemon::host::Desktop;
#[cfg(not(windows))]
use vutric_daemon::host::NoDesktop;

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
struct Win;

#[cfg(windows)]
impl Desktop for Win {
    fn copy_text(&self, text: &str) -> Result<(), String> {
        vutric_host_win::desk::set_clipboard_text(text)
    }

    fn ring(&self, on: bool) {
        vutric_host_win::desk::ring_sound(on);
    }
}

#[cfg(windows)]
fn current() -> impl Desktop {
    Win
}

#[cfg(not(windows))]
fn current() -> impl Desktop {
    NoDesktop
}

/// Puts `text` on the clipboard; `false` (logged) when it could not.
pub fn copy(text: &str) -> bool {
    match current().copy_text(text) {
        Ok(()) => true,
        Err(e) => {
            vutric_net::say(format!("clipboard: {e}"));
            false
        }
    }
}

/// Starts or stops the looping ring sound.
pub fn ring(on: bool) {
    current().ring(on);
}
