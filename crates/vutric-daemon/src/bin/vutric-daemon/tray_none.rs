//! Platforms without a tray backend yet. The Linux `StatusNotifierItem` over D-Bus arrives with D10; until then the
//! daemon runs without an icon and is reached through its IPC (`vutric-daemon ctl show`).

#![allow(
    dead_code,
    clippy::unused_self,
    reason = "mirrors the Windows tray API; nothing constructs it until D10"
)]

use anyhow::{Result, bail};
use vutric_daemon::i18n::Lang;
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

pub struct Tray;

impl Tray {
    pub fn new(_: Lang, _: &EventLoopProxy<UiEvent>) -> Result<Self> {
        bail!("no tray backend on this platform yet (D10)")
    }

    pub fn action(&self, _: &MenuId) -> Option<Action> {
        None
    }

    pub fn set_paused(&self, _: bool) {}
}
