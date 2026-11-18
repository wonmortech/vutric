//! C4 on loopback: our own KDE Connect client plays the phone against the receiver, in both connection roles.
//! The real-app test (KDE Connect for Android and Linux) is backlog M5.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::sync::mpsc;
use vutric_compat_kdeconnect::link::{Link, Me};
use vutric_compat_kdeconnect::proto::{DeviceType, Identity, Share};
use vutric_compat_kdeconnect::receiver::{Config, Event, Receiver};
use vutric_compat_kdeconnect::tls;
use vutric_compat_kdeconnect::trust::Trust;

const LO: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

fn me(name: &str, kind: DeviceType) -> Me {
    Me {
        tls: tls::Identity::generate().unwrap(),
        name: name.into(),
        kind,
    }
}

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("vutric-kdc-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

async fn receiver(tag: &str, udp: bool) -> (Receiver, mpsc::Receiver<Event>, Me, PathBuf) {
    let staging = temp(tag);
    let rx_me = me("Ish PC", DeviceType::Desktop);
    let (rx, events) = Receiver::start(Config {
        me: rx_me.clone(),
        bind: LO,
        tcp_ports: 0..=0,
        udp_port: udp.then_some(0),
        staging: staging.clone(),
        trust: Trust::memory(),
        decide_timeout: Duration::from_secs(5),
        payload_ports: 0..=0,
    })
    .await
    .unwrap();
    (rx, events, rx_me, staging)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

async fn event(events: &mut mpsc::Receiver<Event>, ms: u64) -> Option<Event> {
    tokio::time::timeout(Duration::from_millis(ms), events.recv())
        .await
        .ok()
        .flatten()
}

/// The phone asks to pair; the user answers `yes` after checking the key both screens show.
async fn pair(
    mut link: Link,
    phone: &Me,
    rx_me: &Me,
    events: &mut mpsc::Receiver<Event>,
    yes: bool,
) -> (bool, Link) {
    let ts = now();
    let want = tls::verification_key(phone.tls.cert(), rx_me.tls.cert(), ts).unwrap();
    let asking = tokio::spawn(async move {
        let r = link.request_pair(ts, Duration::from_secs(10)).await;
        (r, link)
    });
    let mut ev = event(events, 5000).await;
    if matches!(ev, Some(Event::Seen { .. })) {
        ev = event(events, 5000).await;
    }
    let Some(Event::PairRequest {
        name, key, answer, ..
    }) = ev
    else {
        panic!("no pairing request")
    };
    assert_eq!(name, "Pixel 8");
    assert_eq!(key, want, "both screens show the same key");
    answer.send(yes).unwrap();
    let (r, link) = asking.await.unwrap();
    let answered = r.unwrap();
    if yes {
        assert!(matches!(
            event(events, 5000).await,
            Some(Event::Paired { .. })
        ));
    }
    (answered, link)
}

#[tokio::test(flavor = "multi_thread")]
async fn pair_then_receive_files_and_text() {
    let (rx, mut events, rx_me, staging) = receiver("files", false).await;
    let phone = me("Pixel 8", DeviceType::Phone);
    let link = Link::dial(&phone, rx.addr(), &rx_me.tls.device_id)
        .await
        .unwrap();
    assert_eq!(link.peer.device_name, "Ish PC");
    let (yes, mut link) = pair(link, &phone, &rx_me, &mut events, true).await;
    assert!(yes);

    let src = temp("files-src");
    let file = src.join("rasm 1.jpg");
    let bytes: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&file, &bytes).unwrap();
    let sender = {
        let phone = phone.clone();
        let files = vec![file.clone(), file.clone()];
        tokio::spawn(async move {
            link.send_files(&phone, &files, LO, 0..=0).await.unwrap();
            link.send(&Share::text("salom")).await.unwrap();
            link
        })
    };
    let mut names = Vec::new();
    for _ in 0..2 {
        let Some(Event::File {
            path, size, name, ..
        }) = event(&mut events, 10_000).await
        else {
            panic!("file expected")
        };
        assert_eq!(name, "Pixel 8");
        assert_eq!(size, bytes.len() as u64);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(path.parent().unwrap(), staging);
        names.push(path.file_name().unwrap().to_string_lossy().into_owned());
    }
    assert_eq!(names, ["rasm 1.jpg", "rasm 1 (2).jpg"]);
    let Some(Event::Text { text, .. }) = event(&mut events, 5000).await else {
        panic!("text expected")
    };
    assert_eq!(text, "salom");
    drop(sender.await.unwrap());
    let leftovers = std::fs::read_dir(&staging)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|x| x == "vutric-part")
        })
        .count();
    assert_eq!(leftovers, 0);
    let _ = std::fs::remove_dir_all(staging);
    let _ = std::fs::remove_dir_all(src);
}

#[tokio::test(flavor = "multi_thread")]
async fn unpaired_is_ignored_and_no_means_no() {
    let (rx, mut events, rx_me, staging) = receiver("unpaired", false).await;
    let phone = me("Pixel 8", DeviceType::Phone);
    let mut link = Link::dial(&phone, rx.addr(), &rx_me.tls.device_id)
        .await
        .unwrap();
    link.send(&Share::text("not yet")).await.unwrap();
    assert!(
        event(&mut events, 500).await.is_none(),
        "unpaired share must be dropped"
    );
    let (yes, mut link) = pair(link, &phone, &rx_me, &mut events, false).await;
    assert!(!yes);
    link.send(&Share::text("still no")).await.unwrap();
    assert!(event(&mut events, 500).await.is_none());
    let _ = std::fs::remove_dir_all(staging);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_certificate_is_dropped() {
    let (rx, mut events, rx_me, staging) = receiver("pinning", false).await;
    let phone = me("Pixel 8", DeviceType::Phone);
    let link = Link::dial(&phone, rx.addr(), &rx_me.tls.device_id)
        .await
        .unwrap();
    let (_, link) = pair(link, &phone, &rx_me, &mut events, true).await;
    drop(link);
    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut impostor = me("Pixel 8", DeviceType::Phone);
    impostor.tls.device_id.clone_from(&phone.tls.device_id);
    let mut link = Link::dial(&impostor, rx.addr(), &rx_me.tls.device_id)
        .await
        .unwrap();
    let _ = link.send(&Share::text("let me in")).await;
    let closed = tokio::time::timeout(Duration::from_secs(5), link.next())
        .await
        .unwrap();
    assert!(
        matches!(closed, Ok(None) | Err(_)),
        "impostor connection must close"
    );
    assert!(event(&mut events, 300).await.is_none());
    let _ = std::fs::remove_dir_all(staging);
}

#[tokio::test(flavor = "multi_thread")]
async fn first_line_must_name_us_and_stay_small() {
    let (rx, _events, _rx_me, staging) = receiver("first", false).await;
    let stranger = tls::Identity::generate().unwrap();
    for bytes in [
        Identity::first(&stranger.device_id, &stranger.device_id),
        vec![b'x'; 5000],
    ] {
        let mut tcp = tokio::net::TcpStream::connect(rx.addr()).await.unwrap();
        let _ = tcp.write_all(&bytes).await;
        let mut buf = [0u8; 16];
        let n = tokio::time::timeout(Duration::from_secs(5), tcp.read(&mut buf))
            .await
            .unwrap();
        assert!(matches!(n, Ok(0) | Err(_)), "connection must close");
    }
    let _ = std::fs::remove_dir_all(staging);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_udp_identity_makes_the_receiver_dial() {
    let (rx, mut events, rx_me, staging) = receiver("udp", true).await;
    let phone = me("Pixel 8", DeviceType::Phone);
    let listener = tokio::net::TcpListener::bind((LO, 0)).await.unwrap();
    let udp = tokio::net::UdpSocket::bind((LO, 0)).await.unwrap();
    let hello = Identity::udp(
        &phone.tls.device_id,
        &phone.name,
        phone.kind,
        listener.local_addr().unwrap().port(),
    );
    udp.send_to(&hello, SocketAddr::new(LO, rx.udp_port()))
        .await
        .unwrap();
    let (tcp, from) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let link = Link::answer(&phone, tcp, from).await.unwrap();
    assert_eq!(link.peer.device_id, rx_me.tls.device_id);
    let (yes, _link) = pair(link, &phone, &rx_me, &mut events, true).await;
    assert!(yes, "pairing works when the receiver dialled");
    assert_eq!(rx.paired().len(), 1);

    // We send to the paired phone: it listens, we dial it, the file arrives on its side.
    let src = temp("udp-src");
    let file = src.join("hujjat.pdf");
    std::fs::write(&file, b"%PDF-1.7 test").unwrap();
    let phone_listener = tokio::net::TcpListener::bind((LO, 0)).await.unwrap();
    let phone_addr = phone_listener.local_addr().unwrap();
    let phone2 = phone.clone();
    let phone_side = tokio::spawn(async move {
        let (tcp, from) = phone_listener.accept().await.unwrap();
        let mut l = Link::answer(&phone2, tcp, from).await.unwrap();
        let p = l.next().await.unwrap().unwrap();
        assert_eq!(p.kind, "kdeconnect.share.request.update");
        let p = l.next().await.unwrap().unwrap();
        let Some(Share::File {
            name, size, port, ..
        }) = Share::from_packet(&p)
        else {
            panic!("file packet expected")
        };
        let dest = std::env::temp_dir().join(format!("vutric-kdc-phone-{}", std::process::id()));
        vutric_compat_kdeconnect::payload::fetch(
            &phone2,
            from.ip(),
            port,
            &l.peer_cert,
            size,
            &dest,
        )
        .await
        .unwrap();
        let got = std::fs::read(&dest).unwrap();
        let _ = std::fs::remove_file(dest);
        (name, got)
    });
    let sent = rx
        .send(phone_addr, &phone.tls.device_id, &[file])
        .await
        .unwrap();
    assert_eq!(sent, 13);
    let (name, got) = phone_side.await.unwrap();
    assert_eq!(name, "hujjat.pdf");
    assert_eq!(got, b"%PDF-1.7 test");

    // A device that never paired is refused before anything is offered.
    let stranger = me("Begona", DeviceType::Phone);
    let l2 = tokio::net::TcpListener::bind((LO, 0)).await.unwrap();
    let a2 = l2.local_addr().unwrap();
    let s2 = stranger.clone();
    tokio::spawn(async move {
        let (tcp, from) = l2.accept().await.unwrap();
        let _ = Link::answer(&s2, tcp, from).await;
    });
    let refused = rx
        .send(a2, &stranger.tls.device_id, &[src.join("hujjat.pdf")])
        .await;
    assert!(refused.is_err(), "not paired");
    let _ = std::fs::remove_dir_all(src);
    let _ = std::fs::remove_dir_all(staging);
}
