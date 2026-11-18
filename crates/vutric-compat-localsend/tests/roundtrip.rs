//! Vutric's `LocalSend` sender against Vutric's `LocalSend` receiver over real HTTPS on loopback: accept part of
//! an offer, reject, PIN, and the full path through the sandboxed adapter process.

use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use vutric_compat_localsend::client::{self, Refusal, Target};
use vutric_compat_localsend::host::Adapter;
use vutric_compat_localsend::ipc::{FromAdapter, Start};
use vutric_compat_localsend::proto::DeviceType;
use vutric_compat_localsend::server::{self, Decision, Event, Receiver};
use vutric_compat_localsend::tls::Identity;

const LOCAL: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("vutric-ls-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn config(staging: PathBuf, pin: Option<&str>) -> server::Config {
    server::Config {
        alias: "Vutric test".into(),
        device_type: DeviceType::Desktop,
        port: 0,
        bind: LOCAL,
        staging,
        pin: pin.map(str::to_owned),
        decide_timeout: Duration::from_secs(10),
    }
}

fn sender() -> (Identity, vutric_compat_localsend::proto::DeviceInfo) {
    let id = Identity::generate().unwrap();
    let us = server::our_info("Sender", DeviceType::Mobile, &id.fingerprint(), 53317);
    (id, us)
}

#[tokio::test(flavor = "multi_thread")]
async fn accept_one_of_two_files() {
    let dir = tmp("accept");
    let staging = dir.join("staging");
    let (recv_id, (send_id, us)) = (Identity::generate().unwrap(), sender());
    let (rx, mut events) = Receiver::start(config(staging.clone(), None), &recv_id)
        .await
        .unwrap();
    let a = dir.join("photo.jpg");
    let b = dir.join("notes.txt");
    let big: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&a, &big).unwrap();
    std::fs::write(&b, b"hello").unwrap();

    let user = tokio::spawn(async move {
        let mut got = Vec::new();
        while let Some(ev) = events.recv().await {
            match ev {
                Event::Offer { offer, reply } => {
                    assert_eq!(offer.from.alias, "Sender");
                    assert_eq!(offer.files.len(), 2);
                    assert!(
                        offer.peer_cert.is_some(),
                        "sender presented its certificate"
                    );
                    let jpg = offer
                        .files
                        .iter()
                        .find(|f| f.file_name == "photo.jpg")
                        .unwrap();
                    reply.send(Decision::Accept(vec![jpg.id.clone()])).unwrap();
                }
                Event::Received {
                    name, path, size, ..
                } => got.push((name, path, size)),
                Event::Finished { .. } => return got,
                _ => {}
            }
        }
        got
    });
    let t0 = Instant::now();
    let sent = client::send(
        &send_id,
        Target {
            addr: rx.addr(),
            https: true,
        },
        &us,
        &[a, b],
        None,
    )
    .await
    .unwrap();
    let took = t0.elapsed();
    assert_eq!(sent.files, 1);
    assert_eq!(sent.peer_cert.as_deref(), Some(rx.fingerprint()));
    let got = user.await.unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, "photo.jpg");
    assert_eq!(std::fs::read(&got[0].1).unwrap(), big);
    println!(
        "3 MB over loopback HTTPS in {took:?} ({:.0} MB/s)",
        3.0 / took.as_secs_f64()
    );
    assert!(
        std::fs::read_dir(&staging).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".vutric-part")),
        "no part file left"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn reject_and_pin() {
    let dir = tmp("reject");
    let f = dir.join("x.bin");
    std::fs::write(&f, b"x").unwrap();
    let (send_id, us) = sender();

    let (rx, mut events) =
        Receiver::start(config(dir.join("s1"), None), &Identity::generate().unwrap())
            .await
            .unwrap();
    tokio::spawn(async move {
        while let Some(ev) = events.recv().await {
            if let Event::Offer { reply, .. } = ev {
                let _ = reply.send(Decision::Reject);
            }
        }
    });
    let t = Target {
        addr: rx.addr(),
        https: true,
    };
    let e = client::send(&send_id, t, &us, std::slice::from_ref(&f), None)
        .await
        .unwrap_err();
    assert_eq!(e.downcast_ref::<Refusal>(), Some(&Refusal::Rejected));

    let (rx2, _events2) = Receiver::start(
        config(dir.join("s2"), Some("123456")),
        &Identity::generate().unwrap(),
    )
    .await
    .unwrap();
    let t2 = Target {
        addr: rx2.addr(),
        https: true,
    };
    let e = client::send(&send_id, t2, &us, std::slice::from_ref(&f), Some("000000"))
        .await
        .unwrap_err();
    assert_eq!(e.downcast_ref::<Refusal>(), Some(&Refusal::PinRequired));

    let (info, cert) = client::register(&send_id, t2, &us).await.unwrap();
    assert_eq!(info.alias, "Vutric test");
    assert_eq!(cert.as_deref(), Some(rx2.fingerprint()));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn through_the_sandboxed_process() {
    let dir = tmp("proc");
    let staging = dir.join("staging");
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_vutric-compat"));
    let (mut adapter, mut events) = Adapter::spawn(
        &exe,
        Start {
            alias: "Vutric proc".into(),
            device_type: DeviceType::Desktop,
            port: 0,
            bind: LOCAL,
            staging: staging.clone(),
            identity: dir.join("localsend.id"),
            pin: None,
            multicast: false,
            decide_timeout_secs: 10,
            quickshare: false,
            kdeconnect: false,
        },
    )
    .await
    .unwrap();
    let port = match tokio::time::timeout(Duration::from_secs(10), events.recv()).await {
        Ok(Some(FromAdapter::Ready { port, .. })) => port,
        other => panic!("no ready: {other:?}"),
    };
    let f = dir.join("doc.pdf");
    std::fs::write(&f, vec![7u8; 100_000]).unwrap();
    let (send_id, us) = sender();
    let send = tokio::spawn(async move {
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
            } => {
                adapter
                    .decide(&session_id, files.iter().map(|f| f.id.clone()).collect())
                    .await
                    .unwrap();
            }
            FromAdapter::Received { path, .. } => {
                saved = adapter.received_file(&path);
            }
            FromAdapter::Finished { .. } => break,
            _ => {}
        }
    }
    assert_eq!(send.await.unwrap().unwrap().files, 1);
    let saved = saved.expect("a checked staged file");
    assert_eq!(std::fs::metadata(&saved).unwrap().len(), 100_000);
    assert!(
        adapter.received_file(&dir.join("doc.pdf")).is_none(),
        "outside staging"
    );
    adapter.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn quickshare_through_the_sandboxed_process() {
    use vutric_compat_localsend::ipc::Via;
    let dir = tmp("qsproc");
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_vutric-compat"));
    let (mut adapter, mut events) = Adapter::spawn(
        &exe,
        Start {
            alias: "Vutric proc".into(),
            device_type: DeviceType::Desktop,
            port: 0,
            bind: LOCAL,
            staging: dir.join("staging"),
            identity: dir.join("localsend.id"),
            pin: None,
            multicast: false,
            decide_timeout_secs: 10,
            quickshare: true,
            kdeconnect: false,
        },
    )
    .await
    .unwrap();
    let port = match tokio::time::timeout(Duration::from_secs(10), events.recv()).await {
        Ok(Some(FromAdapter::Ready {
            quickshare_port: Some(p),
            ..
        })) => p,
        other => panic!("no ready: {other:?}"),
    };
    let f = dir.join("IMG_0007.jpg");
    std::fs::write(&f, vec![9u8; 700_000]).unwrap();
    let send = tokio::spawn(async move {
        vutric_compat_quickshare::client::send((LOCAL, port).into(), "Pixel", &[f]).await
    });
    let (mut code, mut saved) = (None, None);
    while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(10), events.recv()).await {
        match ev {
            FromAdapter::Offer {
                session_id,
                via,
                code: c,
                alias,
                ..
            } => {
                assert_eq!(via, Via::Quickshare);
                assert_eq!(alias, "Pixel");
                code = c;
                adapter.decide(&session_id, vec!["0".into()]).await.unwrap();
            }
            FromAdapter::Received { path, .. } => saved = adapter.received_file(&path),
            FromAdapter::Finished { .. } => break,
            _ => {}
        }
    }
    let outcome = send.await.unwrap().unwrap();
    assert_eq!(
        Some(outcome.pin),
        code,
        "the daemon gets the code the sender shows"
    );
    assert_eq!(
        std::fs::metadata(saved.expect("checked staged file"))
            .unwrap()
            .len(),
        700_000
    );
    adapter.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn kdeconnect_through_the_sandboxed_process() {
    use vutric_compat_kdeconnect::link::{Link, Me};
    use vutric_compat_kdeconnect::proto::DeviceType as KdeType;
    use vutric_compat_localsend::ipc::Via;
    let dir = tmp("kdcproc");
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_vutric-compat"));
    let (mut adapter, mut events) = Adapter::spawn(
        &exe,
        Start {
            alias: "Vutric proc".into(),
            device_type: DeviceType::Desktop,
            port: 0,
            bind: LOCAL,
            staging: dir.join("staging"),
            identity: dir.join("localsend.id"),
            pin: None,
            multicast: false,
            decide_timeout_secs: 10,
            quickshare: false,
            kdeconnect: true,
        },
    )
    .await
    .unwrap();
    let (port, id) = match tokio::time::timeout(Duration::from_secs(10), events.recv()).await {
        Ok(Some(FromAdapter::Ready {
            kdeconnect_port: Some(p),
            kdeconnect_id: Some(id),
            ..
        })) => (p, id),
        other => panic!("no ready: {other:?}"),
    };
    let phone = Me {
        tls: vutric_compat_kdeconnect::tls::Identity::generate().unwrap(),
        name: "Pixel 8".into(),
        kind: KdeType::Phone,
    };
    let f = dir.join("video.mp4");
    std::fs::write(&f, vec![3u8; 250_000]).unwrap();
    let mut link = Link::dial(&phone, (LOCAL, port).into(), &id).await.unwrap();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let phone2 = phone.clone();
    let send = tokio::spawn(async move {
        assert!(
            link.request_pair(ts, Duration::from_secs(10))
                .await
                .unwrap()
        );
        link.send_files(&phone2, &[f], LOCAL, 0..=0).await
    });
    let (mut code, mut paired, mut saved) = (None, false, None);
    while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(10), events.recv()).await {
        match ev {
            FromAdapter::PairRequest {
                session_id,
                via,
                alias,
                code: c,
                ..
            } => {
                assert_eq!(via, Via::Kdeconnect);
                assert_eq!(alias, "Pixel 8");
                code = Some(c);
                adapter
                    .decide(&session_id, vec!["pair".into()])
                    .await
                    .unwrap();
            }
            FromAdapter::Paired { .. } => paired = true,
            FromAdapter::Received { path, .. } => {
                saved = adapter.received_file(&path);
                break;
            }
            _ => {}
        }
    }
    send.await.unwrap().unwrap();
    assert!(paired);
    assert_eq!(code.map(|c| c.len()), Some(8));
    assert_eq!(
        std::fs::metadata(saved.expect("checked staged file"))
            .unwrap()
            .len(),
        250_000
    );
    assert!(
        dir.join("kdeconnect-trusted.json").exists(),
        "pairing is kept"
    );
    adapter.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
