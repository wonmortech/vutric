//! Strings only the Linux pieces show. Same shape and rules as `vutric-daemon::i18n` (Uzbek Latin default, English
//! fallback, stable keys); the GNOME extension carries the same keys in `apps/gnome-extension/.../i18n.js`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Uz,
    En,
}

impl Lang {
    /// From `LC_ALL`, `LC_MESSAGES` or `LANG` (`uz_UZ.UTF-8@latin`, `en_US.UTF-8`); anything not Uzbek is English.
    #[must_use]
    pub fn from_locale(locale: &str) -> Self {
        if locale.to_ascii_lowercase().starts_with("uz") {
            Self::Uz
        } else {
            Self::En
        }
    }

    #[must_use]
    pub fn from_env() -> Self {
        let first = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .find(|v| !v.is_empty());
        first.map_or(Self::Uz, |v| {
            if v == "C" || v.starts_with("C.") || v == "POSIX" {
                Self::Uz
            } else {
                Self::from_locale(&v)
            }
        })
    }
}

/// `(key, uz, en)`.
pub const CATALOG: &[(&str, &str, &str)] = &[
    (
        "linux.send_with",
        "Vutric bilan yuborish",
        "Send with Vutric",
    ),
    (
        "linux.gnome.extension.title",
        "Effektlar uchun GNOME kengaytmasini yoqing",
        "Turn on the GNOME extension for effects",
    ),
    (
        "linux.gnome.extension.body",
        "Fayllar baribir qabul qilinadi. Uchish effekti va kartochkalar \"Vutric\" GNOME kengaytmasi bilan ishlaydi.",
        "Files are still received. The fling effect and the cards need the \"Vutric\" GNOME extension.",
    ),
    (
        "linux.gnome.extension.enable",
        "Kengaytmani yoqish",
        "Turn on the extension",
    ),
    (
        "linux.unsupported.x11",
        "X11 seansida effekt ko'rsatilmaydi, fayllar qabul qilinadi",
        "No effect in an X11 session; files are still received",
    ),
    (
        "linux.unsupported.compositor",
        "Bu ish stoli effektni qo'llab-quvvatlamaydi, fayllar qabul qilinadi",
        "This desktop cannot show the effect; files are still received",
    ),
];

/// The string for `key`, or the key itself when it is missing (visible in review, never a crash).
#[must_use]
pub fn t(lang: Lang, key: &str) -> &str {
    CATALOG
        .iter()
        .find(|(k, _, _)| *k == key)
        .map_or(key, |(_, uz, en)| match lang {
            Lang::Uz => uz,
            Lang::En => en,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_filled() {
        for (i, (k, uz, en)) in CATALOG.iter().enumerate() {
            assert!(k.starts_with("linux."), "{k}");
            assert!(!uz.is_empty() && !en.is_empty(), "{k}");
            assert!(
                !uz.chars().any(|c| ('\u{400}'..='\u{4ff}').contains(&c)),
                "{k}: Uzbek is Latin"
            );
            assert!(CATALOG[i + 1..].iter().all(|(o, _, _)| o != k), "{k} twice");
        }
    }

    #[test]
    fn lookup_and_fallback() {
        assert_eq!(t(Lang::Uz, "linux.send_with"), "Vutric bilan yuborish");
        assert_eq!(t(Lang::En, "linux.send_with"), "Send with Vutric");
        assert_eq!(t(Lang::En, "missing.key"), "missing.key");
        assert_eq!(Lang::from_locale("uz_UZ.UTF-8@latin"), Lang::Uz);
        assert_eq!(Lang::from_locale("en_GB.UTF-8"), Lang::En);
    }
}
