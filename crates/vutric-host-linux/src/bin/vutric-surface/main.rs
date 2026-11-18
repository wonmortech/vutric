//! `vutric-surface`: the process that owns the GPU and the Wayland surfaces on Linux (ADR-0090 §3).
//!
//! ```text
//! vutric-surface --socket $XDG_RUNTIME_DIR/vutric/surface-1234.sock [--backend auto|layer|gnome] [--output DP-1]
//! vutric-surface --self-test 5 [--out stats.json] [--cards 50]
//! ```
//!
//! With `--socket` it connects to the daemon and obeys [`vutric_host_linux::proto`]. `--self-test` plays the Halqa
//! effect N times on the real compositor (and, on layer-shell, shows N synthetic cards) and writes the frame
//! numbers as JSON: the CI job runs it under headless sway.

#[cfg(target_os = "linux")]
mod app;

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    app::main()
}

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!("vutric-surface runs on Linux only");
    std::process::ExitCode::from(2)
}
