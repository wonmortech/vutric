//! The daemon's IPC from the window side: JSON lines over the per-user pipe (Windows) or socket (elsewhere), the
//! same wire as `crates/vutric-daemon/src/ipc.rs`. Kept free of the daemon crate so this process stays small.

use std::io::{self, BufRead as _, BufReader, Write as _};

use serde_json::{Value, json};

pub const VERSION: u32 = 1;

#[cfg(windows)]
type Stream = std::fs::File;
#[cfg(unix)]
type Stream = std::os::unix::net::UnixStream;

fn user() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
}

/// The first 16 hex digits of the user name's BLAKE3, as the daemon names its endpoint.
#[must_use]
pub fn short_name(user: &str) -> String {
    blake3::hash(user.as_bytes()).to_hex().as_str()[..16].to_owned()
}

#[cfg(windows)]
fn connect() -> io::Result<Stream> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(format!(r"\\.\pipe\vutric-{}", short_name(&user())))
}

#[cfg(unix)]
fn connect() -> io::Result<Stream> {
    let path = if let Some(p) = std::env::var_os("VUTRIC_IPC").filter(|p| !p.is_empty()) {
        std::path::PathBuf::from(p)
    } else if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME").ok_or_else(|| io::Error::other("no HOME"))?;
        std::path::PathBuf::from(home).join("Library/Application Support/Vutric/daemon.sock")
    } else {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .ok_or_else(|| io::Error::other("no XDG_RUNTIME_DIR"))?;
        std::path::PathBuf::from(dir).join(format!("vutric-{}.sock", short_name(&user())))
    };
    Stream::connect(path)
}

/// One connection, greeted.
pub struct Conn {
    rd: BufReader<Stream>,
    wr: Stream,
}

impl Conn {
    pub fn open() -> io::Result<Self> {
        let wr = connect()?;
        let mut c = Self {
            rd: BufReader::new(wr.try_clone()?),
            wr,
        };
        let hello = c.send(&json!({ "op": "hello", "version": VERSION }))?;
        if hello["kind"] != "hello" {
            return Err(io::Error::other(error_text(&hello)));
        }
        Ok(c)
    }

    /// Writes one request and reads the next line.
    pub fn send(&mut self, req: &Value) -> io::Result<Value> {
        let mut line = req.to_string();
        line.push('\n');
        self.wr.write_all(line.as_bytes())?;
        self.wr.flush()?;
        self.next()
    }

    /// The next reply or event line.
    pub fn next(&mut self) -> io::Result<Value> {
        let mut line = String::new();
        if self.rd.read_line(&mut line)? == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        serde_json::from_str(&line).map_err(io::Error::other)
    }
}

fn error_text(reply: &Value) -> String {
    reply["message"]
        .as_str()
        .map_or_else(|| reply.to_string(), str::to_owned)
}

/// One request on a fresh connection. `Err` carries the daemon's message, or why it could not be reached.
pub fn call(req: &Value) -> Result<Value, String> {
    let mut c = Conn::open().map_err(|e| format!("daemon: {e}"))?;
    let reply = c.send(req).map_err(|e| format!("daemon: {e}"))?;
    if reply["kind"] == "error" {
        Err(error_text(&reply))
    } else {
        Ok(reply)
    }
}

/// `{"op": op}`.
#[must_use]
pub fn op(op: &str) -> Value {
    json!({ "op": op })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_name_matches_the_daemon() {
        // blake3("") = af1349b9f5f9a1a6..., the daemon's `endpoint_name("")` uses the same 16 digits.
        assert_eq!(short_name(""), "af1349b9f5f9a1a6");
    }

    #[test]
    fn errors_show_the_daemon_message() {
        let r = json!({ "kind": "error", "message": "no phone connected" });
        assert_eq!(error_text(&r), "no phone connected");
        assert_eq!(op("status"), json!({ "op": "status" }));
    }
}
