//! ADR-0066 (issue #11): the compat loop as a library, the way the phone runs it. The app side talks to it with
//! `Adapter::attach` over a stream instead of spawning `vutric-compat`: receive from a `LocalSend` sender, send to
//! a `LocalSend` receiver, refuse a received path outside staging. On Unix the same runs over a real socket pair
//! with `serve_socket_fd`, as the Android `:compat` service does.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::sync::mpsc;
use vutric_compat_localsend::client::{self, Target};
use vutric_compat_localsend::host::Adapter;
use vutric_compat_localsend::ipc::{FromAdapter, Start};
use vutric_compat_localsend::proto::DeviceType;
use vutric_compat_localsend::server::{self, Decision, Event, Receiver};
use vutric_compat_localsend::tls::Identity;

const LOCAL: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("vutric-lib-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(dir: &Path) -> Start {
    Start {
        alias: "Vutric phone".into(),
        device_type: DeviceType::Mobile,
        port: 0,
        bind: LOCAL,
        staging: dir.join("staging"),
        identity: dir.join("localsend.id"),
        pin: None,
        multicast: false,
        decide_timeout_secs: 10,
        quickshare: false,
        kdeconnect: false,
    }
}

async fn ready(events: &mut mpsc::Receiver<FromAdapter>) -> u16 {
    match tokio::time::timeout(Duration::from_secs(10), events.recv()).await {
        Ok(Some(FromAdapter::Ready { port, .. })) => port,
        other => panic!("no ready: {other:?}"),
    }
}

/// Receive one file from a `LocalSend` sender, then send one file to a `LocalSend` receiver, all through `adapter`.
async fn both_ways(dir: &Path, mut adapter: Adapter, mut events: mpsc::Receiver<FromAdapter>) {
    let port = ready(&mut events).await;

    let f = dir.join("photo.jpg");
    std::fs::write(&f, vec![3u8; 70_000]).unwrap();
    let send_id = Identity::generate().unwrap();
    let us = server::our_info("Laptop", DeviceType::Desktop, &send_id.fingerprint(), 53317);
    let upload = tokio::spawn(async move {
        client::send(
            &send_id,
            Target {
                addr: (LOCAL, port).into(),
                https: true,
            },
            &us,
            &[f],
            None,
        )
        .await
    });
    let mut saved = None;
    while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(10), events.recv()).await {
        match ev {
            FromAdapter::Offer {
                session_id, files, ..
            } => adapter
                .decide(&session_id, files.iter().map(|f| f.id.clone()).collect())
                .await
                .unwrap(),
            FromAdapter::Received { path, .. } => saved = adapter.received_file(&path),
            FromAdapter::Finished { .. } => break,
            _ => {}
        }
    }
    assert_eq!(upload.await.unwrap().unwrap().files, 1);
    let saved = saved.expect("a checked staged file");
    assert_eq!(std::fs::metadata(&saved).unwrap().len(), 70_000);
    assert!(adapter.received_file(&dir.join("photo.jpg")).is_none());

    // The other way: the phone sends to a LocalSend receiver on the LAN.
    let other = dir.join("other");
    let recv_id = Identity::generate().unwrap();
    let (rx, mut rx_events) = Receiver::start(
        server::Config {
            alias: "LocalSend laptop".into(),
            device_type: DeviceType::Desktop,
            port: 0,
            bind: LOCAL,
            staging: other.clone(),
            pin: None,
            decide_timeout: Duration::from_secs(10),
        },
        &recv_id,
    )
    .await
    .unwrap();
    let user = tokio::spawn(async move {
        while let Some(ev) = rx_events.recv().await {
            match ev {
                Event::Offer { offer, reply } => {
                    let ids = offer.files.iter().map(|f| f.id.clone()).collect();
                    reply.send(Decision::Accept(ids)).unwrap();
                }
                Event::Finished { .. } => return true,
                _ => {}
            }
        }
        false
    });
    let doc = dir.join("doc.pdf");
    std::fs::write(&doc, vec![9u8; 50_000]).unwrap();
    adapter
        .send(7, rx.addr(), true, vec![doc], None)
        .await
        .unwrap();
    let mut reported = false;
    while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(10), events.recv()).await {
        match ev {
            FromAdapter::Sent { request_id: 7, .. } => {
                reported = true;
                break;
            }
            FromAdapter::SendFailed { reason, .. } => panic!("send failed: {reason}"),
            _ => {}
        }
    }
    assert!(reported, "the adapter reported the send");
    assert!(
        tokio::time::timeout(Duration::from_secs(10), user)
            .await
            .unwrap()
            .unwrap()
    );
    adapter.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn attached_over_an_in_memory_stream() {
    let dir = tmp("mem");
    let (app_end, compat_end) = tokio::io::duplex(64 * 1024);
    let (cr, cw) = tokio::io::split(compat_end);
    let served = tokio::spawn(vutric_compat_localsend::serve::serve(cr, cw));
    let (ar, aw) = tokio::io::split(app_end);
    let (adapter, events) = Adapter::attach(ar, aw, start(&dir)).await.unwrap();
    both_ways(&dir, adapter, events).await;
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("the loop ends after Stop")
        .unwrap()
        .unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn first_line_must_be_start() {
    let (mut app_end, compat_end) = tokio::io::duplex(4096);
    let (cr, cw) = tokio::io::split(compat_end);
    let served = tokio::spawn(vutric_compat_localsend::serve::serve(cr, cw));
    tokio::io::AsyncWriteExt::write_all(&mut app_end, b"{\"announce\":null}\n")
        .await
        .unwrap();
    let r = tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .unwrap()
        .unwrap();
    assert!(r.is_err(), "a loop without start refuses to run");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn served_on_a_socket_pair_like_the_android_service() {
    use std::os::fd::IntoRawFd as _;
    let dir = tmp("fd");
    let (app_end, compat_end) = std::os::unix::net::UnixStream::pair().unwrap();
    let fd = compat_end.into_raw_fd();
    // SAFETY: `fd` is a fresh connected socket that only the service thread uses from here on.
    let service =
        std::thread::spawn(move || unsafe { vutric_compat_localsend::serve::serve_socket_fd(fd) });
    // SAFETY: the app's end, used only by the adapter from here on.
    let (adapter, events) =
        unsafe { Adapter::attach_socket_fd(app_end.into_raw_fd(), start(&dir)) }
            .await
            .unwrap();
    both_ways(&dir, adapter, events).await;
    service.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
