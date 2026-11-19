#![cfg_attr(windows, windows_subsystem = "windows")]

use std::io::Write as _;

/// Appends one JSON line from the page to `VUTRIC_M1_LOG` (the M1 measurement contract, ../README.md).
#[tauri::command]
fn probe(line: String) {
    let Some(path) = std::env::var_os("VUTRIC_M1_LOG") else {
        return;
    };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

#[tauri::command]
fn want_frames() -> u32 {
    std::env::var("VUTRIC_M1_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600)
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![probe, want_frames])
        .run(tauri::generate_context!())
        .expect("tauri runs");
}
