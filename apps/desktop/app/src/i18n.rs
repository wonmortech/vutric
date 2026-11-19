//! The window texts, Uzbek Latin first (ʻ in oʻ/gʻ, as on the phone), English when asked for.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Uz,
    En,
}

impl Lang {
    /// `VUTRIC_LANG` (the daemon passes its own choice when it opens a window), then `LANG`; Uzbek by default.
    #[must_use]
    pub fn detect() -> Self {
        let l = std::env::var("VUTRIC_LANG")
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default();
        if l.to_ascii_lowercase().starts_with("en") {
            Self::En
        } else {
            Self::Uz
        }
    }
}

/// (key, Uzbek, English).
const CATALOG: &[(&str, &str, &str)] = &[
    ("nav.pair", "Telefonni ulash", "Connect a phone"),
    ("nav.devices", "Qurilmalar", "Devices"),
    ("nav.history", "Tarix", "History"),
    ("nav.send", "Yuborish", "Send"),
    ("nav.settings", "Sozlamalar", "Settings"),
    (
        "pair.title",
        "Telefondagi Vutric bilan QR'ni skanerlang",
        "Scan the QR with Vutric on your phone",
    ),
    (
        "pair.hint",
        "Telefon ham shu Wi-Fi'da boʻlsin. QR bir marta ishlaydi va 10 daqiqada eskiradi.",
        "Keep the phone on the same Wi-Fi. The QR works once and expires in 10 minutes.",
    ),
    ("pair.link", "Havola", "Link"),
    ("pair.left", "Qoldi", "Left"),
    ("pair.expired", "QR eskirdi", "The QR expired"),
    ("pair.renew", "Yangi QR", "New QR"),
    ("pair.done", "Ulandi", "Connected"),
    (
        "pair.starting",
        "Vutric ishga tushmoqda…",
        "Vutric is starting…",
    ),
    (
        "pair.code",
        "Raqamlarni telefonda ham tekshiring",
        "Check the digits on the phone too",
    ),
    (
        "err.daemon",
        "Vutric fon xizmati ishlamayapti",
        "The Vutric background service is not running",
    ),
];

#[must_use]
pub fn t(lang: Lang, key: &str) -> &'static str {
    CATALOG
        .iter()
        .find(|(k, ..)| *k == key)
        .map_or("", |(_, uz, en)| match lang {
            Lang::Uz => uz,
            Lang::En => en,
        })
}

/// `m:ss` for a countdown.
#[must_use]
pub fn clock(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_both_languages_are_filled() {
        let mut keys: Vec<_> = CATALOG.iter().map(|(k, ..)| *k).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), CATALOG.len());
        assert!(
            CATALOG
                .iter()
                .all(|(_, uz, en)| !uz.is_empty() && !en.is_empty())
        );
        assert!(
            CATALOG
                .iter()
                .all(|(_, uz, _)| !uz.contains("o'") && !uz.contains("g'")),
            "use the okina"
        );
    }

    #[test]
    fn countdown_reads_minutes_and_seconds() {
        assert_eq!(clock(600), "10:00");
        assert_eq!(clock(61), "1:01");
    }
}
