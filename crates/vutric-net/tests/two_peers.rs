//! Two peers in one process, over the LAN addresses the ticket advertises (no relay, no internet).
//! Both directions are exercised: whoever connects can also receive, as ADR-0005 requires.

use std::time::{Duration, Instant};

use vutric_core::{
    EffectId, FlingId, FlingStart, Gesture, Message, Platform, Preview, PreviewFormat,
};
use vutric_net::{Peer, Session};

fn fling(n: u8) -> Message {
    Message::FlingStart(FlingStart {
        id: FlingId([n; 16]),
        file_name: format!("IMG_{n:04}.jpg"),
        mime: "image/jpeg".into(),
        size: 4_200_000,
        blake3: [n; 32],
        gesture: Gesture {
            speed: 1800.0,
            angle: 0.1,
            exit_x: 0.6,
        },
        effect: EffectId::Halqa,
        preview: Some(Preview {
            format: PreviewFormat::Jpeg,
            width: 512,
            height: 384,
            bytes: vec![n; 30 * 1024],
        }),
    })
}

/// Receiver side of one probe: answer every `FlingStart` with `EffectStarted`.
async fn answer(session: &mut Session, start: Instant, count: usize) {
    for _ in 0..count {
        match session.recv().await.expect("recv") {
            Message::FlingStart(f) => {
                let preview = f.preview.expect("preview");
                assert_eq!(preview.bytes.len(), 30 * 1024);
                session
                    .send(&Message::EffectStarted {
                        id: f.id,
                        receiver_micros: vutric_net::micros_since(start),
                    })
                    .await
                    .expect("send");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

/// Sender side: one fling, wait for the effect; returns the round trip.
async fn probe(session: &mut Session, n: u8) -> Duration {
    let t0 = Instant::now();
    session.send(&fling(n)).await.expect("send");
    match session.recv().await.expect("recv") {
        Message::EffectStarted { id, .. } => assert_eq!(id, FlingId([n; 16])),
        other => panic!("unexpected {other:?}"),
    }
    t0.elapsed()
}

#[tokio::test(flavor = "multi_thread")]
async fn both_directions_and_matching_check_code() {
    let desktop = Peer::bind([1; 32], "Studio PC", Platform::Windows)
        .await
        .expect("bind desktop");
    let phone = Peer::bind([2; 32], "Pixel 8", Platform::Android)
        .await
        .expect("bind phone");

    let ticket = desktop.ticket();
    assert!(!ticket.addrs.is_empty(), "ticket carries LAN address hints");
    let parsed = vutric_core::PairingTicket::parse(&ticket.to_uri()).expect("ticket round-trips");

    let start = Instant::now();
    let accept = tokio::spawn({
        let desktop = desktop.clone();
        async move {
            let mut s = desktop.accept().await.expect("accept");
            assert_eq!(s.remote_name, "Pixel 8");
            assert_eq!(s.remote_platform, Platform::Android);
            let code = s.check_code().expect("code");
            answer(&mut s, start, 3).await; // phone -> desktop
            for n in 10..13 {
                probe(&mut s, n).await; // desktop -> phone, same session
            }
            (code, s)
        }
    });

    let mut s = phone
        .connect(parsed.endpoint, &parsed.addrs)
        .await
        .expect("connect");
    assert_eq!(s.remote_name, "Studio PC");
    let phone_code = s.check_code().expect("code");
    for n in 0..3 {
        let rtt = probe(&mut s, n).await;
        assert!(rtt < Duration::from_secs(2), "{rtt:?}");
    }
    answer(&mut s, start, 3).await;

    let (desktop_code, _keep) = tokio::time::timeout(Duration::from_secs(10), accept)
        .await
        .expect("desktop finished")
        .expect("desktop task");
    assert_eq!(
        phone_code, desktop_code,
        "both screens show the same pairing check"
    );
    assert_eq!(phone_code.len(), 6);

    s.close();
    phone.close().await;
    desktop.close().await;
}
