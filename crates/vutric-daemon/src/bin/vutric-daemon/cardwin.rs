//! Draws the [`Deck`]'s card into the host's focus-free card window ([`CardSurface`]) and turns clicks into
//! `UiEvent::Card`. One surface per platform: Windows (`vutric-host-win`), macOS (`vutric-host-mac`, behind the
//! `mac-cards` feature until W5's `CardPanel` is on the base branch); Linux plugs in with W4's host crate.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use vutric_daemon::cards::Deck;
use vutric_daemon::host::CardSurface;
use vutric_overlay::card::{self, Button, Fonts, Rendered};
use vutric_overlay::palette::Palette;
use winit::event_loop::EventLoopProxy;

use crate::UiEvent;

type Hits = Rc<RefCell<Vec<(Button, [f32; 4])>>>;

#[cfg_attr(
    not(any(windows, all(target_os = "macos", feature = "mac-cards"))),
    allow(dead_code, reason = "no card window on this platform yet")
)]
fn hit(hits: &Hits, x: f32, y: f32) -> Option<Button> {
    hits.borrow()
        .iter()
        .find(|(_, r)| x >= r[0] && x < r[2] && y >= r[1] && y < r[3])
        .map(|(b, _)| *b)
}

pub struct CardWin {
    surface: Box<dyn CardSurface>,
    hits: Hits,
    /// Loaded on the first card, so an idle daemon never holds the font files (about 2 MB).
    fonts: Option<Fonts>,
    palette: Palette,
}

impl std::fmt::Debug for CardWin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CardWin").finish_non_exhaustive()
    }
}

#[cfg(windows)]
struct WinCards(vutric_host_win::card::CardWindow);

#[cfg(windows)]
impl CardSurface for WinCards {
    fn show(&mut self, width: u32, height: u32, bgra: &[u8]) -> Result<(), String> {
        let at = vutric_host_win::card::placement(width, height);
        self.0.show(width, height, bgra, at)
    }

    fn hide(&mut self) {
        self.0.hide();
    }

    fn scale(&self) -> f32 {
        vutric_host_win::card::target_scale()
    }
}

#[cfg(windows)]
fn surface(proxy: EventLoopProxy<UiEvent>, hits: &Hits) -> Result<Box<dyn CardSurface>, String> {
    use vutric_host_win::card::{CardWindow, Mouse};
    let h = Rc::clone(hits);
    let window = CardWindow::new(move |m| match m {
        Mouse::Move(x, y) => hit(&h, x, y).is_some(),
        Mouse::Up(x, y) => {
            if let Some(b) = hit(&h, x, y) {
                let _ = proxy.send_event(UiEvent::Card(b));
            }
            false
        }
        Mouse::Leave => false,
    })?;
    Ok(Box::new(WinCards(window)))
}

#[cfg(all(target_os = "macos", feature = "mac-cards"))]
struct MacCards(vutric_host_mac::card::CardPanel);

#[cfg(all(target_os = "macos", feature = "mac-cards"))]
impl CardSurface for MacCards {
    fn show(&mut self, width: u32, height: u32, bgra: &[u8]) -> Result<(), String> {
        let mtm = objc2::MainThreadMarker::new().ok_or("cards live on the main thread")?;
        let scale = vutric_host_mac::card::target_scale(mtm);
        let at = vutric_host_mac::card::placement(mtm, width, height, scale, 16.0);
        self.0
            .show(width, height, bgra, at)
            .map_err(|e| e.to_string())
    }

    fn hide(&mut self) {
        self.0.hide();
    }

    fn scale(&self) -> f32 {
        objc2::MainThreadMarker::new()
            .map_or(1.0, |mtm| vutric_host_mac::card::target_scale(mtm) as f32)
    }
}

#[cfg(all(target_os = "macos", feature = "mac-cards"))]
fn surface(proxy: EventLoopProxy<UiEvent>, hits: &Hits) -> Result<Box<dyn CardSurface>, String> {
    let mtm = objc2::MainThreadMarker::new().ok_or("cards must be created on the main thread")?;
    let h = Rc::clone(hits);
    // CardPanel reports mouse-up in card pixels (top-left origin, backing scale): the space of `Rendered::buttons`.
    let panel = vutric_host_mac::card::CardPanel::new(
        mtm,
        vutric_host_mac::OverlayLevel::default(),
        move |x, y| {
            if let Some(b) = hit(&h, x, y) {
                let _ = proxy.send_event(UiEvent::Card(b));
            }
        },
    );
    Ok(Box::new(MacCards(panel)))
}

#[cfg(not(any(windows, all(target_os = "macos", feature = "mac-cards"))))]
fn surface(_: EventLoopProxy<UiEvent>, _: &Hits) -> Result<Box<dyn CardSurface>, String> {
    Err("no card window on this platform yet".into())
}

impl CardWin {
    pub fn new(proxy: EventLoopProxy<UiEvent>) -> Result<Self, String> {
        let hits: Hits = Rc::default();
        Ok(Self {
            surface: surface(proxy, &hits)?,
            hits,
            fonts: None,
            palette: Palette::vutric(),
        })
    }

    /// Shows the deck's current card, or hides the window. Returns how long rasterising and presenting took.
    pub fn sync(&mut self, deck: &Deck) -> std::time::Duration {
        let started = Instant::now();
        let Some(c) = deck.card(card::qr) else {
            self.hits.borrow_mut().clear();
            self.surface.hide();
            return started.elapsed();
        };
        let scale = self.surface.scale();
        let fonts = self.fonts.get_or_insert_with(Fonts::system);
        let r: Rendered = card::render(&c, fonts, &self.palette, scale);
        if let Err(e) = self.surface.show(r.width, r.height, &r.bgra) {
            vutric_net::say(format!("card: {e}"));
        }
        *self.hits.borrow_mut() = r.buttons;
        started.elapsed()
    }
}
