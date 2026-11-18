//! UI strings for the daemon's own surfaces (tray menu, toasts, overlay cards). Uzbek (Latin) is the default and
//! English the fallback (CLAUDE.md). The desktop windows carry their own catalog with the same keys.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Uz,
    En,
}

impl Lang {
    /// From an OS locale name such as `uz-Latn-UZ` or `en-US`; anything not Uzbek gets English.
    #[must_use]
    pub fn from_locale(locale: &str) -> Self {
        if locale.to_ascii_lowercase().starts_with("uz") {
            Self::Uz
        } else {
            Self::En
        }
    }
}

/// Every key, `(key, uz, en)`. Keys are stable; never reuse one for a different meaning.
const CATALOG: &[(&str, &str, &str)] = &[
    (
        "tray.tooltip",
        "Vutric — qabul qilishga tayyor",
        "Vutric — ready to receive",
    ),
    (
        "tray.tooltip.paused",
        "Vutric — to'xtatilgan",
        "Vutric — paused",
    ),
    ("tray.pair", "Telefonni ulash", "Connect a phone"),
    ("tray.settings", "Sozlamalar", "Settings"),
    ("tray.pause", "To'xtatib turish", "Pause"),
    ("tray.resume", "Davom ettirish", "Resume"),
    ("tray.quit", "Chiqish", "Quit"),
    ("toast.received.one", "{name} keldi", "{name} arrived"),
    (
        "toast.received.many",
        "{count} ta fayl keldi",
        "{count} files arrived",
    ),
    ("toast.from", "{device} dan", "from {device}"),
    ("toast.open", "Ochish", "Open"),
    (
        "toast.show_in_folder",
        "Papkada ko'rsatish",
        "Show in folder",
    ),
    ("toast.failed", "Qabul qilib bo'lmadi", "Could not receive"),
    ("card.offer.title", "Qabul qilasizmi?", "Accept?"),
    (
        "card.offer.body",
        "{device}: {count} ta fayl, {size}",
        "{device}: {count} files, {size}",
    ),
    ("card.accept", "Qabul qilish", "Accept"),
    ("card.decline", "Rad etish", "Decline"),
    ("card.landed", "Tushdi", "Landed"),
    ("card.progress.title", "Qabul qilinmoqda", "Receiving"),
    (
        "card.progress.body",
        "{done} / {total} · {percent}%",
        "{done} / {total} · {percent}%",
    ),
    ("card.pair.title", "Telefonni ulash", "Connect a phone"),
    (
        "card.pair.body",
        "Telefondagi Vutric bilan QR'ni skanerlang",
        "Scan the QR with Vutric on your phone",
    ),
    ("card.close", "Yopish", "Close"),
    (
        "firewall.public",
        "Tarmoq «Umumiy» deb belgilangan: telefon bu kompyuterni topa olmaydi. Uni «Shaxsiy» qiling.",
        "This network is marked Public, so phones cannot find this PC. Mark it Private.",
    ),
];

/// The string for `key`; the key itself if it is unknown, so a missing string is visible, never empty.
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

/// [`t`] with `{name}` placeholders filled from `args`.
#[must_use]
pub fn tf(lang: Lang, key: &str, args: &[(&str, &str)]) -> String {
    let mut s = t(lang, key).to_owned();
    for (k, v) in args {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

/// A byte count the way people read it: `840 KB`, `12,4 MB`, `3,1 GB` (Uzbek uses a decimal comma).
#[must_use]
pub fn size(lang: Lang, bytes: u64) -> String {
    let (v, unit) = match bytes {
        b if b >= 1 << 30 => (b as f64 / f64::from(1u32 << 30), "GB"),
        b if b >= 1 << 20 => (b as f64 / f64::from(1u32 << 20), "MB"),
        b => (b.div_ceil(1024) as f64, "KB"),
    };
    let text = if unit == "KB" || v >= 100.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    };
    let text = if lang == Lang::Uz {
        text.replace('.', ",")
    } else {
        text
    };
    format!("{text} {unit}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_unique_and_translated() {
        for (i, (k, uz, en)) in CATALOG.iter().enumerate() {
            assert!(!uz.is_empty() && !en.is_empty(), "{k}");
            assert!(
                CATALOG[i + 1..].iter().all(|(o, _, _)| o != k),
                "duplicate {k}"
            );
            let holes = |s: &str| s.matches('{').count();
            assert_eq!(holes(uz), holes(en), "placeholders differ in {k}");
        }
    }

    #[test]
    fn lookup_and_fallback() {
        assert_eq!(t(Lang::Uz, "tray.quit"), "Chiqish");
        assert_eq!(t(Lang::En, "tray.quit"), "Quit");
        assert_eq!(t(Lang::Uz, "no.such.key"), "no.such.key");
        assert_eq!(Lang::from_locale("uz-Latn-UZ"), Lang::Uz);
        assert_eq!(Lang::from_locale("ru-RU"), Lang::En);
        assert_eq!(
            tf(Lang::Uz, "toast.received.many", &[("count", "3")]),
            "3 ta fayl keldi"
        );
    }

    #[test]
    fn sizes() {
        assert_eq!(size(Lang::Uz, 1), "1 KB");
        assert_eq!(size(Lang::En, 860_000), "840 KB");
        assert_eq!(size(Lang::Uz, 13_000_000), "12,4 MB");
        assert_eq!(size(Lang::En, 13_000_000), "12.4 MB");
        assert_eq!(size(Lang::En, 3_300_000_000), "3.1 GB");
        assert_eq!(size(Lang::En, 300 << 20), "300 MB");
    }
}
