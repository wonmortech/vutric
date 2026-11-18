//! The tray and the notifications against stand-in servers on a real session bus. CI runs this under
//! `dbus-run-session`; without a session bus every test returns early and says so.

#![cfg(target_os = "linux")]
// Stand-in D-Bus servers: the zbus macro wants owned, named arguments.
#![allow(
    clippy::unused_self,
    clippy::needless_pass_by_value,
    clippy::used_underscore_binding,
    clippy::items_after_statements
)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::time::timeout;
use vutric_host_linux::menu::{ICON, ICON_PAUSED, TrayModel};
use vutric_host_linux::notify::{Notice, NotificationsProxy, show};
use vutric_host_linux::sni::Tray;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{connection, interface};

const WAIT: Duration = Duration::from_secs(5);

fn no_bus() -> bool {
    let none = std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none();
    if none {
        eprintln!("skipped: no session bus (run under dbus-run-session)");
    }
    none
}

struct Watcher(UnboundedSender<String>);

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    fn register_status_notifier_item(&self, service: String) {
        let _ = self.0.send(service);
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }
}

type Captured = (String, Vec<String>, HashMap<String, OwnedValue>, String);

struct FakeNotifications(UnboundedSender<Captured>);

#[interface(name = "org.freedesktop.Notifications")]
impl FakeNotifications {
    fn get_capabilities(&self) -> Vec<String> {
        vec!["actions".into(), "body".into(), "body-markup".into()]
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        _replaces_id: u32,
        _app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        _expire_timeout: i32,
    ) -> u32 {
        let _ = self
            .0
            .send((format!("{app_name}: {summary}"), actions, hints, body));
        7
    }

    fn close_notification(&self, _id: u32) {}
}

fn model(paused: bool) -> TrayModel {
    TrayModel::standard(
        "Vutric",
        "Vutric — qabul qilishga tayyor",
        paused,
        (
            "Telefonni ulash",
            "Sozlamalar",
            "To'xtatib turish",
            "Chiqish",
        ),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn tray_registers_and_menu_clicks_arrive() {
    if no_bus() {
        return;
    }
    let (wtx, mut wrx) = unbounded_channel();
    let _watcher = connection::Builder::session()
        .unwrap()
        .name("org.kde.StatusNotifierWatcher")
        .unwrap()
        .serve_at("/StatusNotifierWatcher", Watcher(wtx))
        .unwrap()
        .build()
        .await
        .unwrap();
    let (atx, mut arx) = unbounded_channel();
    let tray = Tray::spawn(model(false), atx).await.unwrap();
    let registered = timeout(WAIT, wrx.recv()).await.unwrap().unwrap();
    assert_eq!(registered, tray.name());

    let client = zbus::Connection::session().await.unwrap();
    let reply = client
        .call_method(
            Some(tray.name()),
            "/MenuBar",
            Some("com.canonical.dbusmenu"),
            "GetLayout",
            &(0i32, -1i32, Vec::<String>::new()),
        )
        .await
        .unwrap();
    type Node = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);
    let (_rev, root): (u32, Node) = reply.body().deserialize().unwrap();
    assert_eq!(root.0, 0);
    assert_eq!(root.2.len(), 6);

    client
        .call_method(
            Some(tray.name()),
            "/MenuBar",
            Some("com.canonical.dbusmenu"),
            "Event",
            &(1i32, "clicked", Value::from(0i32), 0u32),
        )
        .await
        .unwrap();
    assert_eq!(timeout(WAIT, arx.recv()).await.unwrap().unwrap(), "pair");

    client
        .call_method(
            Some(tray.name()),
            "/StatusNotifierItem",
            Some("org.kde.StatusNotifierItem"),
            "Activate",
            &(0i32, 0i32),
        )
        .await
        .unwrap();
    assert_eq!(
        timeout(WAIT, arx.recv()).await.unwrap().unwrap(),
        "activate"
    );

    let icon = |c: zbus::Connection, name: String| async move {
        let p = zbus::Proxy::new(
            &c,
            name,
            "/StatusNotifierItem",
            "org.kde.StatusNotifierItem",
        )
        .await
        .unwrap();
        p.get_property::<String>("IconName").await.unwrap()
    };
    assert_eq!(icon(client.clone(), tray.name().to_owned()).await, ICON);
    tray.set_model(model(true)).await.unwrap();
    let uncached = zbus::Connection::session().await.unwrap();
    assert_eq!(icon(uncached, tray.name().to_owned()).await, ICON_PAUSED);
}

#[tokio::test(flavor = "multi_thread")]
async fn notification_carries_actions_hints_and_escaped_body() {
    if no_bus() {
        return;
    }
    let (tx, mut rx) = unbounded_channel();
    let _server = connection::Builder::session()
        .unwrap()
        .name("org.freedesktop.Notifications")
        .unwrap()
        .serve_at("/org/freedesktop/Notifications", FakeNotifications(tx))
        .unwrap()
        .build()
        .await
        .unwrap();
    let conn = zbus::Connection::session().await.unwrap();
    let proxy = NotificationsProxy::new(&conn).await.unwrap();
    let n = Notice {
        summary: "photo.jpg keldi".into(),
        body: "Ali's <Pixel> dan".into(),
        actions: vec![
            ("open".into(), "Ochish".into()),
            ("folder".into(), "Papkada ko'rsatish".into()),
        ],
        image: Some(PathBuf::from("/tmp/photo 1.jpg")),
        default_action: Some("open".into()),
        replaces: 0,
    };
    assert_eq!(show(&proxy, &n).await.unwrap(), 7);
    let (title, actions, hints, body) = timeout(WAIT, rx.recv()).await.unwrap().unwrap();
    assert_eq!(title, "Vutric: photo.jpg keldi");
    assert_eq!(&actions[..2], ["default", "Ochish"]);
    assert_eq!(actions.len(), 6);
    assert_eq!(body, "Ali's &lt;Pixel&gt; dan");
    let entry: String = hints["desktop-entry"].clone().try_into().unwrap();
    assert_eq!(entry, "io.wonmor.Vutric");
    let image: String = hints["image-path"].clone().try_into().unwrap();
    assert_eq!(image, "file:///tmp/photo%201.jpg");
}
