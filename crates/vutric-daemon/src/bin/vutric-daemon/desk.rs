//! The clipboard and the ring sound, per platform. Linux (W4) and macOS (W5) plug in through `vutric-host-*`;
//! until then they log and do nothing.

/// Puts `text` on the clipboard; `false` when it could not.
#[cfg(windows)]
pub fn copy(text: &str) -> bool {
    match vutric_host_win::desk::set_clipboard_text(text) {
        Ok(()) => true,
        Err(e) => {
            vutric_net::say(format!("clipboard: {e}"));
            false
        }
    }
}

/// Starts or stops the looping ring sound.
#[cfg(windows)]
pub fn ring(on: bool) {
    vutric_host_win::desk::ring_sound(on);
}

#[cfg(not(windows))]
pub fn copy(_: &str) -> bool {
    vutric_net::say("clipboard: not on this platform yet (needs-w4/needs-w5)".into());
    false
}

#[cfg(not(windows))]
pub fn ring(_: bool) {}
