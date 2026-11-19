//! What the daemon needs from the desktop it runs on, as traits, so each platform plugs in its own crate:
//! `vutric-host-win` (W2), `vutric-host-mac` (W5), `vutric-host-wlr` (W4). The daemon picks one implementation per
//! target at start and never calls a platform API outside these traits and the overlay window (`gpu.rs`).

/// The focus-free window that shows one instant card (ADR-0032). It must never take keyboard focus, must take
/// clicks, and reports them in card pixels (top-left origin, the space of `Rendered::buttons`).
pub trait CardSurface {
    /// Shows `bgra` (premultiplied, `width * height * 4` bytes) near the corner of the screen under the pointer.
    fn show(&mut self, width: u32, height: u32, bgra: &[u8]) -> Result<(), String>;
    fn hide(&mut self);
    /// Device pixels per logical pixel where the card will appear.
    fn scale(&self) -> f32;
}

/// Desktop services that are one call each.
pub trait Desktop {
    /// Puts `text` on the clipboard.
    fn copy_text(&self, text: &str) -> Result<(), String>;
    /// Starts or stops the looping "find my computer" sound (ADR-0069).
    fn ring(&self, on: bool);
}

/// A desktop with none of the services (a platform whose host crate is not wired yet): every call is a no-op and
/// the clipboard says why.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoDesktop;

impl Desktop for NoDesktop {
    fn copy_text(&self, _: &str) -> Result<(), String> {
        Err("no clipboard on this platform yet".into())
    }

    fn ring(&self, _: bool) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_desktop_refuses_the_clipboard_and_ignores_the_ring() {
        let d = NoDesktop;
        assert!(d.copy_text("x").is_err());
        d.ring(true);
    }
}
