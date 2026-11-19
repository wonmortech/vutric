//! C7 through the sandboxed `vutric-compat` process against the real `wormhole` tool (ADR-0067). Needs the same
//! environment as `crates/vutric-compat-wormhole/tests/interop.rs` (`VUTRIC_WORMHOLE_BIN`,
//! `VUTRIC_WORMHOLE_MAILBOX`, `VUTRIC_WORMHOLE_RELAY`), else it returns early.

use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use vutric_compat_localsend::host::Adapter;
use vutric_compat_localsend::ipc::{FromAdapter, Start, ToAdapter, Via, WormholeServers};
use vutric_compat_localsend::proto::DeviceType;

#[tokio::test(flavor = "multi_thread")]
#[allow(clippy::too_many_lines)] // one scenario, both directions, read top to bottom
async fn wormhole_both_ways_through_the_process() {
    let (Ok(bin), Ok(mailbox), Ok(relay)) = (
        std::env::var("VUTRIC_WORMHOLE_BIN"),
        std::env::var("VUTRIC_WORMHOLE_MAILBOX"),
        std::env::var("VUTRIC_WORMHOLE_RELAY"),
    ) else {
        eprintln!(
            "skipped: set VUTRIC_WORMHOLE_BIN, VUTRIC_WORMHOLE_MAILBOX and VUTRIC_WORMHOLE_RELAY"
        );
        return;
    };
    let servers = WormholeServers {
        mailbox: Some(mailbox.clone()),
        relay: Some(relay.clone()),
    };
    let dir = std::env::temp_dir().join(format!("vutric-whp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_vutric-compat"));
    let (mut adapter, mut events) = Adapter::spawn(
        &exe,
        Start {
            alias: "Vutric".into(),
            device_type: DeviceType::Desktop,
            port: 0,
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            staging: dir.join("staging"),
            identity: dir.join("localsend.id"),
            pin: None,
            multicast: false,
            decide_timeout_secs: 30,
            quickshare: false,
            kdeconnect: false,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(10), events.recv()).await,
        Ok(Some(FromAdapter::Ready { .. }))
    ));
    let tool = |args: &[&str]| {
        let mut c = tokio::process::Command::new(&bin);
        c.args(["--relay-url", &mailbox, "--transit-helper", &relay])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        c
    };

    // 1. The real tool sends a file; the process asks, we accept, the file is staged and checked.
    let src = dir.join("photo.jpg");
    std::fs::write(&src, vec![5u8; 300_000]).unwrap();
    let code = format!("{}-4242-4242", 200 + std::process::id() % 1000);
    let mut sender = tool(&["send", "--hide-progress", "--code", &code])
        .arg(&src)
        .spawn()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    adapter
        .wormhole(ToAdapter::WormholeReceive {
            request_id: 1,
            code,
            servers: servers.clone(),
        })
        .await
        .unwrap();
    let mut staged = None;
    while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(30), events.recv()).await {
        match ev {
            FromAdapter::Offer {
                session_id,
                via,
                files,
                ..
            } => {
                assert_eq!(via, Via::Wormhole);
                assert_eq!(
                    (files[0].name.as_str(), files[0].size),
                    ("photo.jpg", 300_000)
                );
                adapter.decide(&session_id, vec!["0".into()]).await.unwrap();
            }
            FromAdapter::Received { path, .. } => staged = adapter.received_file(&path),
            FromAdapter::Finished { .. } => break,
            FromAdapter::WormholeFailed { reason, .. } => panic!("receive failed: {reason}"),
            _ => {}
        }
    }
    let staged = staged.expect("a checked staged file");
    assert_eq!(std::fs::read(staged).unwrap(), vec![5u8; 300_000]);
    assert!(
        tokio::time::timeout(Duration::from_secs(30), sender.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );

    // 2. We send a text; the process reports the code, the real tool receives it.
    adapter
        .wormhole(ToAdapter::WormholeSend {
            request_id: 2,
            paths: vec![],
            text: Some("Vutric jarayonidan salom".into()),
            servers,
        })
        .await
        .unwrap();
    let code = match tokio::time::timeout(Duration::from_secs(10), events.recv()).await {
        Ok(Some(FromAdapter::WormholeCode {
            request_id: 2,
            code,
        })) => code,
        other => panic!("no code: {other:?}"),
    };
    let receiver = tool(&["receive", "--hide-progress", "--only-text", &code])
        .spawn()
        .unwrap();
    let out = tokio::time::timeout(Duration::from_secs(30), receiver.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("Vutric jarayonidan salom"));
    match tokio::time::timeout(Duration::from_secs(10), events.recv()).await {
        Ok(Some(FromAdapter::Sent { request_id: 2, .. })) => {}
        other => panic!("no sent: {other:?}"),
    }
    adapter.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
