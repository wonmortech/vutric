//! Instant cards on Linux (ADR-0093 §1): the same CPU card model as Windows (ADR-0032). The daemon rasterises a
//! card with `vutric_overlay::card::render` (premultiplied BGRA plus button rectangles) and this module turns it
//! into what the Linux window needs:
//!
//! - layer-shell desktops: [`to_surface`], a [`ToSurface::Card`] for `vutric-surface`, which shows it on its own
//!   `overlay`-layer surface with `keyboard_interactivity = none` (clicks without focus);
//! - GNOME: [`to_gnome`], the arguments of the extension's `ShowCard`, which shows it as Shell chrome.
//!
//! Button ids are the strings [`ACCEPT`], [`DECLINE`] and [`CLOSE`]; [`button_id`] maps W2's `card::Button`
//! names onto them, and both backends send the id back (`FromSurface::Clicked`, `CardClicked`).

use crate::proto::{Hit, ToSurface};

pub const ACCEPT: &str = "accept";
pub const DECLINE: &str = "decline";
pub const CLOSE: &str = "close";

/// Largest card raster accepted (a 4× card is at most about 1800 × 900 pixels; this leaves room).
pub const MAX_SIDE: u32 = 4096;

/// A rendered card, in buffer pixels at `scale`.
#[derive(Debug, Clone, PartialEq)]
pub struct LinuxCard {
    pub width: u32,
    pub height: u32,
    /// Integer buffer scale of the output the card is on (`FromSurface::Ready::scale`).
    pub scale: i32,
    /// Premultiplied BGRA, row-major, `width * height * 4` bytes.
    pub bgra: Vec<u8>,
    /// `(id, [x0, y0, x1, y1])` in buffer pixels.
    pub buttons: Vec<(String, [f32; 4])>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardError {
    Empty,
    TooLarge { width: u32, height: u32 },
    WrongLength { expected: u64, got: usize },
    BadScale(i32),
}

impl std::fmt::Display for CardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("card has no pixels"),
            Self::TooLarge { width, height } => write!(f, "card {width}x{height} exceeds {MAX_SIDE}"),
            Self::WrongLength { expected, got } => {
                write!(f, "card raster is {got} bytes, expected {expected}")
            }
            Self::BadScale(s) => write!(f, "card scale {s} outside 1..=4"),
        }
    }
}

impl std::error::Error for CardError {}

impl LinuxCard {
    /// The same checks the surface and the extension apply, done before anything crosses a socket.
    ///
    /// # Errors
    /// When the raster is empty, too large, not `width * height * 4` bytes, or the scale is outside 1..=4.
    pub fn check(&self) -> Result<(), CardError> {
        if self.width == 0 || self.height == 0 {
            return Err(CardError::Empty);
        }
        if self.width > MAX_SIDE || self.height > MAX_SIDE {
            return Err(CardError::TooLarge {
                width: self.width,
                height: self.height,
            });
        }
        let expected = u64::from(self.width) * u64::from(self.height) * 4;
        if self.bgra.len() as u64 != expected {
            return Err(CardError::WrongLength {
                expected,
                got: self.bgra.len(),
            });
        }
        if !(1..=4).contains(&self.scale) {
            return Err(CardError::BadScale(self.scale));
        }
        Ok(())
    }

    /// The button under a point in buffer pixels.
    #[must_use]
    pub fn hit(&self, x: f32, y: f32) -> Option<&str> {
        self.buttons
            .iter()
            .find(|(_, r)| x >= r[0] && x < r[2] && y >= r[1] && y < r[3])
            .map(|(id, _)| id.as_str())
    }
}

/// W2's `vutric_overlay::card::Button` variant name (`format!("{b:?}")`) to the id the Linux backends use.
#[must_use]
pub fn button_id(debug_name: &str) -> Option<&'static str> {
    match debug_name {
        "Accept" => Some(ACCEPT),
        "Decline" => Some(DECLINE),
        "Close" => Some(CLOSE),
        _ => None,
    }
}

/// The message for `vutric-surface` (layer-shell).
///
/// # Errors
/// When [`LinuxCard::check`] fails.
pub fn to_surface(card: LinuxCard) -> Result<ToSurface, CardError> {
    card.check()?;
    Ok(ToSurface::Card {
        width: card.width,
        height: card.height,
        scale: card.scale,
        bgra: card.bgra,
        hits: card
            .buttons
            .into_iter()
            .map(|(id, rect)| Hit { id, rect })
            .collect(),
    })
}

/// The arguments of the GNOME extension's `ShowCard(u width, u height, i scale, ay bgra, a(sdddd) hits)`.
pub type GnomeCard = (u32, u32, i32, Vec<u8>, Vec<(String, f64, f64, f64, f64)>);

/// # Errors
/// When [`LinuxCard::check`] fails.
pub fn to_gnome(card: LinuxCard) -> Result<GnomeCard, CardError> {
    card.check()?;
    let hits = card
        .buttons
        .into_iter()
        .map(|(id, r)| {
            (
                id,
                f64::from(r[0]),
                f64::from(r[1]),
                f64::from(r[2]),
                f64::from(r[3]),
            )
        })
        .collect();
    Ok((card.width, card.height, card.scale, card.bgra, hits))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> LinuxCard {
        LinuxCard {
            width: 4,
            height: 2,
            scale: 2,
            bgra: vec![0; 4 * 2 * 4],
            buttons: vec![
                (ACCEPT.into(), [0.0, 0.0, 2.0, 2.0]),
                (DECLINE.into(), [2.0, 0.0, 4.0, 2.0]),
            ],
        }
    }

    #[test]
    fn a_good_card_converts_for_both_backends() {
        let ToSurface::Card {
            width,
            height,
            scale,
            bgra,
            hits,
        } = to_surface(card()).unwrap()
        else {
            panic!("not a card");
        };
        assert_eq!((width, height, scale, bgra.len()), (4, 2, 2, 32));
        assert_eq!(hits[1].id, DECLINE);
        let (w, h, s, px, hits) = to_gnome(card()).unwrap();
        assert_eq!((w, h, s, px.len()), (4, 2, 2, 32));
        assert_eq!(hits[0], (ACCEPT.to_owned(), 0.0, 0.0, 2.0, 2.0));
    }

    #[test]
    fn bad_cards_are_refused_before_the_socket() {
        let mut c = card();
        c.bgra.pop();
        assert_eq!(
            c.check(),
            Err(CardError::WrongLength {
                expected: 32,
                got: 31
            })
        );
        let mut c = card();
        c.scale = 0;
        assert_eq!(to_surface(c).unwrap_err(), CardError::BadScale(0));
        let mut c = card();
        c.width = 0;
        assert_eq!(c.check(), Err(CardError::Empty));
        let mut c = card();
        c.width = MAX_SIDE + 1;
        assert!(matches!(c.check(), Err(CardError::TooLarge { .. })));
    }

    #[test]
    fn hits_and_button_names() {
        let c = card();
        assert_eq!(c.hit(1.0, 1.0), Some(ACCEPT));
        assert_eq!(c.hit(3.9, 1.9), Some(DECLINE));
        assert_eq!(c.hit(4.0, 1.0), None);
        assert_eq!(button_id("Accept"), Some(ACCEPT));
        assert_eq!(button_id("Close"), Some(CLOSE));
        assert_eq!(button_id("Other"), None);
    }
}
