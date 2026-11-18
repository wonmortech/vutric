//! The desktop command line end to end (backlog F10, ADR-0069): `receive --json` in one process, then `clipboard`,
//! `notice`, `ring` and `send` from another, as a script would drive them. The first link pairs both sides; the
//! receiver reports every event as one JSON line.

use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_vutric-net-spike");

fn home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vutric-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The command with its data and config folders inside `home`, so the test never touches the real ones.
fn cmd(home: &Path, mode: &str) -> Command {
    let mut c = Command::new(BIN);
    c.arg(mode);
    for k in ["APPDATA", "LOCALAPPDATA", "HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME"] {
        c.env(k, home);
    }
    c.arg("--lan-only");
    c
}

struct Receiver {
    child: Child,
    lines: mpsc::Receiver<serde_json::Value>,
    link: String,
}

impl Receiver {
    fn start(home: &Path, out: &Path) -> Self {
        let mut child = cmd(home, "receive")
            .args(["--json", "--dir"])
            .arg(out)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start receive");
        let (link_tx, link_rx) = mpsc::channel();
        let err = BufReader::new(child.stderr.take().unwrap());
        std::thread::spawn(move || {
            for line in err.lines().map_while(Result::ok) {
                if let Some(link) = line.strip_prefix("pairing link: ") {
                    let _ = link_tx.send(link.to_owned());
                }
            }
        });
        let (tx, lines) = mpsc::channel();
        let out = BufReader::new(child.stdout.take().unwrap());
        std::thread::spawn(move || {
            for line in out.lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str(&line) {
                    let _ = tx.send(v);
                }
            }
        });
        let link = link_rx
            .recv_timeout(Duration::from_secs(30))
            .expect("receive prints its pairing link");
        Self { child, lines, link }
    }

    /// The next event of `kind`, skipping others (connected, closed).
    fn next(&self, kind: &str) -> serde_json::Value {
        loop {
            let v = self
                .lines
                .recv_timeout(Duration::from_secs(30))
                .unwrap_or_else(|_| panic!("no {kind} event"));
            if v["event"] == kind {
                return v;
            }
        }
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn run(home: &Path, args: &[&str], link: &str) -> std::process::Output {
    cmd(home, args[0])
        .args(&args[1..])
        .args(["--ticket", link])
        .stdin(Stdio::null())
        .output()
        .expect("run")
}

#[test]
fn a_script_drives_clipboard_notice_ring_and_files() {
    let (rx_home, tx_home) = (home("rx"), home("tx"));
    let out = rx_home.join("in");
    let r = Receiver::start(&rx_home, &out);

    let o = run(&tx_home, &["clipboard", "salom", "dunyo"], &r.link);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(r.next("paired")["text"], "scanned the QR code");
    assert_eq!(r.next("clipboard")["text"], "salom dunyo");

    let o = run(
        &tx_home,
        &["notice", "--app", "Telegram", "--title", "Ali", "kelyapman\u{202E}"],
        &r.link,
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let n = r.next("notice");
    assert_eq!(
        (&n["app"], &n["title"], &n["body"]),
        (&"Telegram".into(), &"Ali".into(), &"kelyapman".into()),
        "bidi override removed"
    );

    let o = run(&tx_home, &["ring", "--stop"], &r.link);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    r.next("ring-stopped");

    let file = tx_home.join("a.txt");
    std::fs::write(&file, b"hello").unwrap();
    let o = run(&tx_home, &["send", file.to_str().unwrap()], &r.link);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(r.next("files")["files"], 1);
    assert_eq!(std::fs::read(out.join("a.txt")).unwrap(), b"hello");

    let o = cmd(&tx_home, "devices").arg("--json").output().unwrap();
    let listed = String::from_utf8_lossy(&o.stdout);
    assert_eq!(listed.lines().count(), 1, "{listed}");
    assert!(listed.contains("\"Ticket\""), "{listed}");

    drop(r);
    let _ = std::fs::remove_dir_all(&rx_home);
    let _ = std::fs::remove_dir_all(&tx_home);
}

#[test]
fn a_ring_nobody_stops_exits_with_three() {
    let (rx_home, tx_home) = (home("rx3"), home("tx3"));
    let r = Receiver::start(&rx_home, &rx_home.join("in"));
    let o = run(&tx_home, &["ring", "--seconds", "1"], &r.link);
    assert_eq!(o.status.code(), Some(3), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(r.next("ring")["seconds"], 1);
    drop(r);
    let _ = std::fs::remove_dir_all(&rx_home);
    let _ = std::fs::remove_dir_all(&tx_home);
}
