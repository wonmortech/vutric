//! Vutric's Quick Share sender against Vutric's Quick Share receiver on loopback, plus a hostile sender that
//! breaks the rules after the user accepted, and the mDNS advert read back by a browser.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use vutric_compat_quickshare::client;
use vutric_compat_quickshare::conn::{self, Bytes, Piece};
use vutric_compat_quickshare::endpoint::{self, Device};
use vutric_compat_quickshare::msg::{self, PayloadHeader, Share};
use vutric_compat_quickshare::server::{Config, Decision, Event, Receiver};
use vutric_compat_quickshare::ukey2::{self, Channel};

const LOCAL: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("vutric-qs-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

async fn receiver(dir: &Path) -> (Receiver, tokio::sync::mpsc::Receiver<Event>) {
    Receiver::start(Config {
        name: "Ota kompyuteri".into(),
        device: Device::Laptop,
        port: 0,
        bind: LOCAL,
        staging: dir.join("staging"),
        decide_timeout: Duration::from_secs(10),
    })
    .await
    .unwrap()
}

fn no_parts(staging: &Path) -> bool {
    std::fs::read_dir(staging).unwrap().all(|e| {
        !e.unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".vutric-part")
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn accept_two_files_with_many_chunks() {
    let dir = tmp("accept");
    let (rx, mut events) = receiver(&dir).await;
    let big: Vec<u8> = (0..3_200_000u32).map(|i| (i % 249) as u8).collect();
    let a = dir.join("VID_2026.mp4");
    let b = dir.join("empty.txt");
    std::fs::write(&a, &big).unwrap();
    std::fs::write(&b, b"").unwrap();
    let user = tokio::spawn(async move {
        let mut got = Vec::new();
        let mut pin = String::new();
        while let Some(ev) = events.recv().await {
            match ev {
                Event::Offer { offer, reply } => {
                    assert_eq!(offer.from.as_deref(), Some("Telefon"));
                    assert_eq!(offer.device, Device::Laptop);
                    assert_eq!(offer.files.len(), 2);
                    pin.clone_from(&offer.pin);
                    reply.send(Decision::Accept).unwrap();
                }
                Event::Received {
                    name, path, size, ..
                } => got.push((name, path, size)),
                Event::Finished { .. } => return (pin, got),
                other => panic!("{other:?}"),
            }
        }
        (pin, got)
    });
    let t0 = Instant::now();
    let sent = client::send(rx.addr(), "Telefon", &[a, b]).await.unwrap();
    let took = t0.elapsed();
    let (pin, got) = user.await.unwrap();
    assert_eq!(sent.pin, pin, "both sides show the same code");
    assert_eq!(sent.files, 2);
    assert_eq!(got.len(), 2);
    let video = got.iter().find(|g| g.0 == "VID_2026.mp4").unwrap();
    assert_eq!(std::fs::read(&video.1).unwrap(), big);
    assert_eq!(got.iter().find(|g| g.0 == "empty.txt").unwrap().2, 0);
    assert!(no_parts(&dir.join("staging")));
    println!(
        "3.2 MB over loopback Quick Share (UKEY2 + AES-CBC + HMAC) in {took:?} ({:.0} MB/s, {})",
        3.2 / took.as_secs_f64(),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn reject() {
    let dir = tmp("reject");
    let (rx, mut events) = receiver(&dir).await;
    let f = dir.join("x.bin");
    std::fs::write(&f, b"x").unwrap();
    tokio::spawn(async move {
        while let Some(ev) = events.recv().await {
            if let Event::Offer { reply, .. } = ev {
                let _ = reply.send(Decision::Reject);
            }
        }
    });
    let e = client::send(rx.addr(), "Telefon", &[f]).await.unwrap_err();
    assert_eq!(
        e.downcast_ref::<client::Refused>(),
        Some(&client::Refused(msg::REJECT))
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A sender that gets an accepted offer for one 10-byte file and then misbehaves in the given way.
async fn hostile(addr: SocketAddr, attack: u8) {
    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let (r, mut w) = tcp.into_split();
    let mut rx = conn::reader(r);
    conn::write_frame(
        &mut w,
        &msg::connection_request("EVIL", &endpoint::info(Device::Phone, "x")),
    )
    .await
    .unwrap();
    let (hs, init) = ukey2::Client::start();
    conn::write_frame(&mut w, &init).await.unwrap();
    let (keys, fin) = hs.finish(&conn::next(&mut rx).await.unwrap()).unwrap();
    conn::write_frame(&mut w, &fin).await.unwrap();
    conn::write_frame(&mut w, &msg::connection_response(true))
        .await
        .unwrap();
    let _ = conn::next(&mut rx).await.unwrap();
    let mut ch = Channel::new(&keys, false);
    let mut bytes = Bytes::default();
    conn::send_share(
        &mut w,
        &mut ch,
        &msg::paired_key_encryption(&[0; 6], &[0; 72]),
    )
    .await
    .unwrap();
    conn::send_share(&mut w, &mut ch, &msg::paired_key_result())
        .await
        .unwrap();
    let file = msg::FileMeta {
        name: "a.bin".into(),
        payload_id: 42,
        size: 10,
        mime: String::new(),
    };
    conn::send_share(&mut w, &mut ch, &msg::introduction(&[file]))
        .await
        .unwrap();
    // Wait for the accept.
    loop {
        let plain = ch.open(&conn::next(&mut rx).await.unwrap()).unwrap();
        let f = msg::parse_offline(&plain).unwrap();
        if let Piece::Message(_, b) = bytes.feed(&f)
            && msg::parse_share(&b) == Some(Share::Response(msg::ACCEPT))
        {
            break;
        }
    }
    let h = |id, total| PayloadHeader {
        id,
        kind: 2,
        total_size: total,
    };
    let frames: Vec<Vec<u8>> = match attack {
        // A payload id that was never offered.
        0 => vec![
            msg::payload(&h(42, 10), 0, b"12345", false),
            msg::payload(&h(99, 10), 0, b"x", true),
        ],
        // A gap in the offsets.
        1 => vec![
            msg::payload(&h(42, 10), 0, b"12345", false),
            msg::payload(&h(42, 10), 7, b"x", true),
        ],
        // More bytes than announced.
        2 => vec![msg::payload(&h(42, 10), 0, b"12345678901", true)],
        // Leaving half way.
        _ => vec![
            msg::payload(&h(42, 10), 0, b"12345", false),
            msg::disconnection(),
        ],
    };
    for f in frames {
        let _ = conn::send(&mut w, &mut ch, &f).await;
    }
    // A forged frame outside the channel must not be accepted either.
    let _ = conn::write_frame(&mut w, &msg::payload(&h(42, 10), 5, b"67890", true)).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), conn::next(&mut rx)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rule_breaking_sender_leaves_nothing() {
    let dir = tmp("hostile");
    let (rx, mut events) = receiver(&dir).await;
    for attack in 0..4u8 {
        let send = tokio::spawn(hostile(rx.addr(), attack));
        let mut cancelled = false;
        while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(10), events.recv()).await
        {
            match ev {
                Event::Offer { reply, .. } => reply.send(Decision::Accept).unwrap(),
                Event::Cancelled { .. } => {
                    cancelled = true;
                    break;
                }
                other => panic!("attack {attack}: {other:?}"),
            }
        }
        send.await.unwrap();
        assert!(cancelled, "attack {attack} ends the transfer");
        assert!(
            no_parts(&dir.join("staging")),
            "attack {attack} left a part file"
        );
        assert_eq!(std::fs::read_dir(dir.join("staging")).unwrap().count(), 0);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[ignore = "needs working multicast on the machine; run with --ignored"]
fn advert_is_visible_over_mdns() {
    use mdns_sd::{ServiceDaemon, ServiceEvent};
    let info = endpoint::info(Device::Laptop, "Ota kompyuteri");
    let advert = vutric_compat_quickshare::mdns::Advert::start("AB12", &info, 45678).unwrap();
    let browser = ServiceDaemon::new().unwrap();
    let rx = browser.browse(endpoint::SERVICE_TYPE).unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if let Ok(ServiceEvent::ServiceResolved(s)) = rx.recv_timeout(Duration::from_millis(500))
            && s.get_fullname() == advert.fullname()
        {
            assert_eq!(s.get_port(), 45678);
            let n = s.get_property_val_str("n").unwrap();
            let raw = data_encoding::BASE64URL_NOPAD.decode(n.as_bytes()).unwrap();
            assert_eq!(
                endpoint::parse(&raw).unwrap().name.as_deref(),
                Some("Ota kompyuteri")
            );
            let _ = browser.shutdown();
            return;
        }
    }
    panic!("advert not seen");
}
