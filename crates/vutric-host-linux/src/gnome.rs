//! The daemon's side of the Vutric GNOME Shell extension (ADR-0090 §2): the D-Bus API it exports on the Shell's
//! connection. The XML in `apps/gnome-extension/` is the contract; [`METHODS`] and [`SIGNALS`] are checked
//! against it in the tests so the two never drift.

/// Extension UUID on extensions.gnome.org.
pub const UUID: &str = "vutric@wonmor.io";
pub const INTERFACE: &str = "io.wonmor.Vutric.Shell1";
pub const PATH: &str = "/io/wonmor/Vutric/Shell";
pub const SERVICE: &str = "org.gnome.Shell";
/// The `app_id` of the GNOME effect surface; the extension only retypes windows of the client it launched.
pub const SURFACE_APP_ID: &str = "io.wonmor.Vutric.Surface";
/// API level this build speaks. The extension answers `Ping` with its own; a lower one is "update the extension".
pub const API: u32 = 1;

pub const INTERFACE_XML: &str =
    include_str!("../../../apps/gnome-extension/vutric@wonmor.io/dbus/io.wonmor.Vutric.Shell1.xml");

pub const METHODS: &[&str] = &["Ping", "SpawnSurface", "ShowCard", "HideCard", "SetStatus"];
pub const SIGNALS: &[&str] = &["CardClicked", "MenuActivated"];

/// Whether this extension can serve us, from its `Ping` answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Extension {
    Ready { shell: String },
    TooOld { api: u32 },
    Missing,
}

#[must_use]
pub fn judge(ping: Option<(u32, String)>) -> Extension {
    match ping {
        None => Extension::Missing,
        Some((api, _)) if api < API => Extension::TooOld { api },
        Some((_, shell)) => Extension::Ready { shell },
    }
}

/// A card button area for `ShowCard`: `(id, x0, y0, x1, y1)` in buffer pixels.
#[must_use]
pub fn hits(h: &[crate::proto::Hit]) -> Vec<(String, f64, f64, f64, f64)> {
    h.iter()
        .map(|h| {
            let [x0, y0, x1, y1] = h.rect.map(f64::from);
            (h.id.clone(), x0, y0, x1, y1)
        })
        .collect()
}

#[cfg(target_os = "linux")]
mod imp {
    #[zbus::proxy(
        interface = "io.wonmor.Vutric.Shell1",
        default_service = "org.gnome.Shell",
        default_path = "/io/wonmor/Vutric/Shell"
    )]
    pub trait Shell {
        fn ping(&self) -> zbus::Result<(u32, String)>;

        fn spawn_surface(&self, socket: &str) -> zbus::Result<i32>;

        fn show_card(
            &self,
            width: u32,
            height: u32,
            scale: i32,
            bgra: &[u8],
            hits: &[(String, f64, f64, f64, f64)],
        ) -> zbus::Result<()>;

        fn hide_card(&self) -> zbus::Result<()>;

        fn set_status(&self, paused: bool, lang: &str) -> zbus::Result<()>;

        #[zbus(signal)]
        fn card_clicked(&self, id: String) -> zbus::Result<()>;

        #[zbus(signal)]
        fn menu_activated(&self, id: String) -> zbus::Result<()>;
    }

    /// GNOME Shell's own extension manager (what `gnome-extensions enable` calls).
    #[zbus::proxy(
        interface = "org.gnome.Shell.Extensions",
        default_service = "org.gnome.Shell",
        default_path = "/org/gnome/Shell"
    )]
    pub trait ShellExtensions {
        fn enable_extension(&self, uuid: &str) -> zbus::Result<bool>;
    }

    /// Turns the Vutric extension on (the first-run question, ADR-0093 §2). `Ok(true)` once its `Ping` answers;
    /// `Ok(false)` when Shell accepted the request but has not loaded it (installed during this session: on
    /// Wayland that needs a new login).
    pub async fn enable(conn: &zbus::Connection) -> zbus::Result<bool> {
        let ext = ShellExtensionsProxy::new(conn).await?;
        if !ext.enable_extension(super::UUID).await? {
            return Ok(false);
        }
        for _ in 0..10 {
            if matches!(probe(conn).await, super::Extension::Ready { .. }) {
                return Ok(true);
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        Ok(false)
    }

    /// Asks the extension whether it runs; `Missing` when GNOME Shell does not have it enabled.
    pub async fn probe(conn: &zbus::Connection) -> super::Extension {
        let ping = match ShellProxy::new(conn).await {
            Ok(p) => p.ping().await.ok(),
            Err(_) => None,
        };
        super::judge(ping)
    }
}

#[cfg(target_os = "linux")]
pub use imp::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn names(kind: &str) -> Vec<String> {
        let tag = format!("<{kind} name=\"");
        INTERFACE_XML
            .match_indices(&tag)
            .map(|(i, _)| {
                let rest = &INTERFACE_XML[i + tag.len()..];
                rest[..rest.find('"').unwrap()].to_owned()
            })
            .collect()
    }

    #[test]
    fn rust_side_matches_the_xml() {
        assert!(INTERFACE_XML.contains(&format!("<interface name=\"{INTERFACE}\">")));
        assert_eq!(names("method"), METHODS);
        assert_eq!(names("signal"), SIGNALS);
    }

    #[test]
    fn ping_answers_are_judged() {
        assert_eq!(judge(None), Extension::Missing);
        assert_eq!(
            judge(Some((0, "46.0".into()))),
            Extension::TooOld { api: 0 }
        );
        assert_eq!(
            judge(Some((API, "50.1".into()))),
            Extension::Ready {
                shell: "50.1".into()
            }
        );
    }

    #[test]
    fn hit_rects_widen_to_f64() {
        let h = hits(&[crate::proto::Hit {
            id: "accept".into(),
            rect: [1.0, 2.0, 3.5, 4.0],
        }]);
        assert_eq!(h, vec![("accept".to_owned(), 1.0, 2.0, 3.5, 4.0)]);
    }
}
