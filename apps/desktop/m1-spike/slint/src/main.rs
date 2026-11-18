#![cfg_attr(windows, windows_subsystem = "windows")]

mod probe;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let probe = probe::Probe::from_env();
    let ui = SettingsWindow::new()?;
    let p = probe.clone();
    // The software renderer (SLINT_BACKEND=winit-software) has no rendering notifier: it then runs unprobed.
    let _ = ui.window().set_rendering_notifier(move |state, _| {
        if matches!(state, slint::RenderingState::AfterRendering) {
            p.frame();
        }
    });
    ui.run()?;
    probe.finish();
    Ok(())
}
