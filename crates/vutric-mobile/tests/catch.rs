//! The phone API against a real second peer in this process: the "phone" is the blocking API the Flutter app
//! calls (one global peer and session, so the scenarios run in order inside one test), the other device is a
//! plain `vutric-net` peer.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use vutric_core::message::FailReason;
use vutric_core::{
    FlingId, FlingStart, Gesture, Message, PairingTicket, Platform, Preview, PreviewFormat,
};
use vutric_mobile::api;
use vutric_net::{Peer, Session, probe};

fn jpeg(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, 120])
    });
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80)
        .encode_image(&img)
        .expect("encode");
    out
}

fn gesture() -> Gesture {
    Gesture {
        speed: 2400.0,
        angle: 0.2,
        exit_x: 0.7,
    }
}

async fn thrower(link: &str) -> (Peer, Session) {
    let ticket = PairingTicket::parse(link).expect("ticket");
    let peer = Peer::bind(
        vutric_net::new_identity(),
        "Thrower",
        Platform::Windows,
        vutric_net::Reach::Lan,
    )
    .await
    .expect("bind");
    let s = peer
        .connect(ticket.endpoint, &ticket.addrs)
        .await
        .expect("connect");
    (peer, s)
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

#[test]
#[allow(clippy::too_many_lines)] // one scenario after another on the one global peer
fn phone_api_end_to_end() {
    let dir = std::env::temp_dir().join(format!("vutric-catch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    api::set_settings_dir(dir.to_string_lossy().into_owned());
    let mut settings: serde_json::Value =
        serde_json::from_str(&api::get_settings().expect("settings")).expect("json");
    settings["route"] = "lan_only".into();
    api::put_settings(settings.to_string()).expect("lan only");
    let link = api::start("Test phone".into(), api::new_identity()).expect("start");
    // The QR is "on screen": unknown devices may connect to pair (ADR-0050).
    assert_eq!(
        api::set_visibility("pairing".into()).expect("pairing").mode,
        "pairing"
    );
    let code = api::my_code().expect("code");
    assert_eq!((code.len(), code.as_bytes()[4]), (9, b'-'));

    // 1. disconnect() cancels a phone that waits to be found, at once, and a later session still works.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(api::accept_session());
    });
    std::thread::sleep(Duration::from_millis(300));
    let t0 = Instant::now();
    api::disconnect();
    let err = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("accept_session must return after disconnect")
        .expect_err("cancelled");
    assert_eq!(err.to_string(), api::CANCELLED);
    assert!(
        t0.elapsed() < Duration::from_secs(1),
        "cancel took {:?}",
        t0.elapsed()
    );

    // 2. The phone catches a photo, skips a probe, refuses a fake preview, and its last answer survives an
    //    immediate disconnect().
    let (tx, rx) = mpsc::channel();
    let phone = std::thread::spawn(move || {
        let c = api::accept_session().expect("accept");
        assert_eq!(c.peer_name, "Thrower");
        let caught = api::next_incoming()
            .expect("next incoming")
            .photo
            .expect("a photo");
        api::shown(caught.id.clone()).expect("shown");
        api::disconnect();
        tx.send(caught).expect("send");
    });
    rt().block_on(async {
        let (peer, mut s) = thrower(&link).await;
        probe::send_one(&mut s, 1).await.expect("probe answered");
        let fake = Message::FlingStart(FlingStart {
            id: FlingId([9; 16]),
            file_name: "fake.jpg".into(),
            mime: "image/jpeg".into(),
            size: 10,
            blake3: [1; 32],
            gesture: gesture(),
            effect: vutric_core::EffectId::Halqa,
            preview: Some(Preview {
                format: PreviewFormat::Jpeg,
                width: 60_000,
                height: 60_000,
                bytes: vec![0xff, 0xd8, 0xff, 0xe0, 1, 2, 3],
            }),
        });
        let refused = probe::round_trip(&mut s, &fake).await.expect_err("refused");
        assert_eq!(
            refused.downcast_ref::<probe::Refused>(),
            Some(&probe::Refused(FailReason::Rejected)),
            "{refused:#}"
        );
        let msg = probe::photo("cat.jpg", jpeg(64, 48), 64, 48, gesture());
        probe::round_trip(&mut s, &msg)
            .await
            .expect("shown() closes the round trip, even with disconnect() right after it");
        s.close();
        peer.close().await;
    });
    let caught = rx.recv_timeout(Duration::from_secs(10)).expect("caught");
    phone.join().expect("phone thread");
    assert_eq!(caught.file_name, "cat.jpg");
    assert_eq!((caught.width, caught.height), (64, 48));
    assert!((caught.exit_x - 0.7).abs() < 1e-6);

    // 3. A throw at a device that never answers fails in bounded time and closes the session.
    let (tx, rx) = mpsc::channel();
    let silent = std::thread::spawn(move || {
        rt().block_on(async {
            let peer = Peer::bind(
                vutric_net::new_identity(),
                "Silent",
                Platform::Windows,
                vutric_net::Reach::Lan,
            )
            .await
            .expect("bind");
            tx.send(peer.ticket().to_uri()).expect("link");
            let mut s = peer.accept().await.expect("accept");
            // Read everything, answer nothing, keep the connection up well past the thrower's timeout.
            let _ = tokio::time::timeout(Duration::from_secs(8), async {
                while s.recv().await.is_ok() {}
            })
            .await;
            peer.close().await;
        });
    });
    let silent_link = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("silent link");
    api::connect_link(silent_link).expect("connect to silent");
    let photo = api::prepare_photo("dog.png".into(), jpeg(900, 600)).expect("prepare");
    assert_eq!((photo.width, photo.height), (512, 341));
    let t0 = Instant::now();
    let err = api::fling(photo.clone(), 2000.0, 0.0, 0.5).expect_err("no answer");
    let waited = t0.elapsed();
    assert!(err.to_string().contains("no answer"), "{err:#}");
    assert!(
        waited >= probe::ROUND_TRIP_TIMEOUT
            && waited < probe::ROUND_TRIP_TIMEOUT + Duration::from_secs(2),
        "{waited:?}"
    );
    let after = api::fling(photo, 2000.0, 0.0, 0.5).expect_err("session gone");
    assert!(after.to_string().contains("not connected"), "{after:#}");
    silent.join().expect("silent thread");

    // 4. Files offered to the phone show up as an offer, and a "no" reaches the sender as Declined.
    let file = dir.join("report.pdf");
    std::fs::write(&file, b"%PDF-1.7 hello").expect("write");
    let (tx, rx) = mpsc::channel();
    let phone = std::thread::spawn(move || {
        api::accept_session().expect("accept");
        let offer = api::next_incoming()
            .expect("next incoming")
            .files
            .expect("a file offer");
        api::decline_files().expect("decline");
        tx.send(offer).expect("send");
    });
    rt().block_on(async {
        let (peer, mut s) = thrower(&link).await;
        let one = vec![vutric_net::transfer::Outgoing::file(&file, None).expect("file")];
        let err = vutric_net::transfer::send(&mut s, one, std::sync::Arc::new(|_| {}))
            .await
            .expect_err("declined");
        assert!(
            err.downcast_ref::<vutric_net::transfer::Declined>()
                .is_some(),
            "{err:#}"
        );
        s.close();
        peer.close().await;
    });
    let offer = rx.recv_timeout(Duration::from_secs(10)).expect("offer");
    phone.join().expect("phone thread");
    assert_eq!((offer.files, offer.bytes), (1, 14));
    assert_eq!(offer.names, vec!["report.pdf".to_owned()]);

    // 5. Everyone who connected through the pairing window, and the device whose link we dialed, is paired;
    //    with the window closed a stranger is refused before its first message (ADR-0050).
    let devices = api::devices();
    assert!(
        devices
            .iter()
            .any(|d| d.name == "Thrower" && d.method == "ticket"),
        "{devices:?}"
    );
    assert!(devices.iter().any(|d| d.name == "Silent"));
    assert_eq!(
        api::set_visibility("trusted_only".into())
            .expect("closed")
            .mode,
        "trusted_only"
    );
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(api::accept_session());
    });
    rt().block_on(async {
        let ticket = PairingTicket::parse(&link).expect("ticket");
        let stranger = Peer::bind(
            vutric_net::new_identity(),
            "Stranger",
            Platform::Windows,
            vutric_net::Reach::Lan,
        )
        .await
        .expect("bind");
        let err = stranger
            .connect(ticket.endpoint, &ticket.addrs)
            .await
            .expect_err("refused");
        assert!(
            err.downcast_ref::<vutric_net::NotAdmitted>().is_some(),
            "{err:#}"
        );
        stranger.close().await;
    });
    api::disconnect();
    let _ = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("accept returns");

    // 6. Per-device overrides (T3): set, read back, all-null removes them, a bad id is refused.
    let thrower = devices
        .iter()
        .find(|d| d.name == "Thrower")
        .expect("thrower");
    api::put_device_rules(
        thrower.id.clone(),
        r#"{"accept":{"mode":"ask"},"gallery":false}"#.into(),
    )
    .expect("rules");
    let back: serde_json::Value =
        serde_json::from_str(&api::device_rules(thrower.id.clone()).expect("read")).expect("json");
    assert_eq!(back["gallery"], false);
    assert_eq!(back["accept"]["mode"], "ask");
    assert!(
        api::devices()
            .iter()
            .any(|d| d.id == thrower.id && d.has_rules)
    );
    api::put_device_rules(thrower.id.clone(), "{}".into()).expect("clear");
    assert!(
        !api::devices()
            .iter()
            .any(|d| d.id == thrower.id && d.has_rules)
    );
    assert!(api::put_device_rules("abc".into(), "{}".into()).is_err());
    assert!(api::forget_device(thrower.id.clone()).expect("forget"));
    assert!(!api::devices().iter().any(|d| d.id == thrower.id));
}
