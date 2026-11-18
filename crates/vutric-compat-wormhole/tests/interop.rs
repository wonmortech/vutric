//! C7 against the real `wormhole` tool (magic-wormhole, Python) and its servers, ADR-0067.
//!
//! Needs three environment variables, else every test returns early and says so:
//! - `VUTRIC_WORMHOLE_BIN`: the `wormhole` executable;
//! - `VUTRIC_WORMHOLE_MAILBOX`: a mailbox server, e.g. `ws://127.0.0.1:14000/v1`
//!   (`twist wormhole-mailbox --port=tcp:14000`);
//! - `VUTRIC_WORMHOLE_RELAY`: a transit relay, e.g. `tcp:127.0.0.1:14001` (`twist transitrelay --port=tcp:14001`).
//!
//! CI runs them in the `interop-wormhole` job (`.github/workflows/interop.yml`).

use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio::process::Command;
use vutric_compat_wormhole::code::Code;
use vutric_compat_wormhole::transfer::{self, APPID, Config, Offer};
use vutric_compat_wormhole::wormhole::Wormhole;

struct Env {
    bin: PathBuf,
    cfg: Config,
}

fn env() -> Option<Env> {
    let (Ok(bin), Ok(mailbox), Ok(relay)) = (
        std::env::var("VUTRIC_WORMHOLE_BIN"),
        std::env::var("VUTRIC_WORMHOLE_MAILBOX"),
        std::env::var("VUTRIC_WORMHOLE_RELAY"),
    ) else {
        eprintln!(
            "skipped: set VUTRIC_WORMHOLE_BIN, VUTRIC_WORMHOLE_MAILBOX and VUTRIC_WORMHOLE_RELAY"
        );
        return None;
    };
    Some(Env {
        bin: bin.into(),
        cfg: Config {
            mailbox,
            relay: Some(relay),
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
        },
    })
}

fn tool(e: &Env) -> Command {
    let mut c = Command::new(&e.bin);
    c.arg("--relay-url")
        .arg(&e.cfg.mailbox)
        .arg("--transit-helper")
        .arg(e.cfg.relay.as_deref().unwrap())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    c
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("vutric-wh-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn fixed_code(tag: u8) -> Code {
    Code::parse(&format!(
        "{}-{:04}-{:04}",
        100 + u32::from(tag),
        std::process::id() % 10_000,
        tag
    ))
    .unwrap()
}

/// Real `wormhole send --text` → our receiver.
#[tokio::test(flavor = "multi_thread")]
async fn receive_text_from_the_real_tool() {
    let Some(e) = env() else { return };
    let code = fixed_code(1);
    let mut child = tool(&e)
        .args([
            "send",
            "--hide-progress",
            "--code",
            code.as_str(),
            "--text",
            "salom dunyo",
        ])
        .spawn()
        .unwrap();
    let mut w = Wormhole::join(&e.cfg.mailbox, APPID, &code)
        .await
        .expect("join");
    let (offer, _) = transfer::receive_offer(&mut w).await.expect("offer");
    assert_eq!(offer, Offer::Text("salom dunyo".into()));
    transfer::accept_text(&mut w).await.unwrap();
    w.close("happy").await;
    let out = tokio::time::timeout(Duration::from_secs(30), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(out.success(), "wormhole send exited with {out}");
}

/// Real `wormhole send <file>` → our receiver, through transit.
#[tokio::test(flavor = "multi_thread")]
async fn receive_file_from_the_real_tool() {
    let Some(e) = env() else { return };
    let dir = tmp("rf");
    let src = dir.join("rasm.bin");
    let data: Vec<u8> = (0..1_500_000u32).map(|i| (i * 7 % 251) as u8).collect();
    std::fs::write(&src, &data).unwrap();
    let code = fixed_code(2);
    let mut child = tool(&e)
        .args(["send", "--hide-progress", "--code", code.as_str()])
        .arg(&src)
        .spawn()
        .unwrap();
    let mut w = Wormhole::join(&e.cfg.mailbox, APPID, &code)
        .await
        .expect("join");
    let (offer, hints) = transfer::receive_offer(&mut w).await.expect("offer");
    let Offer::File { name, size } = offer else {
        panic!("{offer:?}")
    };
    assert_eq!((name.as_str(), size), ("rasm.bin", data.len() as u64));
    let part = dir.join("in.part");
    let got = transfer::accept_file(&mut w, size, hints, &part, &e.cfg, &|_| {})
        .await
        .expect("receive file");
    assert_eq!(got, size);
    assert_eq!(std::fs::read(&part).unwrap(), data);
    w.close("happy").await;
    let out = tokio::time::timeout(Duration::from_secs(30), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(out.success(), "wormhole send exited with {out}");
    let _ = std::fs::remove_dir_all(dir);
}

/// Our sender with a generated code → real `wormhole receive` (text on its stdout).
#[tokio::test(flavor = "multi_thread")]
async fn send_text_to_the_real_tool() {
    let Some(e) = env() else { return };
    let pending = Wormhole::create(&e.cfg.mailbox, APPID)
        .await
        .expect("allocate");
    let code = pending.code.clone();
    let child = tool(&e)
        .args(["receive", "--hide-progress", "--only-text", code.as_str()])
        .spawn()
        .unwrap();
    let mut w = pending.open().await.expect("open");
    transfer::send_text(&mut w, "Vutric'dan salom")
        .await
        .expect("send text");
    w.close("happy").await;
    let out = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("Vutric'dan salom"));
}

/// Our sender → real `wormhole receive --accept-file`, through transit.
#[tokio::test(flavor = "multi_thread")]
async fn send_file_to_the_real_tool() {
    let Some(e) = env() else { return };
    let dir = tmp("sf");
    let src = dir.join("hujjat.pdf");
    let data: Vec<u8> = (0..900_000u32).map(|i| (i % 253) as u8).collect();
    std::fs::write(&src, &data).unwrap();
    let out_file = dir.join("got.pdf");
    let pending = Wormhole::create(&e.cfg.mailbox, APPID)
        .await
        .expect("allocate");
    let code = pending.code.clone();
    let mut child = tool(&e)
        .args(["receive", "--hide-progress", "--accept-file", "-o"])
        .arg(&out_file)
        .arg(code.as_str())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let log = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut all = String::new();
        while let Ok(Some(l)) = lines.next_line().await {
            all.push_str(&l);
            all.push('\n');
        }
        all
    });
    let mut w = pending.open().await.expect("open");
    let sent = transfer::send_file(&mut w, &src, &e.cfg, &|_| {})
        .await
        .expect("send file");
    assert_eq!(sent, data.len() as u64);
    w.close("happy").await;
    let status = tokio::time::timeout(Duration::from_secs(30), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(status.success(), "{}", log.await.unwrap());
    assert_eq!(std::fs::read(&out_file).unwrap(), data);
    let _ = std::fs::remove_dir_all(dir);
}

/// A wrong code never opens: the real sender and we disagree on the key.
#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_code_does_not_open() {
    let Some(e) = env() else { return };
    let code = fixed_code(5);
    let wrong = Code::parse(&format!("{}-0000-0000", code.nameplate())).unwrap();
    let mut child = tool(&e)
        .args([
            "send",
            "--hide-progress",
            "--code",
            code.as_str(),
            "--text",
            "maxfiy",
        ])
        .spawn()
        .unwrap();
    let r = tokio::time::timeout(
        Duration::from_secs(30),
        Wormhole::join(&e.cfg.mailbox, APPID, &wrong),
    )
    .await
    .expect("finishes");
    assert!(r.is_err(), "a wrong code must not open the wormhole");
    let _ = child.kill().await;
}

/// Two of our transits that can only meet through the real transit relay (no direct hints either way).
#[tokio::test(flavor = "multi_thread")]
async fn transit_through_the_real_relay() {
    use vutric_compat_wormhole::transit::{Hints, Role, Transit, parse_relay, transit_side};
    let Some(e) = env() else { return };
    let relay = parse_relay(e.cfg.relay.as_deref().unwrap()).unwrap();
    let key = [42u8; 32];
    let lo = e.cfg.bind;
    let a = Transit::new(key, Role::Sender, &transit_side(), lo, Some(relay.clone()))
        .await
        .unwrap();
    let b = Transit::new(key, Role::Receiver, &transit_side(), lo, Some(relay))
        .await
        .unwrap();
    let nobody = Hints::default();
    let tb = tokio::spawn(async move { b.connect(&Hints::default()).await });
    let mut ra = a.connect(&nobody).await.expect("sender through the relay");
    let mut rb = tb.await.unwrap().expect("receiver through the relay");
    let relay_port = parse_relay(e.cfg.relay.as_deref().unwrap()).unwrap().1;
    assert_eq!(
        ra.peer().unwrap().port(),
        relay_port,
        "the pipe goes through the relay"
    );
    ra.send(&vec![1u8; 300_000]).await.unwrap();
    assert_eq!(rb.recv().await.unwrap().len(), 300_000);
    rb.send(b"ok").await.unwrap();
    assert_eq!(ra.recv().await.unwrap(), b"ok");
}
