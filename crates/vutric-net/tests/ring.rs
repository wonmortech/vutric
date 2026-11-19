//! "Ring my phone" end to end (ADR-0069, backlog F14): the desktop rings, the phone side sees the ring, applies
//! the rules of `vutric_core::ring`, and its "found it" stop comes back. A ring during a transfer is kept.

use std::time::{Duration, Instant};

use vutric_core::ring::{self, MAX_RING_SECS, Ring, RingLimit};
use vutric_core::{Message, Platform};
use vutric_net::{Admission, Peer, Reach};

async fn pair_of(a: u8, b: u8) -> (Peer, Peer) {
    let phone = Peer::bind([a; 32], "Pixel", Platform::Android, Reach::Lan)
        .await
        .expect("bind phone");
    let desktop = Peer::bind([b; 32], "Ofis", Platform::Windows, Reach::Lan)
        .await
        .expect("bind desktop");
    (phone, desktop)
}

async fn next(s: &mut vutric_net::Session) -> Message {
    tokio::time::timeout(Duration::from_secs(5), s.recv())
        .await
        .expect("in time")
        .expect("message")
}

#[tokio::test(flavor = "multi_thread")]
async fn ring_reaches_a_trusting_phone_and_found_comes_back() {
    let (phone, desktop) = pair_of(51, 52).await;
    let listener = phone.clone();
    let task = tokio::spawn(async move {
        let (mut s, admission) = listener
            .accept_with(|_| Admission::Trusted)
            .await
            .expect("accept");
        let mut limit = RingLimit::default();
        let mut rang = Vec::new();
        for _ in 0..5 {
            let msg = next(&mut s).await;
            assert!(ring::accepts(&msg, admission));
            let Message::Ring(r) = msg else {
                panic!("expected a ring, got {}", msg.kind())
            };
            rang.push((r, limit.allow(r, Instant::now())));
        }
        s.stop_ring().await.expect("found it");
        (s, rang)
    });
    let t = phone.ticket();
    let mut s = desktop
        .connect(t.endpoint, &t.addrs)
        .await
        .expect("connect");
    for _ in 0..4 {
        s.ring(500).await.expect("ring");
    }
    s.stop_ring().await.expect("stop");
    let (ps, rang) = task.await.unwrap();
    let max = Ring::Start {
        secs: MAX_RING_SECS,
    };
    assert_eq!(
        rang,
        vec![
            (max, true),
            (max, true),
            (max, true),
            (max, false),
            (Ring::Stop, true)
        ],
        "clamped to the cap, a burst of three, a stop always passes"
    );
    assert_eq!(next(&mut s).await, Message::Ring(Ring::Stop));
    s.close();
    ps.close();
    desktop.close().await;
    phone.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_cannot_ring() {
    let (phone, desktop) = pair_of(53, 54).await;
    let listener = phone.clone();
    let task = tokio::spawn(async move {
        let (mut s, admission) = listener
            .accept_with(|_| Admission::Unknown)
            .await
            .expect("accept");
        let msg = next(&mut s).await;
        (s, ring::accepts(&msg, admission))
    });
    let t = phone.ticket();
    let mut s = desktop
        .connect(t.endpoint, &t.addrs)
        .await
        .expect("connect");
    s.ring(10).await.expect("ring is sent");
    let (ps, taken) = task.await.unwrap();
    assert!(!taken, "a ring from an unpaired device is dropped");
    s.close();
    ps.close();
    desktop.close().await;
    phone.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ring_during_a_transfer_is_kept_for_the_host() {
    use std::sync::Arc;
    use vutric_net::transfer::{self, IncomingOffer, Outgoing, SaveTo};
    let dir = std::env::temp_dir().join(format!("vutric-ring-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("out")).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, b"hello").unwrap();

    let (phone, desktop) = pair_of(55, 56).await;
    let listener = phone.clone();
    let out = dir.join("out");
    let rx = tokio::spawn(async move {
        let (mut s, _) = listener
            .accept_with(|_| Admission::Trusted)
            .await
            .expect("accept");
        let offer = IncomingOffer::from_message(&s.recv().await.unwrap()).expect("offer");
        s.ring(5).await.unwrap();
        transfer::receive(&mut s, offer, &SaveTo::folder(out), Arc::new(|_| {}))
            .await
            .expect("receive");
        s
    });
    let t = phone.ticket();
    let mut s = desktop
        .connect(t.endpoint, &t.addrs)
        .await
        .expect("connect");
    transfer::send(
        &mut s,
        vec![Outgoing::file(&file, None).unwrap()],
        Arc::new(|_| {}),
    )
    .await
    .expect("send");
    assert_eq!(next(&mut s).await, Message::Ring(Ring::Start { secs: 5 }));
    let ps = rx.await.unwrap();
    s.close();
    ps.close();
    desktop.close().await;
    phone.close().await;
    let _ = std::fs::remove_dir_all(&dir);
}
