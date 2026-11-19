//! `vutric-app [--page=pairing|devices|history|send|settings]`: the desktop windows, opened by the daemon's tray
//! and closed to nothing. Every read and write goes through the daemon's IPC (ADR-0031, ADR-0033).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod i18n;
mod ipc;
mod qr;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use i18n::{Lang, clock, t};
use slint::{ComponentHandle as _, Timer, TimerMode};

slint::include_modules!();

/// How long one pairing QR works (the daemon's window, ADR-0050).
const PAIRING_FOR: Duration = Duration::from_secs(600);

fn texts(ui: &AppWindow, lang: Lang) {
    let tr = ui.global::<Tr>();
    let s = |k: &str| slint::SharedString::from(t(lang, k));
    tr.set_nav_pair(s("nav.pair"));
    tr.set_nav_devices(s("nav.devices"));
    tr.set_nav_history(s("nav.history"));
    tr.set_nav_send(s("nav.send"));
    tr.set_nav_settings(s("nav.settings"));
    tr.set_pair_title(s("pair.title"));
    tr.set_pair_hint(s("pair.hint"));
    tr.set_pair_link(s("pair.link"));
    tr.set_pair_left(s("pair.left"));
    tr.set_pair_expired(s("pair.expired"));
    tr.set_pair_renew(s("pair.renew"));
    tr.set_pair_done(s("pair.done"));
}

/// The open pairing window, if this process opened one.
#[derive(Debug, Default)]
struct Pairing {
    opened: Option<Instant>,
}

fn open_pairing(ui: &AppWindow, lang: Lang, state: &RefCell<Pairing>) {
    match ipc::call(&ipc::op("pairing")) {
        Ok(r) => {
            let link = r["link"].as_str().unwrap_or_default().to_owned();
            if let Some(img) = qr::image(&link) {
                ui.set_qr(img);
            }
            ui.set_link(link.into());
            ui.set_expired(false);
            ui.set_paired(false);
            ui.set_error(slint::SharedString::new());
            ui.set_left(clock(PAIRING_FOR.as_secs()).into());
            state.borrow_mut().opened = Some(Instant::now());
        }
        Err(e) => ui.set_error(format!("{}: {e}", t(lang, "err.daemon")).into()),
    }
}

fn tick_pairing(ui: &AppWindow, state: &RefCell<Pairing>) {
    let Some(opened) = state.borrow().opened else {
        return;
    };
    let left = PAIRING_FOR.saturating_sub(opened.elapsed());
    ui.set_left(clock(left.as_secs()).into());
    if left.is_zero() {
        ui.set_expired(true);
        state.borrow_mut().opened = None;
    }
}

/// Follows the daemon's events on a second connection, reconnecting while the daemon restarts.
fn follow_events(ui: slint::Weak<AppWindow>) {
    std::thread::Builder::new()
        .name("vutric-app-events".into())
        .spawn(move || {
            loop {
                if let Ok(mut c) = ipc::Conn::open()
                    && c.send(&ipc::op("subscribe")).is_ok()
                {
                    while let Ok(v) = c.next() {
                        if v["kind"] != "event" {
                            continue;
                        }
                        let ev = v["event"].as_str().unwrap_or_default().to_owned();
                        let ui = ui.clone();
                        if ui
                            .upgrade_in_event_loop(move |w| on_event(&w, &ev, &v))
                            .is_err()
                        {
                            return;
                        }
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        })
        .expect("spawn the event thread");
}

fn on_event(ui: &AppWindow, event: &str, _v: &serde_json::Value) {
    if event == "devices" && ui.get_page() == "pairing" && !ui.get_expired() {
        ui.set_paired(true);
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let page = std::env::args()
        .skip(1)
        .find_map(|a| a.strip_prefix("--page=").map(str::to_owned))
        .unwrap_or_else(|| "pairing".into());
    let lang = Lang::detect();
    let ui = AppWindow::new()?;
    texts(&ui, lang);
    ui.set_page(page.clone().into());

    let pairing = Rc::new(RefCell::new(Pairing::default()));
    if page == "pairing" {
        open_pairing(&ui, lang, &pairing);
    }
    {
        let (w, p) = (ui.as_weak(), Rc::clone(&pairing));
        ui.on_renew_pairing(move || {
            if let Some(ui) = w.upgrade() {
                open_pairing(&ui, lang, &p);
            }
        });
    }
    {
        let (w, p) = (ui.as_weak(), Rc::clone(&pairing));
        ui.on_page_changed(move |page| {
            if let Some(ui) = w.upgrade()
                && page == "pairing"
                && p.borrow().opened.is_none()
            {
                open_pairing(&ui, lang, &p);
            }
        });
    }
    let timer = Timer::default();
    {
        let (w, p) = (ui.as_weak(), Rc::clone(&pairing));
        timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            if let Some(ui) = w.upgrade() {
                tick_pairing(&ui, &p);
            }
        });
    }
    follow_events(ui.as_weak());

    ui.run()?;
    // A QR left on a closed window must stop working.
    if pairing.borrow().opened.is_some() {
        let _ = ipc::call(&ipc::op("close_pairing"));
    }
    Ok(())
}
