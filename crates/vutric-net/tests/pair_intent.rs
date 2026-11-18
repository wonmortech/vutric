//! Issue #13 / ADR-0064 end to end: the listener learns how the dialer pairs. A dialer that scanned the QR proves
//! the link secret; one that typed a short code starts the six-digit check; one that does neither stays a guest,
//! and its first real message is not lost. Companion messages (#12) arrive, and are kept across a transfer.

use std::time::Duration;

use vutric_core::sas::LinkSecret;
use vutric_core::{Message, Platform, pairing};
use vutric_net::{Admission, PAIR_INTENT_WAIT, PairIntent, Peer, Reach, Session};

async fn pair_of(a: u8, b: u8) -> (Peer, Peer) {
    let listener = Peer::bind([a; 32], "Pixel A", Platform::Android, Reach::Lan)
        .await
        .expect("bind listener");
    let dialer = Peer::bind([b; 32], "Pixel B", Platform::Android, Reach::Lan)
        .await
        .expect("bind dialer");
    (listener, dialer)
}

/// The listener's side: admit as Unknown (pairing window), then ask how the dialer pairs.
fn listen(
    peer: &Peer,
    secret: Option<LinkSecret>,
) -> tokio::task::JoinHandle<(Session, PairIntent)> {
    let peer = peer.clone();
    tokio::spawn(async move {
        let (mut s, admission) = peer
            .accept_with(|_| Admission::Unknown)
            .await
            .expect("accept");
        assert_eq!(admission, Admission::Unknown);
        let intent = s
            .pair_intent(secret.as_ref(), PAIR_INTENT_WAIT)
            .await
            .expect("intent");
        (s, intent)
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn qr_dialer_proves_the_link() {
    let (listener, dialer) = pair_of(31, 32).await;
    let secret = LinkSecret([9; 16]);
    let link = pairing::with_secret(&listener.ticket().to_uri(), &secret);
    let task = listen(&listener, Some(secret));
    let s = dialer.connect_link(&link).await.expect("connect by link");
    let (ls, intent) = task.await.unwrap();
    assert_eq!(intent, PairIntent::Link);
    s.close();
    ls.close();
    dialer.close().await;
    listener.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_old_qr_is_not_trusted() {
    let (listener, dialer) = pair_of(33, 34).await;
    let old = pairing::with_secret(&listener.ticket().to_uri(), &LinkSecret([1; 16]));
    let task = listen(&listener, Some(LinkSecret([2; 16])));
    let s = dialer.connect_link(&old).await.expect("connect");
    let (ls, intent) = task.await.unwrap();
    assert_eq!(intent, PairIntent::WrongLink);
    s.close();
    ls.close();
    dialer.close().await;
    listener.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn code_dialer_runs_the_check_and_the_listener_follows() {
    let (listener, dialer) = pair_of(35, 36).await;
    let task = listen(&listener, Some(LinkSecret([3; 16])));
    let t = listener.ticket();
    let mut s = dialer.connect(t.endpoint, &t.addrs).await.expect("connect");
    let dialer_side = tokio::spawn(async move {
        let code = s.pair_code().await.expect("dialer code");
        let both = s.pair_finish(true).await.expect("dialer finish");
        (code, both, s)
    });
    let (mut ls, intent) = task.await.unwrap();
    assert_eq!(intent, PairIntent::Code);
    let code = ls.pair_code().await.expect("listener code");
    assert!(ls.pair_finish(true).await.expect("listener finish"));
    let (dcode, both, s) = dialer_side.await.unwrap();
    assert!(both);
    assert_eq!(code, dcode, "same six digits on both screens");
    s.close();
    ls.close();
    dialer.close().await;
    listener.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn neither_keeps_the_first_message_and_companion_messages_flow() {
    let (listener, dialer) = pair_of(37, 38).await;
    let task = listen(&listener, None);
    let t = listener.ticket();
    let mut s = dialer.connect(t.endpoint, &t.addrs).await.expect("connect");
    s.send_notice("Telegram", "Ali", &"x".repeat(1000), 42)
        .await
        .expect("notice is cut to the caps, not refused");
    s.send_clipboard("salom").await.expect("clipboard");
    assert!(s.send_clipboard("").await.is_err());
    let (mut ls, intent) = task.await.unwrap();
    assert_eq!(intent, PairIntent::Neither);
    match ls.recv().await.expect("first message kept") {
        Message::Notice(n) => {
            assert_eq!(
                (n.app.as_str(), n.title.as_str(), n.at_ms),
                ("Telegram", "Ali", 42)
            );
            assert_eq!(
                n.text.chars().count(),
                vutric_core::message::MAX_NOTICE_TEXT_CHARS
            );
        }
        other => panic!("expected the notice, got {}", other.kind()),
    }
    assert_eq!(
        ls.recv().await.expect("clipboard"),
        Message::Clipboard {
            text: "salom".into()
        }
    );
    s.close();
    ls.close();
    dialer.close().await;
    listener.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn silent_dialer_is_a_guest_after_the_wait() {
    let (listener, dialer) = pair_of(39, 40).await;
    let peer = listener.clone();
    let task = tokio::spawn(async move {
        let (mut s, _) = peer
            .accept_with(|_| Admission::Unknown)
            .await
            .expect("accept");
        let started = std::time::Instant::now();
        let intent = s
            .pair_intent(None, Duration::from_millis(400))
            .await
            .expect("intent");
        (intent, started.elapsed(), s)
    });
    let t = listener.ticket();
    let s = dialer.connect(t.endpoint, &t.addrs).await.expect("connect");
    let (intent, took, ls) = task.await.unwrap();
    assert_eq!(intent, PairIntent::Neither);
    assert!(took < Duration::from_secs(2), "{took:?}");
    s.close();
    ls.close();
    dialer.close().await;
    listener.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_notice_during_a_transfer_is_kept_for_the_host() {
    use std::sync::Arc;
    use vutric_net::transfer::{self, IncomingOffer, Outgoing, SaveTo};
    let dir = std::env::temp_dir().join(format!("vutric-intent-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("out")).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, b"hello").unwrap();

    let (listener, dialer) = pair_of(41, 42).await;
    let peer = listener.clone();
    let out = dir.join("out");
    let rx = tokio::spawn(async move {
        let (mut s, _) = peer
            .accept_with(|_| Admission::Trusted)
            .await
            .expect("accept");
        let offer = IncomingOffer::from_message(&s.recv().await.unwrap()).expect("offer");
        s.send_notice("Telegram", "mid-transfer", "", 7)
            .await
            .unwrap();
        transfer::receive(&mut s, offer, &SaveTo::folder(out), Arc::new(|_| {}))
            .await
            .expect("receive");
        s
    });
    let t = listener.ticket();
    let mut s = dialer.connect(t.endpoint, &t.addrs).await.expect("connect");
    transfer::send(
        &mut s,
        vec![Outgoing::file(&file, None).unwrap()],
        Arc::new(|_| {}),
    )
    .await
    .expect("send");
    match tokio::time::timeout(Duration::from_secs(5), s.recv())
        .await
        .unwrap()
        .unwrap()
    {
        Message::Notice(n) => assert_eq!(n.title, "mid-transfer"),
        other => panic!("expected the notice, got {}", other.kind()),
    }
    let ls = rx.await.unwrap();
    assert_eq!(
        std::fs::read(dir.join("out").join("a.txt")).unwrap(),
        b"hello"
    );
    s.close();
    ls.close();
    dialer.close().await;
    listener.close().await;
    let _ = std::fs::remove_dir_all(&dir);
}
