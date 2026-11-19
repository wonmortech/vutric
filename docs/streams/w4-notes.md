# W4 · Linux desktop: stream notes

Working notes of W4 (owner decision 2026-11-19: shared docs are not edited any more; cross-stream needs go here,
with a patch the owning stream can apply, and a TEMP shim on our side where one is needed). Decisions are in
ADR-0090 (overlay), ADR-0091 (session), ADR-0092 (packaging), ADR-0093 (cards, daemon backends, compat on Linux).

Branch `w4/linux-1`, PR #32. Issue #33 (`needs-w2`) has the same list in short.

## 1. For W2 (daemon, overlay crate): the Linux branch

Everything Linux-specific is in `crates/vutric-host-linux` (and `vutric-host-wlr`); the daemon only calls it.
The patches below are written against `origin/w2/desktop` at `0b0f1e9`. Not compiled together (W2's crates are
not on this branch); W2 applies and builds them.

### 1.1 `crates/vutric-daemon/Cargo.toml`

```toml
[target.'cfg(target_os = "linux")'.dependencies]
vutric-host-linux = { path = "../vutric-host-linux" }
vutric-host-wlr = { path = "../vutric-host-wlr" }
zbus = { version = "5.19.0", default-features = false, features = ["tokio"] }
```

### 1.2 Tray: `src/bin/vutric-daemon/main.rs` + new `tray_linux.rs`

```rust
#[cfg(windows)]
mod tray;
#[cfg(target_os = "linux")]
#[path = "tray_linux.rs"]
mod tray;
#[cfg(not(any(windows, target_os = "linux")))]
#[path = "tray_none.rs"]
mod tray;
```

`tray_linux.rs` (same API as `tray.rs`: `Tray::new`, `action`, `set_paused`; the bus work runs on its own thread
with a current-thread runtime, so winit's loop never blocks):

```rust
//! Linux tray: StatusNotifierItem, or the GNOME extension's panel menu (vutric_host_linux::desktop::TrayHandle).

use std::sync::mpsc as std_mpsc;

use anyhow::{Context as _, Result};
use vutric_daemon::i18n::{Lang, t};
use vutric_host_linux::{desktop::TrayHandle, menu::TrayModel};
use winit::event_loop::EventLoopProxy;

use crate::UiEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Pair,
    Settings,
    TogglePause,
    Quit,
}

pub struct Tray {
    paused: std_mpsc::Sender<bool>,
}

fn model(lang: Lang, paused: bool) -> TrayModel {
    let tooltip = t(lang, if paused { "tray.tooltip.paused" } else { "tray.tooltip" });
    let pause = t(lang, if paused { "tray.resume" } else { "tray.pause" });
    TrayModel::standard("Vutric", tooltip, paused, (t(lang, "tray.pair"), t(lang, "tray.settings"), pause, t(lang, "tray.quit")))
}

fn lang_code(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "en",
        _ => "uz",
    }
}

impl Tray {
    pub fn new(lang: Lang, proxy: &EventLoopProxy<UiEvent>) -> Result<Self> {
        let (paused_tx, paused_rx) = std_mpsc::channel::<bool>();
        let (ok_tx, ok_rx) = std_mpsc::channel::<bool>();
        let proxy = proxy.clone();
        std::thread::Builder::new().name("tray".into()).spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tray runtime");
            rt.block_on(async move {
                let bus = zbus::Connection::session().await.ok();
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                let tray = TrayHandle::start(bus.as_ref(), model(lang, false), lang_code(lang), tx).await;
                let _ = ok_tx.send(!matches!(tray, TrayHandle::None));
                loop {
                    tokio::select! {
                        Some(a) = rx.recv() => {
                            let ev = if a == "activate" { UiEvent::TrayOpen } else { UiEvent::Menu(MenuId(a)) };
                            if proxy.send_event(ev).is_err() { return; }
                        }
                        () = tokio::time::sleep(std::time::Duration::from_millis(200)) => {
                            while let Ok(p) = paused_rx.try_recv() {
                                tray.update(model(lang, p), lang_code(lang)).await;
                            }
                        }
                    }
                }
            });
        })?;
        anyhow::ensure!(ok_rx.recv().context("tray thread")?, "no StatusNotifierWatcher and no Vutric GNOME extension");
        Ok(Self { paused: paused_tx })
    }

    pub fn action(&self, id: &MenuId) -> Option<Action> {
        match id.0.as_str() {
            "pair" => Some(Action::Pair),
            "settings" => Some(Action::Settings),
            "toggle_pause" => Some(Action::TogglePause),
            "quit" => Some(Action::Quit),
            _ => None,
        }
    }

    pub fn set_paused(&self, paused: bool) {
        let _ = self.paused.send(paused);
    }
}
```

`UiEvent::Menu`'s `cfg_attr(not(windows), allow(dead_code))` becomes `cfg_attr(not(any(windows, target_os = "linux")), ...)`.

### 1.3 "Files arrived" on Linux (`UiEvent::Received`)

```rust
#[cfg(target_os = "linux")]
{
    use vutric_daemon::i18n::{t, tf};
    use vutric_host_linux::notify::Notice;
    let name = files[0].file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let summary = if files.len() == 1 {
        tf(self.lang, "toast.received.one", &[("name", &name)])
    } else {
        tf(self.lang, "toast.received.many", &[("count", &files.len().to_string())])
    };
    let notice = Notice {
        summary,
        body: tf(self.lang, "toast.from", &[("device", &device)]),
        actions: vec![
            ("open".into(), t(self.lang, "toast.open").into()),
            ("folder".into(), t(self.lang, "toast.show_in_folder").into()),
        ],
        image: (files.len() == 1).then(|| files[0].clone()),
        default_action: Some("open".into()),
        replaces: 0,
    };
    let (first, folder) = (files[0].clone(), folder.clone());
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("notify runtime");
        rt.block_on(async move {
            use futures_util::StreamExt as _;
            let Ok(bus) = zbus::Connection::session().await else { return };
            let Ok(proxy) = vutric_host_linux::notify::NotificationsProxy::new(&bus).await else { return };
            let Ok(mut invoked) = proxy.receive_action_invoked().await else { return };
            let Ok(id) = vutric_host_linux::notify::show(&proxy, &notice).await else { return };
            let deadline = tokio::time::sleep(std::time::Duration::from_secs(600));
            tokio::pin!(deadline);
            loop {
                tokio::select! {
                    Some(sig) = invoked.next() => {
                        let Ok(a) = sig.args() else { continue };
                        if *a.id() != id { continue; }
                        match notice.resolve(a.action_key()) {
                            Some("open") => { let _ = vutric_host_linux::notify::open(&first); }
                            Some("folder") => { let _ = vutric_host_linux::notify::show_in_folder(&bus, &first).await; }
                            _ => {}
                        }
                        let _ = folder;
                        return;
                    }
                    () = &mut deadline => return,
                }
            }
        });
    });
}
```

(`futures-util` is already a dependency of `vutric-host-linux`; the daemon needs it too, or use
`tokio_stream`-free `while let Some(x) = invoked.next().await` with `futures_util::StreamExt`.)

### 1.4 Overlay on Linux: `gpu.rs` stays Windows/macOS; Linux talks to `vutric-surface`

Linux has no in-process GPU (ADR-0090 §3). In `App`:

- `ensure_gpu` on Linux: decide once with
  `vutric_host_linux::desktop::plan(&Session::from_env(), &extension)` where `extension` comes from
  `vutric_host_linux::gnome::probe(&bus)` on GNOME, else `Extension::Missing`. Then
  - `OverlayPlan::LayerShell` → `SurfaceLink::start(Launch::Spawn(find_helper("vutric-surface")?), None, CONNECT_WITHIN)`;
  - `OverlayPlan::Gnome` → `SurfaceLink::start(Launch::Gnome, Some(&bus), CONNECT_WITHIN)` and **keep `bus` alive** as
    long as the link (the extension kills the surface when the caller's bus name goes away);
  - `GnomeNeedsExtension` → one notification with `linux.gnome.extension.title/body`, no overlay;
  - `NotificationsOnly(_)` → no overlay.
  After `Ready`, send `ToSurface::Warm` (ADR-0031: warm on connect).
- `play`: `ToSurface::Play { id, zone, gesture, image: proto::Image { width, height, rgba } }` (straight RGBA, the
  preview image the Windows path uploads).
- `gpu_release_at` reached: `ToSurface::Release` (process stays for cards, GPU memory goes).
- Quit: drop the link (it sends `Quit`).
- `FromSurface::Error` at any point: log it, fall back to notifications for this session.

The link is async (tokio); W2's daemon already runs a tokio runtime for the network side, so the link lives
there and forwards `FromSurface` into winit with `proxy.send_event(UiEvent::Surface(msg))` (a new variant).

### 1.5 Cards on Linux (`cardwin.rs`)

```rust
#[cfg(target_os = "linux")]
fn show(card: &vutric_overlay::card::Card, scale: i32, out: &SurfaceOrShell) {
    let (regular, strong) = vutric_host_linux::fonts::ui_fonts();   // see 1.7
    let r = vutric_overlay::card::render(card, &fonts, &Palette::vutric(), scale as f32);
    let linux = vutric_host_linux::cards::LinuxCard {
        width: r.width, height: r.height, scale, bgra: r.bgra,
        buttons: r.buttons.iter().filter_map(|(b, rect)| {
            vutric_host_linux::cards::button_id(&format!("{b:?}")).map(|id| (id.to_owned(), *rect))
        }).collect(),
    };
    match out {
        SurfaceOrShell::Surface(link) => { let _ = link.send(vutric_host_linux::cards::to_surface(linux)?); }
        SurfaceOrShell::Shell(shell) => { let (w, h, s, px, hits) = vutric_host_linux::cards::to_gnome(linux)?; shell.show_card(w, h, s, &px, &hits).await?; }
    }
}
```

Clicks: `FromSurface::Clicked { id }` (layer-shell) or the extension's `CardClicked(id)` signal → `UiEvent::Card(b)`
with `b` from the id (`accept` → `Button::Accept`, `decline` → `Decline`, `close` → `Close`). Hide:
`ToSurface::HideCard` / `HideCard()`. `scale` = `FromSurface::Ready::scale`.

### 1.6 Autostart (`autostart.rs`)

```rust
#[cfg(target_os = "linux")]
pub fn set(enabled: bool, exe: &Path) -> std::io::Result<()> {
    use vutric_host_linux::desktop::{AutostartVia, autostart_via};
    match autostart_via(vutric_host_linux::files::systemd_available()) {
        AutostartVia::Systemd => {
            remove_xdg_entry()?;          // never both: systemd would start the daemon twice
            vutric_host_linux::files::set_autostart(enabled, exe)
        }
        AutostartVia::XdgAutostart => set_xdg(enabled, exe), // today's code
    }
}
```

`is_enabled` likewise: `files::autostart_enabled()` under systemd. The existing `cfg(all(unix, not(macos)))` code
becomes `set_xdg` / `is_enabled_xdg`.

### 1.7 Fonts for the cards (`vutric-overlay/src/card.rs`, `Fonts::system`)

On Linux, replace the two DejaVu paths with `vutric_host_linux::fonts::{REGULAR, STRONG}` (or copy the lists:
`vutric-overlay` should not depend on a host crate). Without this, Fedora Workstation draws every card in the
embedded Latin-only Geist, so device names in other scripts show as boxes.

### 1.8 Where files land on Linux

`vutric_net::transfer::downloads_dir()` is `$HOME/Downloads/Vutric` on every OS. On a desktop with a localised
Downloads folder that creates a second, English `Downloads` (see section 2.1). The daemon on Linux should pass
`vutric_host_linux::paths::received_dir_from_env()` instead (and `paths::staging_dir` for partial files).

### 1.9 Sending from Nautilus and Dolphin

Both entries run `vutric-daemon send -- <paths>` (ADR-0091). W2: the `send` subcommand, which forwards to the
running daemon as `{"op":"send_files","paths":[...]}` (issue #29). On first run on Linux, call
`vutric_host_linux::files::install_file_manager_entries(lang)` once.

### 1.10 The settings and pairing windows (Slint, ADR-0030) on Linux

Packaging only on our side: the `.deb` installs the windows binary (expected name `vutric-desktop`) to
`/usr/libexec/vutric/` when it is built, with the runtime libraries Slint's winit + femtovg backend loads on
Linux (`libfontconfig1`, `libxkbcommon0`, `libegl1`, `libwayland-client0`). CI job `slint-m1` builds W2's M1 Slint
spike from `w2/desktop` and runs it under headless sway (frames + first frame logged) until the real binary exists.
W2: keep the binary name, or tell us the new one.

## 2. For W3 (network, compat)

### 2.1 `vutric-net::transfer::downloads_dir` on Linux

Reads `$HOME/Downloads`. Linux desktops localise that folder (`xdg-user-dirs`: `~/Yuklanmalar` in Uzbek, the
Cyrillic name in Russian). Suggested patch (pure, same logic as `vutric_host_linux::paths::parse_user_dirs`):

```rust
#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android"), not(target_os = "ios")))]
fn xdg_download_dir(home: &Path) -> Option<PathBuf> {
    let cfg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    let text = std::fs::read_to_string(cfg.join("user-dirs.dirs")).ok()?;
    let line = text.lines().find(|l| l.trim_start().starts_with("XDG_DOWNLOAD_DIR="))?;
    let v = line.split_once('=')?.1.trim().strip_prefix('"')?.strip_suffix('"')?;
    let p = match v.strip_prefix("$HOME/") { Some(rest) => home.join(rest), None if v.starts_with('/') => PathBuf::from(v), None => return None };
    (p != home).then_some(p)
}
```

### 2.2 Compat adapter memory cap on Linux

`host.rs` caps the adapter at 512 MiB on Windows (job object) only. On Linux `restrict_unix` sets
`no_new_privs`, `PDEATHSIG` and no core dumps but no memory cap. Suggested addition inside the Linux block of the
`pre_exec` closure (async-signal-safe):

```rust
let mem = libc::rlimit { rlim_cur: crate::host::MEMORY_LIMIT as libc::rlim_t * 2, rlim_max: crate::host::MEMORY_LIMIT as libc::rlim_t * 2 };
libc::setrlimit(libc::RLIMIT_AS, &raw const mem);
```

`RLIMIT_AS` counts address space, not resident memory: tokio's thread stacks and the allocator's arenas reserve
far more than they use, so the limit is twice the Windows working-set cap. Needs a run of the compat tests with it
before landing.

### 2.3 Quick Share and LocalSend on Linux (C3 Linux row, C1)

Nothing in the engine is Linux-specific. What W4 adds (ADR-0093 §4): the `.deb` installs `vutric-compat` to
`/usr/libexec/vutric/` (found by `paths::find_helper("vutric-compat")`), a firewalld service file for Fedora,
the compat tests in the Linux workflow with `avahi-daemon` running (port 5353 shared with Avahi, as on every real
Ubuntu and Fedora desktop), and checklist rows 30-36. `Start` on Linux: `alias = paths::device_name_from_system()`,
`staging = paths::staging_dir(&received)`, `identity = paths::compat_identity(...)`, `device_type = Desktop`.

## 3. TEMP shims on W4's side

| Shim | Where | Remove when |
|---|---|---|
| `examples/surface_driver.rs` plays the daemon's role in CI | `crates/vutric-host-linux/examples/` | W2's daemon runs the Linux branch in CI |
| `.deb` packs the Linux pieces without `vutric-daemon` and says so in its description | `packaging/linux/build-deb.sh` | W2's daemon is in the tree (the script picks it up by itself) |
| CI `slint-m1` builds W2's spike from the `w2/desktop` branch | `.github/workflows/linux-desktop.yml` | the windows binary is in the workspace; then build it from the tree |
| File-manager entries fail with the daemon's error until `send` exists | `packaging/linux/{nautilus,kio}` | #29 / §1.9 |

## 4. Things only a real machine can answer

`docs/checklists/linux-desktop.md` (Ubuntu 24.04 GNOME, Kubuntu / Plasma 6, Fedora GNOME 50) and
`docs/checklists/linux-ios.md` (Phase 4 cross-check). Top of the list: GNOME frame callbacks after the dock retype
(row 5b), focus and click-through on all three, the tray on Ubuntu's AppIndicator, Quick Share from an Android phone
with Avahi running.

## 5. Owner questions from W4 (not copied into owner-questions.md, per the 2026-11-19 rule)

From ADR-0093: (1) Plasma: Vutric's KDE Connect mode off by default when `kdeconnectd` runs? (2) Should the `.deb`
turn on the firewalld service itself? W4: no. (3) Where the compat identity lives on Linux:
`$XDG_DATA_HOME/vutric/compat` or the same layout W3 uses on Windows?
