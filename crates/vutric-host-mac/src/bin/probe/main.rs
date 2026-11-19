//! Frame-pacing probe for the macOS overlay, the counterpart of the Windows Phase 0.1 spike: plays the Halqa effect
//! on the real `WindowServer` and writes fps, frame times, missed refreshes and the focus checks as JSON.
//!
//! ```text
//! cargo run -p vutric-host-mac --release --bin vutric-mac-probe -- --runs 5 --window panel --json pacing.json
//! ```
//!
//! Exit code: 0 = exit criteria met, 1 = measured but not met, 2 = could not measure (no GPU, no display). The
//! JSON is written in every case, with `error` set when nothing was measured.

#[cfg(target_os = "macos")]
mod mac;

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    mac::main()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("vutric-mac-probe runs on macOS only");
}
