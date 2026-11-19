//! The Linux first-run question (roadmap Phase 2: "asks once to enable the extension", ADR-0093 §2).
//!
//! On GNOME without a usable extension the daemon shows one notification, once per install (the daemon keeps the
//! `asked` flag in its prefs): "turn it on" calls `gnome::enable`; "later" just closes it. On X11 or a compositor
//! without layer-shell it says once that files still arrive without the effect. Nothing is ever asked twice; the
//! settings window keeps the same button for later.

use crate::desktop::OverlayPlan;
use crate::gnome::Extension;
use crate::i18n::{Lang, t};
use crate::notify::Notice;
use vutric_host_wlr::session::Unsupported;

/// What to ask at first run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    Nothing,
    EnableExtension,
    UpdateExtension,
    /// Say once that there is no effect here (the key of the explanation).
    NoEffect(&'static str),
}

/// Notification action keys.
pub const ENABLE: &str = "enable";
pub const LATER: &str = "later";

#[must_use]
pub fn ask(plan: &OverlayPlan, already_asked: bool) -> Ask {
    if already_asked {
        return Ask::Nothing;
    }
    match plan {
        // No display at all: nobody to show a notification to.
        OverlayPlan::LayerShell
        | OverlayPlan::Gnome
        | OverlayPlan::NotificationsOnly(Unsupported::NoDisplay) => Ask::Nothing,
        OverlayPlan::GnomeNeedsExtension(Extension::TooOld { .. }) => Ask::UpdateExtension,
        OverlayPlan::GnomeNeedsExtension(_) => Ask::EnableExtension,
        OverlayPlan::NotificationsOnly(Unsupported::X11) => Ask::NoEffect("linux.unsupported.x11"),
        OverlayPlan::NotificationsOnly(Unsupported::NoLayerShell) => {
            Ask::NoEffect("linux.unsupported.compositor")
        }
    }
}

/// The notification for `a`, or `None` for [`Ask::Nothing`].
#[must_use]
pub fn notice(lang: Lang, a: Ask) -> Option<Notice> {
    let n = |summary: &str, body: &str, actions: Vec<(String, String)>| Notice {
        summary: summary.to_owned(),
        body: body.to_owned(),
        default_action: actions.first().map(|(k, _)| k.clone()),
        actions,
        image: None,
        replaces: 0,
    };
    let buttons = || {
        vec![
            (
                ENABLE.to_owned(),
                t(lang, "linux.gnome.extension.enable").to_owned(),
            ),
            (
                LATER.to_owned(),
                t(lang, "linux.gnome.extension.later").to_owned(),
            ),
        ]
    };
    match a {
        Ask::Nothing => None,
        Ask::EnableExtension => Some(n(
            t(lang, "linux.gnome.extension.title"),
            t(lang, "linux.gnome.extension.body"),
            buttons(),
        )),
        Ask::UpdateExtension => Some(n(
            t(lang, "linux.gnome.extension.update.title"),
            t(lang, "linux.gnome.extension.update.body"),
            Vec::new(),
        )),
        Ask::NoEffect(key) => Some(n("Vutric", t(lang, key), Vec::new())),
    }
}

/// After "turn it on": `None` when it worked, else the follow-up notification (log out and back in).
#[must_use]
pub fn after_enable(lang: Lang, loaded: bool) -> Option<Notice> {
    (!loaded).then(|| Notice {
        summary: "Vutric".to_owned(),
        body: t(lang, "linux.gnome.extension.relogin").to_owned(),
        actions: Vec::new(),
        image: None,
        default_action: None,
        replaces: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_once_and_only_where_it_helps() {
        let missing = OverlayPlan::GnomeNeedsExtension(Extension::Missing);
        assert_eq!(ask(&missing, false), Ask::EnableExtension);
        assert_eq!(ask(&missing, true), Ask::Nothing);
        assert_eq!(
            ask(
                &OverlayPlan::GnomeNeedsExtension(Extension::TooOld { api: 0 }),
                false
            ),
            Ask::UpdateExtension
        );
        assert_eq!(ask(&OverlayPlan::Gnome, false), Ask::Nothing);
        assert_eq!(ask(&OverlayPlan::LayerShell, false), Ask::Nothing);
        assert_eq!(
            ask(&OverlayPlan::NotificationsOnly(Unsupported::X11), false),
            Ask::NoEffect("linux.unsupported.x11")
        );
        assert_eq!(
            ask(
                &OverlayPlan::NotificationsOnly(Unsupported::NoDisplay),
                false
            ),
            Ask::Nothing
        );
    }

    #[test]
    fn the_question_has_two_buttons_and_translated_text() {
        let n = notice(Lang::Uz, Ask::EnableExtension).unwrap();
        assert_eq!(n.summary, "Effektlar uchun GNOME kengaytmasini yoqing");
        assert_eq!(
            n.actions[0],
            (ENABLE.to_owned(), "Kengaytmani yoqish".to_owned())
        );
        assert_eq!(n.actions[1].0, LATER);
        assert_eq!(n.default_action.as_deref(), Some(ENABLE));
        assert!(notice(Lang::En, Ask::Nothing).is_none());
        let x11 = notice(Lang::En, Ask::NoEffect("linux.unsupported.x11")).unwrap();
        assert!(x11.body.starts_with("No effect in an X11 session"));
        assert!(x11.actions.is_empty());
    }

    #[test]
    fn a_new_login_is_asked_for_only_when_shell_did_not_load_it() {
        assert!(after_enable(Lang::Uz, true).is_none());
        assert!(
            after_enable(Lang::En, false)
                .unwrap()
                .body
                .contains("log out and back in")
        );
    }
}
