//! Receiving with the storage rules of backlog T2 and T7: a name clash answered by the user, and a chosen
//! folder that is gone.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use vutric_core::Platform;
use vutric_core::storage::Conflict;
use vutric_net::transfer::{self, AskConflict, IncomingOffer, OnProgress, SaveTo};
use vutric_net::{Peer, Reach, Session};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vutric-rules-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn quiet() -> OnProgress {
    Arc::new(|_| {})
}

async fn pair(a: [u8; 32], b: [u8; 32]) -> (Session, Session) {
    let rx = Peer::bind(a, "Receiver", Platform::Windows, Reach::Lan)
        .await
        .unwrap();
    let tx = Peer::bind(b, "Sender", Platform::Android, Reach::Lan)
        .await
        .unwrap();
    let ticket = rx.ticket();
    let accept = tokio::spawn(async move {
        let s = rx.accept().await.unwrap();
        (rx, s)
    });
    let out = tx.connect_ticket(&ticket).await.unwrap();
    let (rx, inc) = accept.await.unwrap();
    std::mem::forget(rx);
    std::mem::forget(tx);
    (out, inc)
}

async fn send_and_receive(
    files: Vec<PathBuf>,
    to: SaveTo,
    seeds: ([u8; 32], [u8; 32]),
) -> transfer::Summary {
    let list = transfer::collect(&files).unwrap();
    let (mut tx, mut rx) = pair(seeds.0, seeds.1).await;
    let recv = tokio::spawn(async move {
        let msg = rx.recv().await.unwrap();
        let offer = IncomingOffer::from_message(&msg).unwrap();
        let got = transfer::receive(&mut rx, offer, &to, quiet())
            .await
            .unwrap();
        (got, rx)
    });
    transfer::send(&mut tx, list, quiet()).await.unwrap();
    recv.await.unwrap().0
}

#[tokio::test(flavor = "multi_thread")]
async fn ask_on_a_clash_replace_or_skip() {
    let src = scratch("ask-src");
    let dst = scratch("ask-dst");
    std::fs::write(src.join("a.txt"), b"new a").unwrap();
    std::fs::write(src.join("b.txt"), b"new b").unwrap();
    std::fs::write(src.join("c.txt"), b"new c").unwrap();
    std::fs::write(dst.join("a.txt"), b"old a").unwrap();
    std::fs::write(dst.join("b.txt"), b"old b").unwrap();
    let asked = Arc::new(AtomicUsize::new(0));
    let asked2 = Arc::clone(&asked);
    let mut to = SaveTo::folder(&dst);
    to.rules.conflict = Conflict::Ask;
    to.ask = Some(AskConflict(Arc::new(move |path: PathBuf| {
        asked2.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if path.ends_with("a.txt") {
                Conflict::Replace
            } else {
                Conflict::Skip
            }
        })
    })));
    let files = ["a.txt", "b.txt", "c.txt"].map(|n| src.join(n)).to_vec();
    let got = send_and_receive(files, to, ([31; 32], [32; 32])).await;
    assert!(got.failed.is_empty(), "{got:?}");
    assert_eq!(
        asked.load(Ordering::SeqCst),
        2,
        "asked only about the two clashes"
    );
    assert_eq!(std::fs::read(dst.join("a.txt")).unwrap(), b"new a");
    assert_eq!(std::fs::read(dst.join("b.txt")).unwrap(), b"old b");
    assert_eq!(std::fs::read(dst.join("c.txt")).unwrap(), b"new c");
    assert_eq!(
        std::fs::read_dir(&dst).unwrap().count(),
        3,
        "no copies, no parts"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_folder_falls_back_to_the_default() {
    let src = scratch("gone-src");
    let dst = scratch("gone-dst");
    let gone = dst.join("unplugged-drive");
    std::fs::write(src.join("x.jpg"), b"photo").unwrap();
    let mut to = SaveTo::folder(&dst);
    to.rules.folder = Some(gone.clone());
    let got = send_and_receive(vec![src.join("x.jpg")], to, ([33; 32], [34; 32])).await;
    assert_eq!(got.missing_folder.as_deref(), Some(gone.as_path()));
    assert!(!gone.exists(), "the missing folder is not recreated");
    assert_eq!(got.saved.len(), 1);
    assert!(got.saved[0].starts_with(&dst));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_free_space_guard_refuses_and_tells_the_sender() {
    let src = scratch("full-src");
    let dst = scratch("full-dst");
    std::fs::write(src.join("big.mov"), b"movie").unwrap();
    let mut to = SaveTo::folder(&dst);
    to.min_free_bytes = u64::MAX / 2;
    let list = transfer::collect(&[src.join("big.mov")]).unwrap();
    let (mut tx, mut rx) = pair([35; 32], [36; 32]).await;
    let recv = tokio::spawn(async move {
        let msg = rx.recv().await.unwrap();
        let offer = IncomingOffer::from_message(&msg).unwrap();
        let r = transfer::receive(&mut rx, offer, &to, quiet()).await;
        (r.map(|_| ()), rx)
    });
    let sent = transfer::send(&mut tx, list, quiet()).await;
    let (got, _rx) = recv.await.unwrap();
    assert!(
        got.unwrap_err()
            .downcast_ref::<transfer::NoRoom>()
            .is_some()
    );
    assert!(
        sent.unwrap_err()
            .downcast_ref::<transfer::NoRoom>()
            .is_some(),
        "the sender is told why"
    );
    assert_eq!(
        std::fs::read_dir(&dst).unwrap().count(),
        0,
        "nothing written"
    );
}
