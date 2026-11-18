//! Draws the [`Deck`]'s card into the host's focus-free card window and turns clicks into `UiEvent::Card`.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use vutric_daemon::cards::Deck;
use vutric_overlay::card::{self, Button, Fonts, Rendered};
use vutric_overlay::palette::Palette;
use winit::event_loop::EventLoopProxy;

use crate::UiEvent;

type Hits = Rc<RefCell<Vec<(Button, [f32; 4])>>>;

pub struct CardWin {
    #[cfg(windows)]
    window: vutric_host_win::card::CardWindow,
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

impl CardWin {
    #[cfg(windows)]
    pub fn new(proxy: EventLoopProxy<UiEvent>) -> Result<Self, String> {
        use vutric_host_win::card::{CardWindow, Mouse};
        let hits: Hits = Rc::default();
        let h = Rc::clone(&hits);
        let hit = move |x: f32, y: f32| {
            h.borrow()
                .iter()
                .find(|(_, r)| x >= r[0] && x < r[2] && y >= r[1] && y < r[3])
                .map(|(b, _)| *b)
        };
        let window = CardWindow::new(move |m| match m {
            Mouse::Move(x, y) => hit(x, y).is_some(),
            Mouse::Up(x, y) => {
                if let Some(b) = hit(x, y) {
                    let _ = proxy.send_event(UiEvent::Card(b));
                }
                false
            }
            Mouse::Leave => false,
        })?;
        Ok(Self {
            window,
            hits,
            fonts: None,
            palette: Palette::vutric(),
        })
    }

    #[cfg(not(windows))]
    pub fn new(_: EventLoopProxy<UiEvent>) -> Result<Self, String> {
        Err("instant cards are Windows-only for now".into())
    }

    /// Shows the deck's current card, or hides the window. Returns how long rasterising and presenting took.
    pub fn sync(&mut self, deck: &Deck) -> std::time::Duration {
        let started = Instant::now();
        let Some(c) = deck.card(card::qr) else {
            self.hits.borrow_mut().clear();
            #[cfg(windows)]
            self.window.hide();
            return started.elapsed();
        };
        #[cfg(windows)]
        let scale = vutric_host_win::card::target_scale();
        #[cfg(not(windows))]
        let scale = 1.0;
        let fonts = self.fonts.get_or_insert_with(Fonts::system);
        let r: Rendered = card::render(&c, fonts, &self.palette, scale);
        #[cfg(windows)]
        {
            let at = vutric_host_win::card::placement(r.width, r.height);
            if let Err(e) = self.window.show(r.width, r.height, &r.bgra, at) {
                vutric_net::say(format!("card: {e}"));
            }
        }
        *self.hits.borrow_mut() = r.buttons;
        started.elapsed()
    }
}
