//! The tray icon as a `StatusNotifierItem` with a `com.canonical.dbusmenu` menu (ADR-0091), over zbus.
//!
//! Plasma, COSMIC, waybar and Ubuntu's `AppIndicator` extension host it. Plain GNOME has no SNI host:
//! [`Tray::spawn`] then fails at registration and the daemon uses the Vutric GNOME extension's panel menu.

// D-Bus properties and methods take `&self` and return constants by the interface's definition.
// The zbus macro reads argument names (for introspection) and wants owned arguments.
#![allow(
    clippy::unused_self,
    clippy::unnecessary_literal_bound,
    clippy::used_underscore_binding,
    clippy::needless_pass_by_value
)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::mpsc::UnboundedSender;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{self, OwnedObjectPath, OwnedValue, Type, Value};
use zbus::{connection, interface};

use crate::menu::{Prop, TrayModel};

const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
const WATCHER: &str = "org.kde.StatusNotifierWatcher";

type Pixmaps = Vec<(i32, i32, Vec<u8>)>;

/// A dbusmenu layout node, signature `(ia{sv}av)`.
#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, Type, zvariant::Value, zvariant::OwnedValue,
)]
pub struct Layout {
    id: i32,
    props: HashMap<String, OwnedValue>,
    children: Vec<OwnedValue>,
}

fn props_map(props: &[(&'static str, Prop)], only: &[String]) -> HashMap<String, OwnedValue> {
    props
        .iter()
        .filter(|(k, _)| only.is_empty() || only.iter().any(|o| o == k))
        .filter_map(|(k, v)| {
            let v = match v {
                Prop::Str(s) => Value::from(s.clone()),
                Prop::Bool(b) => Value::from(*b),
            };
            OwnedValue::try_from(v).ok().map(|v| ((*k).to_owned(), v))
        })
        .collect()
}

fn layout(model: &TrayModel, parent: i32, depth: i32, only: &[String]) -> Layout {
    let nodes = model.nodes();
    let build = |id: i32, with_children: bool| -> Option<Layout> {
        let n = nodes.iter().find(|n| n.id == id)?;
        let children = if with_children {
            n.children
                .iter()
                .filter_map(|c| nodes.iter().find(|n| n.id == *c))
                .filter_map(|c| {
                    OwnedValue::try_from(Layout {
                        id: c.id,
                        props: props_map(&c.props, only),
                        children: vec![],
                    })
                    .ok()
                })
                .collect()
        } else {
            vec![]
        };
        Some(Layout {
            id: n.id,
            props: props_map(&n.props, only),
            children,
        })
    };
    build(parent, depth != 0).unwrap_or(Layout {
        id: parent,
        props: HashMap::new(),
        children: vec![],
    })
}

#[derive(Debug)]
struct Shared {
    model: Mutex<TrayModel>,
    revision: Mutex<u32>,
    actions: UnboundedSender<String>,
}

impl Shared {
    fn model(&self) -> TrayModel {
        self.model
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

struct Item(Arc<Shared>);
struct Menu(Arc<Shared>);

#[interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn category(&self) -> &str {
        "Communications"
    }

    #[zbus(property)]
    fn id(&self) -> &str {
        "vutric"
    }

    #[zbus(property)]
    fn title(&self) -> String {
        self.0.model().title
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        "Active"
    }

    #[zbus(property)]
    fn window_id(&self) -> i32 {
        0
    }

    #[zbus(property)]
    fn icon_name(&self) -> String {
        self.0.model().icon().to_owned()
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> Pixmaps {
        Vec::new()
    }

    #[zbus(property)]
    fn tool_tip(&self) -> (String, Pixmaps, String, String) {
        let m = self.0.model();
        (m.icon().to_owned(), Vec::new(), m.title, m.tooltip)
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(MENU_PATH).expect("static path is valid")
    }

    fn activate(&self, _x: i32, _y: i32) {
        let _ = self.0.actions.send("activate".to_owned());
    }

    fn secondary_activate(&self, _x: i32, _y: i32) {}

    fn context_menu(&self, _x: i32, _y: i32) {}

    fn scroll(&self, _delta: i32, _orientation: &str) {}

    #[zbus(signal)]
    async fn new_icon(e: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_title(e: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_tool_tip(e: &SignalEmitter<'_>) -> zbus::Result<()>;
}

#[interface(name = "com.canonical.dbusmenu")]
impl Menu {
    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }

    #[zbus(property)]
    fn text_direction(&self) -> &str {
        "ltr"
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        "normal"
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }

    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<String>,
    ) -> (u32, Layout) {
        let rev = *self
            .0
            .revision
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        (
            rev,
            layout(&self.0.model(), parent_id, recursion_depth, &property_names),
        )
    }

    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> Vec<(i32, HashMap<String, OwnedValue>)> {
        self.0
            .model()
            .nodes()
            .into_iter()
            .filter(|n| ids.is_empty() || ids.contains(&n.id))
            .map(|n| (n.id, props_map(&n.props, &property_names)))
            .collect()
    }

    fn get_property(&self, id: i32, name: String) -> zbus::fdo::Result<OwnedValue> {
        self.0
            .model()
            .nodes()
            .into_iter()
            .find(|n| n.id == id)
            .and_then(|n| props_map(&n.props, &[name]).into_values().next())
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("no such item or property".into()))
    }

    fn event(&self, id: i32, event_id: &str, _data: OwnedValue, _timestamp: u32) {
        if event_id == "clicked"
            && let Some(a) = self.0.model().action(id)
        {
            let _ = self.0.actions.send(a.to_owned());
        }
    }

    fn event_group(&self, events: Vec<(i32, String, OwnedValue, u32)>) -> Vec<i32> {
        let model = self.0.model();
        let mut errors = Vec::new();
        for (id, kind, _, _) in events {
            match model.action(id) {
                Some(a) if kind == "clicked" => {
                    let _ = self.0.actions.send(a.to_owned());
                }
                Some(_) => {}
                None => errors.push(id),
            }
        }
        errors
    }

    fn about_to_show(&self, _id: i32) -> bool {
        false
    }

    fn about_to_show_group(&self, _ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        (Vec::new(), Vec::new())
    }

    #[zbus(signal)]
    async fn layout_updated(e: &SignalEmitter<'_>, revision: u32, parent: i32) -> zbus::Result<()>;
}

/// A registered tray icon. Dropping it removes the icon (the bus name goes away with the connection).
#[derive(Debug, Clone)]
pub struct Tray {
    conn: zbus::Connection,
    shared: Arc<Shared>,
    name: String,
}

async fn register(conn: &zbus::Connection, name: &str) -> zbus::Result<()> {
    conn.call_method(
        Some(WATCHER),
        "/StatusNotifierWatcher",
        Some(WATCHER),
        "RegisterStatusNotifierItem",
        &(name,),
    )
    .await
    .map(drop)
}

impl Tray {
    /// Exports the item and the menu and registers with the watcher. Menu clicks and a left click on the icon
    /// (`activate`) arrive on `actions`. Fails when no tray host runs (plain GNOME).
    pub async fn spawn(model: TrayModel, actions: UnboundedSender<String>) -> zbus::Result<Self> {
        let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
        let shared = Arc::new(Shared {
            model: Mutex::new(model),
            revision: Mutex::new(1),
            actions,
        });
        let conn = connection::Builder::session()?
            .name(name.as_str())?
            .serve_at(ITEM_PATH, Item(Arc::clone(&shared)))?
            .serve_at(MENU_PATH, Menu(Arc::clone(&shared)))?
            .build()
            .await?;
        register(&conn, &name).await?;
        let tray = Self { conn, shared, name };
        tray.watch_restarts();
        Ok(tray)
    }

    /// Plasma's shell restarting brings a new watcher; register again so the icon comes back.
    fn watch_restarts(&self) {
        let (conn, name) = (self.conn.clone(), self.name.clone());
        tokio::spawn(async move {
            use futures_util::StreamExt as _;
            let Ok(dbus) = zbus::fdo::DBusProxy::new(&conn).await else {
                return;
            };
            let Ok(mut changes) = dbus.receive_name_owner_changed().await else {
                return;
            };
            while let Some(sig) = changes.next().await {
                let Ok(args) = sig.args() else { continue };
                if args.name().as_str() == WATCHER && args.new_owner().is_some() {
                    let _ = register(&conn, &name).await;
                }
            }
        });
    }

    /// Replaces the model (pause state, language) and tells the host to redraw.
    pub async fn set_model(&self, model: TrayModel) -> zbus::Result<()> {
        *self
            .shared
            .model
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = model;
        let rev = {
            let mut r = self
                .shared
                .revision
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            *r += 1;
            *r
        };
        let server = self.conn.object_server();
        let item = server.interface::<_, Item>(ITEM_PATH).await?;
        Item::new_icon(item.signal_emitter()).await?;
        Item::new_title(item.signal_emitter()).await?;
        Item::new_tool_tip(item.signal_emitter()).await?;
        let menu = server.interface::<_, Menu>(MENU_PATH).await?;
        Menu::layout_updated(menu.signal_emitter(), rev, 0).await
    }

    /// The well-known name this item owns.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> TrayModel {
        TrayModel::standard("Vutric", "tip", false, ("a", "b", "c", "d"))
    }

    #[test]
    fn layout_is_root_with_items() {
        let l = layout(&model(), 0, -1, &[]);
        assert_eq!(l.id, 0);
        assert_eq!(l.children.len(), 6);
        let leaf = layout(&model(), 1, 0, &["label".to_owned()]);
        assert_eq!(leaf.props.len(), 1);
        assert!(leaf.children.is_empty());
        assert_eq!(layout(&model(), 42, -1, &[]).props.len(), 0);
    }

    #[test]
    fn layout_signature_is_dbusmenu() {
        assert_eq!(Layout::SIGNATURE.to_string(), "(ia{sv}av)");
    }
}
