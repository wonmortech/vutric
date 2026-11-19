//! Which instant card is on screen and what it says (ADR-0009 §1, backlog D2/D3). Pure: the bin feeds events in,
//! gets a [`Card`] (or nothing) back and draws it with the host window.
//!
//! Lifecycle:
//!
//! ```text
//! offer ──accept──▶ progress ──received──▶ landed ──3 s──▶ hidden
//!   │                  └────failed────────────────────────▶ hidden
//!   └──decline / 60 s──────────────────────────────────────▶ hidden
//! pair ──close──▶ hidden            (an offer replaces the pair card; it comes back after)
//! ask  ──yes / no / timeout──▶ hidden   (six digits, a ring, clipboard text, our own ring)
//! note ──3 s──▶ hidden                 (a mirrored notice, "paired", "found")
//! ```

use std::time::{Duration, Instant};

use vutric_overlay::card::{Button, Card};

use crate::i18n::{Lang, size, t, tf};

/// How long "Tushdi" stays.
pub const LANDED_FOR: Duration = Duration::from_secs(3);
/// An unanswered offer disappears when the network side stops waiting for it.
pub const OFFER_FOR: Duration = Duration::from_secs(60);
/// How long a mirrored notice or a short note stays.
pub const NOTE_FOR: Duration = Duration::from_secs(5);

/// A yes/no question on the card. The answer goes back with [`Answer::Ask`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Question {
    /// "Raqamlar bir xilmi?": the six digits of a pairing without the QR (ADR-0050). Body: the digits.
    Check,
    /// A trusted phone rings this computer (ADR-0069). Yes = "Topdim".
    Ringing,
    /// We ring the phone ("Telefonni topish"). Yes = stop it.
    FindPhone,
    /// Text the phone sent for this computer's clipboard (ADR-0065). Yes = copy it.
    Clipboard,
}

impl Question {
    /// How long the question waits for the user before it counts as "no".
    #[must_use]
    pub fn wait(self) -> Duration {
        match self {
            Self::Check => Duration::from_secs(120),
            Self::Ringing | Self::FindPhone => {
                Duration::from_secs(u64::from(vutric_core::ring::MAX_RING_SECS))
            }
            Self::Clipboard => Duration::from_secs(30),
        }
    }

    fn keys(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Check => ("card.check.title", "card.check.yes", "card.check.no"),
            Self::Ringing => ("card.ringing.title", "card.ringing.yes", "card.close"),
            Self::FindPhone => ("card.find.title", "card.find.yes", "card.close"),
            Self::Clipboard => ("card.clip.title", "card.clip.yes", "card.close"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Showing {
    None,
    Offer {
        device: String,
        files: u32,
        bytes: u64,
    },
    Progress {
        done: u64,
        total: u64,
    },
    Landed {
        device: String,
        files: usize,
    },
    Pair {
        link: String,
    },
    Ask {
        q: Question,
        device: String,
        body: String,
    },
    Note {
        title: String,
        body: String,
    },
}

/// What the bin must do after an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Nothing to tell the network.
    None,
    /// Answer the pending offer.
    Offer(bool),
    /// Answer a [`Question`].
    Ask(Question, bool),
    /// The user closed the pairing card: close the pairing window too.
    PairClosed,
}

#[derive(Debug)]
pub struct Deck {
    lang: Lang,
    showing: Showing,
    /// The pair card to restore once a transfer is over.
    paused_pair: Option<String>,
    deadline: Option<Instant>,
    /// Last drawn percent, so progress redraws at most 101 times per transfer.
    drawn_percent: Option<u8>,
}

impl Deck {
    #[must_use]
    pub fn new(lang: Lang) -> Self {
        Self {
            lang,
            showing: Showing::None,
            paused_pair: None,
            deadline: None,
            drawn_percent: None,
        }
    }

    /// When the bin must call [`Deck::tick`] next, if ever.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    #[must_use]
    pub fn is_visible(&self) -> bool {
        self.showing != Showing::None
    }

    fn park_pair(&mut self) {
        if let Showing::Pair { link } = &self.showing {
            self.paused_pair = Some(link.clone());
        }
    }

    fn finish(&mut self) {
        self.deadline = None;
        self.drawn_percent = None;
        self.showing = self
            .paused_pair
            .take()
            .map_or(Showing::None, |link| Showing::Pair { link });
    }

    pub fn offer(&mut self, device: &str, files: u32, bytes: u64, now: Instant) {
        self.park_pair();
        self.showing = Showing::Offer {
            device: device.to_owned(),
            files,
            bytes,
        };
        self.deadline = Some(now + OFFER_FOR);
    }

    /// Returns whether the card changed enough to be redrawn.
    pub fn progress(&mut self, done: u64, total: u64) -> bool {
        if !matches!(self.showing, Showing::Progress { .. }) {
            self.park_pair();
            self.drawn_percent = None;
        }
        self.deadline = None;
        self.showing = Showing::Progress { done, total };
        let p = percent(done, total);
        let redraw = self.drawn_percent != Some(p);
        self.drawn_percent = Some(p);
        redraw
    }

    pub fn received(&mut self, device: &str, files: usize, now: Instant) {
        self.showing = Showing::Landed {
            device: device.to_owned(),
            files,
        };
        self.drawn_percent = None;
        self.deadline = Some(now + LANDED_FOR);
    }

    pub fn failed(&mut self) {
        if matches!(
            self.showing,
            Showing::Progress { .. } | Showing::Offer { .. }
        ) {
            self.finish();
        }
    }

    /// Asks `q` about `device`. An unanswered question or offer on screen counts as "no"; that answer is returned
    /// so the bin can pass it on.
    pub fn ask(&mut self, q: Question, device: &str, body: &str, now: Instant) -> Option<Answer> {
        let replaced = match self.showing {
            Showing::Ask { q: old, .. } => Some(Answer::Ask(old, false)),
            Showing::Offer { .. } => Some(Answer::Offer(false)),
            _ => None,
        };
        self.park_pair();
        self.showing = Showing::Ask {
            q,
            device: device.to_owned(),
            body: body.to_owned(),
        };
        self.deadline = Some(now + q.wait());
        replaced
    }

    /// A short note with no buttons: a mirrored notice, "paired", "found".
    pub fn note(&mut self, title: &str, body: &str, now: Instant) {
        if matches!(
            self.showing,
            Showing::Offer { .. } | Showing::Progress { .. } | Showing::Ask { .. }
        ) {
            return;
        }
        self.park_pair();
        self.showing = Showing::Note {
            title: title.to_owned(),
            body: body.to_owned(),
        };
        self.deadline = Some(now + NOTE_FOR);
    }

    /// The question on screen, if any.
    #[must_use]
    pub fn asking(&self) -> Option<Question> {
        match self.showing {
            Showing::Ask { q, .. } => Some(q),
            _ => None,
        }
    }

    /// Takes a question off the screen without an answer (the other side answered it first).
    pub fn dismiss(&mut self, q: Question) {
        if self.asking() == Some(q) {
            self.finish();
        }
    }

    pub fn pair(&mut self, link: &str) {
        match self.showing {
            Showing::None | Showing::Pair { .. } => {
                self.showing = Showing::Pair {
                    link: link.to_owned(),
                };
                self.deadline = None;
            }
            // Never hide a live offer or transfer behind the QR: it shows up once they are done.
            _ => self.paused_pair = Some(link.to_owned()),
        }
    }

    /// A button was pressed on the visible card.
    pub fn press(&mut self, b: Button) -> Answer {
        match (&self.showing, b) {
            (Showing::Offer { bytes, .. }, Button::Accept) => {
                let total = *bytes;
                self.deadline = None;
                self.showing = Showing::Progress { done: 0, total };
                self.drawn_percent = Some(0);
                Answer::Offer(true)
            }
            (Showing::Offer { .. }, Button::Decline) => {
                self.finish();
                Answer::Offer(false)
            }
            (Showing::Pair { .. }, Button::Close) => {
                self.paused_pair = None;
                self.finish();
                Answer::PairClosed
            }
            (Showing::Ask { q, .. }, Button::Accept | Button::Decline) => {
                let a = Answer::Ask(*q, b == Button::Accept);
                self.finish();
                a
            }
            _ => Answer::None,
        }
    }

    /// Expires timed cards. `None` = nothing expired; `Some(answer)` = the card changed (redraw), with the answer
    /// owed to the network when it was an offer.
    pub fn tick(&mut self, now: Instant) -> Option<Answer> {
        match self.deadline {
            Some(d) if now >= d => {
                let answer = match self.showing {
                    Showing::Offer { .. } => Answer::Offer(false),
                    Showing::Ask { q, .. } => Answer::Ask(q, false),
                    _ => Answer::None,
                };
                self.finish();
                Some(answer)
            }
            _ => None,
        }
    }

    /// The card to draw, or `None` to hide the window. `qr` turns a link into QR modules.
    #[must_use]
    pub fn card(&self, qr: impl Fn(&str) -> Option<(Vec<bool>, usize)>) -> Option<Card> {
        let l = self.lang;
        Some(match &self.showing {
            Showing::None => return None,
            Showing::Offer {
                device,
                files,
                bytes,
            } => Card::Offer {
                title: t(l, "card.offer.title").into(),
                body: tf(
                    l,
                    "card.offer.body",
                    &[
                        ("device", device),
                        ("count", &files.to_string()),
                        ("size", &size(l, *bytes)),
                    ],
                ),
                accept: t(l, "card.accept").into(),
                decline: t(l, "card.decline").into(),
            },
            Showing::Progress { done, total } => Card::Progress {
                title: t(l, "card.progress.title").into(),
                body: tf(
                    l,
                    "card.progress.body",
                    &[
                        ("done", &size(l, *done)),
                        ("total", &size(l, *total)),
                        ("percent", &percent(*done, *total).to_string()),
                    ],
                ),
                fraction: (*total > 0).then(|| (*done as f64 / *total as f64) as f32),
                t: 0.0,
            },
            Showing::Landed { device, files } => Card::Landed {
                title: t(l, "card.landed").into(),
                body: if *files == 1 {
                    tf(l, "toast.from", &[("device", device)])
                } else {
                    format!(
                        "{} · {}",
                        tf(l, "toast.received.many", &[("count", &files.to_string())]),
                        tf(l, "toast.from", &[("device", device)])
                    )
                },
            },
            Showing::Ask { q, device, body } => {
                let (title, yes, no) = q.keys();
                Card::Offer {
                    title: t(l, title).into(),
                    body: if body.is_empty() {
                        device.clone()
                    } else {
                        format!("{device}: {body}")
                    },
                    accept: t(l, yes).into(),
                    decline: t(l, no).into(),
                }
            }
            Showing::Note { title, body } => Card::Landed {
                title: title.clone(),
                body: body.clone(),
            },
            Showing::Pair { link } => {
                let (modules, width) = qr(link).unwrap_or_default();
                Card::Pair {
                    title: t(l, "card.pair.title").into(),
                    body: t(l, "card.pair.body").into(),
                    close: t(l, "card.close").into(),
                    qr: modules,
                    qr_width: width,
                }
            }
        })
    }
}

fn percent(done: u64, total: u64) -> u8 {
    if total == 0 {
        0
    } else {
        (u128::from(done.min(total)) * 100 / u128::from(total)) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::unnecessary_wraps)] // matches the `card()` parameter
    fn qr(_: &str) -> Option<(Vec<bool>, usize)> {
        Some((vec![true; 4], 2))
    }

    #[test]
    fn accept_turns_the_offer_into_progress_then_landed_then_hidden() {
        let now = Instant::now();
        let mut d = Deck::new(Lang::Uz);
        assert!(d.card(qr).is_none());
        d.offer("Pixel 9", 3, 12_400_000, now);
        let Some(Card::Offer { body, accept, .. }) = d.card(qr) else {
            panic!()
        };
        assert_eq!(accept, "Qabul qilish");
        assert!(body.starts_with("Pixel 9: 3 ta fayl, "), "{body}");
        assert_eq!(d.press(Button::Accept), Answer::Offer(true));
        assert!(matches!(d.card(qr), Some(Card::Progress { fraction: Some(f), .. }) if f == 0.0));
        assert!(d.progress(6_200_000, 12_400_000));
        assert!(
            !d.progress(6_200_001, 12_400_000),
            "same percent, no redraw"
        );
        let Some(Card::Progress { body, .. }) = d.card(qr) else {
            panic!()
        };
        assert!(body.ends_with("50%"), "{body}");
        d.received("Pixel 9", 3, now);
        let Some(Card::Landed { title, body }) = d.card(qr) else {
            panic!()
        };
        assert_eq!(title, "Tushdi");
        assert_eq!(body, "3 ta fayl keldi · Pixel 9 dan");
        assert_eq!(d.tick(now + Duration::from_secs(2)), None);
        assert!(d.is_visible());
        assert_eq!(d.tick(now + LANDED_FOR), Some(Answer::None));
        assert!(!d.is_visible() && d.deadline().is_none());
    }

    #[test]
    fn decline_and_timeout_answer_no() {
        let now = Instant::now();
        let mut d = Deck::new(Lang::En);
        d.offer("A", 1, 1, now);
        assert_eq!(d.press(Button::Decline), Answer::Offer(false));
        assert!(!d.is_visible());
        d.offer("A", 1, 1, now);
        assert_eq!(d.tick(now + OFFER_FOR), Some(Answer::Offer(false)));
        assert!(!d.is_visible());
        assert_eq!(
            d.press(Button::Accept),
            Answer::None,
            "nothing to accept any more"
        );
    }

    #[test]
    fn the_pair_card_yields_to_a_transfer_and_comes_back() {
        let now = Instant::now();
        let mut d = Deck::new(Lang::Uz);
        d.pair("vutric://pair?x");
        assert!(matches!(d.card(qr), Some(Card::Pair { qr_width: 2, .. })));
        d.offer("A", 1, 10, now);
        assert!(matches!(d.card(qr), Some(Card::Offer { .. })));
        d.press(Button::Decline);
        assert!(matches!(d.card(qr), Some(Card::Pair { .. })), "QR is back");
        d.press(Button::Close);
        assert!(!d.is_visible());

        d.progress(1, 10);
        d.pair("vutric://pair?y");
        assert!(
            matches!(d.card(qr), Some(Card::Progress { .. })),
            "QR waits behind the transfer"
        );
        d.failed();
        assert!(matches!(d.card(qr), Some(Card::Pair { .. })));
    }

    #[test]
    fn questions_answer_yes_no_or_time_out() {
        let now = Instant::now();
        let mut d = Deck::new(Lang::Uz);
        assert_eq!(d.ask(Question::Check, "Pixel", "123 456", now), None);
        let Some(Card::Offer {
            title,
            body,
            accept,
            ..
        }) = d.card(qr)
        else {
            panic!()
        };
        assert_eq!(title, t(Lang::Uz, "card.check.title"));
        assert_eq!(body, "Pixel: 123 456");
        assert_eq!(accept, t(Lang::Uz, "card.check.yes"));
        assert_eq!(d.press(Button::Accept), Answer::Ask(Question::Check, true));
        assert!(!d.is_visible());

        d.ask(Question::Clipboard, "Pixel", "salom", now);
        assert_eq!(
            d.tick(now + Question::Clipboard.wait()),
            Some(Answer::Ask(Question::Clipboard, false))
        );

        d.ask(Question::FindPhone, "Pixel", "", now);
        assert_eq!(
            d.ask(Question::Ringing, "Pixel", "", now),
            Some(Answer::Ask(Question::FindPhone, false)),
            "a newer question answers the older one with no"
        );
        d.dismiss(Question::Ringing);
        assert!(!d.is_visible());
    }

    #[test]
    fn notes_never_hide_a_question_and_closing_the_qr_is_reported() {
        let now = Instant::now();
        let mut d = Deck::new(Lang::Uz);
        d.offer("A", 1, 10, now);
        d.note("Telegram", "salom", now);
        assert!(matches!(d.card(qr), Some(Card::Offer { .. })));
        d.press(Button::Decline);
        d.note("Telegram", "salom", now);
        assert!(matches!(d.card(qr), Some(Card::Landed { .. })));
        assert_eq!(d.tick(now + NOTE_FOR), Some(Answer::None));
        d.pair("vutric://pair?x");
        assert_eq!(d.press(Button::Close), Answer::PairClosed);
    }

    #[test]
    fn auto_accepted_transfer_shows_progress_without_an_offer() {
        let mut d = Deck::new(Lang::Uz);
        assert!(d.progress(0, 0));
        assert!(matches!(
            d.card(qr),
            Some(Card::Progress { fraction: None, .. })
        ));
        assert_eq!(percent(u64::MAX, u64::MAX), 100);
        assert_eq!(percent(5, 0), 0);
    }
}
