#![cfg_attr(windows, windows_subsystem = "windows")]

mod probe;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let probe = probe::Probe::from_env();
    let ui = SettingsWindow::new()?;
    let p = probe.clone();
    let notified = ui.window().set_rendering_notifier(move |state, _| {
        if matches!(state, slint::RenderingState::AfterRendering) {
            p.frame();
        }
    });
    if notified.is_err() {
        // The software renderer (SLINT_BACKEND=winit-software) has no rendering notifier. Every frame it draws is
        // one winit RedrawRequested, so those are timed instead.
        use slint::winit_030::{EventResult, WinitWindowAccessor as _, winit::event::WindowEvent};
        let p = probe.clone();
        ui.window().on_winit_window_event(move |_, ev| {
            if matches!(ev, WindowEvent::RedrawRequested) {
                p.frame();
            }
            EventResult::Propagate
        });
    }
    ui.run()?;
    probe.finish();
    Ok(())
}
