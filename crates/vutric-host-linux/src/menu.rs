//! The tray menu as data: the same four entries as the Windows tray (ADR-0031), laid out for `com.canonical.dbusmenu`
//! (`StatusNotifierItem` hosts: KDE Plasma, COSMIC, waybar, Ubuntu's `AppIndicator` extension) and mirrored by the GNOME
//! extension's panel menu. Labels arrive translated from the daemon's catalog (`tray.*` keys).

/// One menu row. `action` is what the daemon receives when it is clicked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub action: String,
    pub label: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayModel {
    pub title: String,
    pub tooltip: String,
    pub paused: bool,
    /// Rows in order; an entry whose action is empty is a separator.
    pub entries: Vec<Entry>,
}

/// Icon names installed into `hicolor` by the package.
pub const ICON: &str = "io.wonmor.Vutric";
pub const ICON_PAUSED: &str = "io.wonmor.Vutric-paused";

/// A property value in the shape dbusmenu needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prop {
    Str(String),
    Bool(bool),
}

/// A dbusmenu item: id (0 is the root), properties, children ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: i32,
    pub props: Vec<(&'static str, Prop)>,
    pub children: Vec<i32>,
}

impl TrayModel {
    /// The four standard rows. `labels` are `(pair, settings, pause_or_resume, quit)`, already translated.
    #[must_use]
    pub fn standard(
        title: &str,
        tooltip: &str,
        paused: bool,
        labels: (&str, &str, &str, &str),
    ) -> Self {
        let e = |action: &str, label: &str| Entry {
            action: action.to_owned(),
            label: label.to_owned(),
            enabled: true,
        };
        Self {
            title: title.to_owned(),
            tooltip: tooltip.to_owned(),
            paused,
            entries: vec![
                e("pair", labels.0),
                e("settings", labels.1),
                e("", ""),
                e("toggle_pause", labels.2),
                e("", ""),
                e("quit", labels.3),
            ],
        }
    }

    #[must_use]
    pub fn icon(&self) -> &'static str {
        if self.paused { ICON_PAUSED } else { ICON }
    }

    /// dbusmenu ids are the row index + 1; the root is 0.
    #[must_use]
    pub fn nodes(&self) -> Vec<Node> {
        let mut out = vec![Node {
            id: 0,
            props: vec![("children-display", Prop::Str("submenu".into()))],
            children: (1..=self.entries.len())
                .map(|i| i32::try_from(i).unwrap_or(i32::MAX))
                .collect(),
        }];
        for (i, e) in self.entries.iter().enumerate() {
            let id = i32::try_from(i + 1).unwrap_or(i32::MAX);
            let props = if e.action.is_empty() {
                vec![("type", Prop::Str("separator".into()))]
            } else {
                vec![
                    ("label", Prop::Str(escape_mnemonic(&e.label))),
                    ("enabled", Prop::Bool(e.enabled)),
                    ("visible", Prop::Bool(true)),
                ]
            };
            out.push(Node {
                id,
                props,
                children: vec![],
            });
        }
        out
    }

    /// The action behind a clicked dbusmenu id.
    #[must_use]
    pub fn action(&self, id: i32) -> Option<&str> {
        let i = usize::try_from(id).ok()?.checked_sub(1)?;
        self.entries
            .get(i)
            .map(|e| e.action.as_str())
            .filter(|a| !a.is_empty())
    }
}

/// dbusmenu treats `_` as a mnemonic marker; a literal underscore is written twice.
#[must_use]
pub fn escape_mnemonic(s: &str) -> String {
    s.replace('_', "__")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> TrayModel {
        TrayModel::standard(
            "Vutric",
            "Vutric — qabul qilishga tayyor",
            false,
            (
                "Telefonni ulash",
                "Sozlamalar",
                "To'xtatib turish",
                "Chiqish",
            ),
        )
    }

    #[test]
    fn layout_has_a_root_and_separators() {
        let n = model().nodes();
        assert_eq!(n[0].id, 0);
        assert_eq!(n[0].children, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(n[3].props, vec![("type", Prop::Str("separator".into()))]);
        assert_eq!(
            n[1].props[0],
            ("label", Prop::Str("Telefonni ulash".into()))
        );
    }

    #[test]
    fn ids_map_back_to_actions() {
        let m = model();
        assert_eq!(m.action(1), Some("pair"));
        assert_eq!(m.action(4), Some("toggle_pause"));
        assert_eq!(m.action(6), Some("quit"));
        assert_eq!(m.action(3), None);
        assert_eq!(m.action(0), None);
        assert_eq!(m.action(-1), None);
        assert_eq!(m.action(99), None);
    }

    #[test]
    fn paused_icon_and_mnemonics() {
        let mut m = model();
        assert_eq!(m.icon(), ICON);
        m.paused = true;
        assert_eq!(m.icon(), ICON_PAUSED);
        assert_eq!(escape_mnemonic("a_b"), "a__b");
    }
}
