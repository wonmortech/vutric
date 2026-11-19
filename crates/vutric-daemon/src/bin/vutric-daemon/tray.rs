//! The notification-area icon and its menu: Telefonni ulash, Telefonni topish, Hammaga ochish, Sozlamalar,
//! Toʻxtatib turish, Chiqish.

use anyhow::Result;
pub use tray_icon::menu::MenuId;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};
use vutric_daemon::i18n::{Lang, t};
use winit::event_loop::EventLoopProxy;

use crate::UiEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Pair,
    FindPhone,
    Everyone,
    Settings,
    TogglePause,
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
    pause: MenuItem,
    ids: [(MenuId, Action); 6],
    lang: Lang,
}

/// The tray glyph: an edge-on portal ring (sand rim, signal glow), drawn at `size` px. Grey when paused.
fn glyph(size: u32, paused: bool) -> Icon {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let c = (size as f32 - 1.0) / 2.0;
    let (ry, rx) = (size as f32 * 0.44, size as f32 * 0.26);
    let (rim, glow): ([u8; 3], [u8; 3]) = if paused {
        ([150, 156, 170], [90, 96, 110])
    } else {
        ([244, 217, 166], [47, 107, 255])
    };
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = ((x as f32 - c) / rx, (y as f32 - c) / ry);
            let d = ((dx * dx + dy * dy).sqrt() - 1.0).abs() * ry;
            let core = (1.6 - d).clamp(0.0, 1.0);
            let halo = (1.0 - d / 4.0).clamp(0.0, 1.0) * 0.55;
            let a = core.max(halo);
            if a <= 0.0 {
                continue;
            }
            let mix =
                |i: usize| (f32::from(rim[i]) * core + f32::from(glow[i]) * (1.0 - core)) as u8;
            let i = ((y * size + x) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&[mix(0), mix(1), mix(2), (a * 255.0) as u8]);
        }
    }
    Icon::from_rgba(rgba, size, size).expect("valid icon size")
}

impl Tray {
    pub fn new(lang: Lang, proxy: &EventLoopProxy<UiEvent>) -> Result<Self> {
        let pair = MenuItem::new(t(lang, "tray.pair"), true, None);
        let find = MenuItem::new(t(lang, "tray.find"), true, None);
        let everyone = MenuItem::new(t(lang, "tray.everyone"), true, None);
        let settings = MenuItem::new(t(lang, "tray.settings"), true, None);
        let pause = MenuItem::new(t(lang, "tray.pause"), true, None);
        let quit = MenuItem::new(t(lang, "tray.quit"), true, None);
        let menu = Menu::new();
        menu.append_items(&[
            &pair,
            &find,
            &everyone,
            &settings,
            &pause,
            &PredefinedMenuItem::separator(),
            &quit,
        ])?;
        let ids = [
            (pair.id().clone(), Action::Pair),
            (find.id().clone(), Action::FindPhone),
            (everyone.id().clone(), Action::Everyone),
            (settings.id().clone(), Action::Settings),
            (pause.id().clone(), Action::TogglePause),
            (quit.id().clone(), Action::Quit),
        ];
        let p = proxy.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = p.send_event(UiEvent::Menu(e.id));
        }));
        let p = proxy.clone();
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = e
            {
                let _ = p.send_event(UiEvent::TrayOpen);
            }
        }));
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(t(lang, "tray.tooltip"))
            .with_icon(glyph(32, false))
            .build()?;
        Ok(Self {
            icon,
            pause,
            ids,
            lang,
        })
    }

    pub fn action(&self, id: &MenuId) -> Option<Action> {
        self.ids.iter().find(|(i, _)| i == id).map(|(_, a)| *a)
    }

    pub fn set_paused(&self, paused: bool) {
        self.pause.set_text(t(
            self.lang,
            if paused { "tray.resume" } else { "tray.pause" },
        ));
        let _ = self.icon.set_icon(Some(glyph(32, paused)));
        let _ = self.icon.set_tooltip(Some(t(
            self.lang,
            if paused {
                "tray.tooltip.paused"
            } else {
                "tray.tooltip"
            },
        )));
    }
}
