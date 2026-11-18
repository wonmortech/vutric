//! "Files arrived" notifications through `org.freedesktop.Notifications` (GNOME, KDE, COSMIC, mako, dunst), the
//! Linux side of `vutric-daemon::toast` (ADR-0091). The content is built here and tested; strings arrive
//! translated from the daemon's `toast.*` keys.

use std::path::{Path, PathBuf};

/// The desktop entry id (without `.desktop`); GNOME groups notifications and applies settings by it.
pub const DESKTOP_ENTRY: &str = "io.wonmor.Vutric";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub summary: String,
    pub body: String,
    /// `(key, label)`: the key comes back in `ActionInvoked`.
    pub actions: Vec<(String, String)>,
    /// Shown as the notification's picture when the server supports it.
    pub image: Option<PathBuf>,
    /// The action run when the notification body itself is clicked.
    pub default_action: Option<String>,
    /// Replaces an earlier notification with this id (0 = new).
    pub replaces: u32,
}

/// A hint value: the spec types we use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hint {
    Str(String),
    Byte(u8),
    Bool(bool),
}

/// Markup is optional in the spec; servers that support it would parse `<` and `&`, so they are escaped.
#[must_use]
pub fn escape_body(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c if c.is_control() && c != '\n' => {}
            c => out.push(c),
        }
    }
    out
}

/// A `file://` URI with every byte outside the unreserved set percent-encoded.
#[must_use]
pub fn file_uri(p: &Path) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("file://");
    for b in p.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

impl Notice {
    /// The flat `[key, label, key, label, ...]` list of the spec, `default` first when set.
    #[must_use]
    pub fn action_list(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(d) = &self.default_action {
            out.push("default".to_owned());
            out.push(
                self.actions
                    .iter()
                    .find(|(k, _)| k == d)
                    .map_or_else(String::new, |(_, l)| l.clone()),
            );
        }
        for (k, l) in &self.actions {
            out.push(k.clone());
            out.push(l.clone());
        }
        out
    }

    /// The action key behind what the server reported (`default` maps to [`Notice::default_action`]).
    #[must_use]
    pub fn resolve<'a>(&'a self, key: &'a str) -> Option<&'a str> {
        if key == "default" {
            return self.default_action.as_deref();
        }
        self.actions.iter().any(|(k, _)| k == key).then_some(key)
    }

    #[must_use]
    pub fn hints(&self) -> Vec<(&'static str, Hint)> {
        let mut h = vec![
            ("desktop-entry", Hint::Str(DESKTOP_ENTRY.to_owned())),
            ("category", Hint::Str("transfer.complete".to_owned())),
            ("urgency", Hint::Byte(1)),
        ];
        if let Some(img) = &self.image {
            h.push(("image-path", Hint::Str(file_uri(img))));
        }
        h
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::collections::HashMap;

    use zbus::zvariant::Value;

    use super::{Hint, Notice, escape_body};

    #[zbus::proxy(
        interface = "org.freedesktop.Notifications",
        default_service = "org.freedesktop.Notifications",
        default_path = "/org/freedesktop/Notifications"
    )]
    pub trait Notifications {
        #[allow(clippy::too_many_arguments)]
        fn notify(
            &self,
            app_name: &str,
            replaces_id: u32,
            app_icon: &str,
            summary: &str,
            body: &str,
            actions: &[&str],
            hints: HashMap<&str, Value<'_>>,
            expire_timeout: i32,
        ) -> zbus::Result<u32>;

        fn get_capabilities(&self) -> zbus::Result<Vec<String>>;

        fn close_notification(&self, id: u32) -> zbus::Result<()>;

        #[zbus(signal)]
        fn action_invoked(&self, id: u32, action_key: String) -> zbus::Result<()>;

        #[zbus(signal)]
        fn notification_closed(&self, id: u32, reason: u32) -> zbus::Result<()>;
    }

    /// Sends `n`; returns the server's id. `body` is escaped only when the server says it parses markup.
    pub async fn show(proxy: &NotificationsProxy<'_>, n: &Notice) -> zbus::Result<u32> {
        let caps = proxy.get_capabilities().await.unwrap_or_default();
        let markup = caps.iter().any(|c| c == "body-markup");
        let body = if markup {
            escape_body(&n.body)
        } else {
            n.body.clone()
        };
        let actions = if caps.iter().any(|c| c == "actions") {
            n.action_list()
        } else {
            Vec::new()
        };
        let actions: Vec<&str> = actions.iter().map(String::as_str).collect();
        let hints: HashMap<&str, Value<'_>> = n
            .hints()
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    match v {
                        Hint::Str(s) => Value::from(s),
                        Hint::Byte(b) => Value::from(b),
                        Hint::Bool(b) => Value::from(b),
                    },
                )
            })
            .collect();
        proxy
            .notify(
                "Vutric",
                n.replaces,
                crate::menu::ICON,
                &n.summary,
                &body,
                &actions,
                hints,
                -1,
            )
            .await
    }

    /// Opens a file or folder in its default app (`xdg-open`, present on every desktop we target).
    pub fn open(path: &std::path::Path) -> std::io::Result<()> {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(drop)
    }

    #[zbus::proxy(
        interface = "org.freedesktop.FileManager1",
        default_service = "org.freedesktop.FileManager1",
        default_path = "/org/freedesktop/FileManager1"
    )]
    pub trait FileManager1 {
        fn show_items(&self, uris: &[&str], startup_id: &str) -> zbus::Result<()>;
    }

    /// "Papkada ko'rsatish": selects the file in Nautilus, Dolphin or Nemo; falls back to opening its folder.
    pub async fn show_in_folder(
        conn: &zbus::Connection,
        file: &std::path::Path,
    ) -> std::io::Result<()> {
        let uri = super::file_uri(file);
        let shown = match FileManager1Proxy::new(conn).await {
            Ok(p) => p.show_items(&[uri.as_str()], "").await.is_ok(),
            Err(_) => false,
        };
        if shown {
            return Ok(());
        }
        open(file.parent().unwrap_or(file))
    }
}

#[cfg(target_os = "linux")]
pub use imp::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn photo() -> Notice {
        Notice {
            summary: "photo.jpg keldi".into(),
            body: "Ali's <Pixel> dan".into(),
            actions: vec![
                ("open".into(), "Ochish".into()),
                ("folder".into(), "Papkada ko'rsatish".into()),
            ],
            image: Some(PathBuf::from("/home/ali/Yuklanmalar/Vutric/photo 1.jpg")),
            default_action: Some("open".into()),
            replaces: 0,
        }
    }

    #[test]
    fn actions_follow_the_spec_layout() {
        assert_eq!(
            photo().action_list(),
            vec![
                "default",
                "Ochish",
                "open",
                "Ochish",
                "folder",
                "Papkada ko'rsatish"
            ]
        );
        let n = photo();
        assert_eq!(n.resolve("default"), Some("open"));
        assert_eq!(n.resolve("folder"), Some("folder"));
        assert_eq!(n.resolve("rm -rf"), None);
    }

    #[test]
    fn hints_name_us_and_the_picture() {
        let h = photo().hints();
        assert!(h.contains(&("desktop-entry", Hint::Str("io.wonmor.Vutric".into()))));
        assert!(h.contains(&(
            "image-path",
            Hint::Str("file:///home/ali/Yuklanmalar/Vutric/photo%201.jpg".into())
        )));
    }

    #[test]
    fn body_is_escaped_for_markup_servers() {
        assert_eq!(
            escape_body("Ali's <Pixel> & co\u{1b}"),
            "Ali's &lt;Pixel&gt; &amp; co"
        );
        assert_eq!(
            file_uri(Path::new("/o\u{2bb}zbek/a#b")),
            "file:///o%CA%BBzbek/a%23b"
        );
    }
}
