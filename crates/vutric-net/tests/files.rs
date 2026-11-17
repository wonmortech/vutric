//! Whole files between two peers on the LAN: a folder tree, empty and large files, more files than one offer
//! holds, resuming from a part file, a damaged part file, and a declined offer.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use vutric_core::Platform;
use vutric_net::transfer::{self, Declined, IncomingOffer, OnProgress, Outgoing};
use vutric_net::{Peer, Reach, Session};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vutric-files-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Deterministic, non-repeating bytes.
fn bytes(len: usize, seed: u8) -> Vec<u8> {
    let mut h = blake3::Hasher::new();
    h.update(&[seed]);
    let mut out = vec![0; len];
    h.finalize_xof().fill(&mut out);
    out
}

fn quiet() -> OnProgress {
    Arc::new(|_| {})
}

async fn pair(a: [u8; 32], b: [u8; 32]) -> (Session, Session) {
    let rx = Peer::bind(a, "Receiver", Platform::Windows, Reach::Lan)
        .await
        .unwrap();
    let tx = Peer::bind(b, "Sender", Platform::Ios, Reach::Lan)
        .await
        .unwrap();
    let ticket = rx.ticket();
    let accept = tokio::spawn(async move {
        let s = rx.accept().await.unwrap();
        (rx, s)
    });
    let out = tx.connect_ticket(&ticket).await.unwrap();
    let (rx, inc) = accept.await.unwrap();
    // Keep both endpoints alive for the test's length.
    std::mem::forget(rx);
    std::mem::forget(tx);
    (out, inc)
}

async fn offer_of(s: &mut Session) -> IncomingOffer {
    let msg = s.recv().await.unwrap();
    IncomingOffer::from_message(&msg).expect("an offer")
}

fn same(a: &Path, b: &Path) {
    assert_eq!(
        std::fs::read(a).unwrap(),
        std::fs::read(b).unwrap(),
        "{} vs {}",
        a.display(),
        b.display()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn folder_tree_and_many_files_arrive_verified() {
    let src = scratch("tree-src");
    let dst = scratch("tree-dst");
    let trip = src.join("Trip");
    std::fs::create_dir_all(trip.join("day 1")).unwrap();
    std::fs::write(trip.join("day 1").join("IMG_1.jpg"), bytes(5 << 20, 1)).unwrap();
    std::fs::write(trip.join("empty.txt"), b"").unwrap();
    std::fs::write(trip.join("one.bin"), b"x").unwrap();
    for i in 0..300 {
        std::fs::write(trip.join(format!("n{i:03}.txt")), bytes(100 + i, 2)).unwrap();
    }
    let doc = src.join("report.pdf");
    std::fs::write(&doc, bytes(70_000, 3)).unwrap();

    let files = transfer::collect(&[trip.clone(), doc.clone()]).unwrap();
    assert_eq!(files.len(), 304);
    let total: u64 = files.iter().map(|f| f.size).sum();

    let (mut tx, mut rx) = pair([11; 32], [12; 32]).await;
    let dst2 = dst.clone();
    let seen = Arc::new(AtomicU64::new(0));
    let seen2 = Arc::clone(&seen);
    let recv = tokio::spawn(async move {
        let offer = offer_of(&mut rx).await;
        assert_eq!(offer.total_files, 304);
        let on: OnProgress = Arc::new(move |p| seen2.store(p.bytes_done, Ordering::Relaxed));
        let got = transfer::receive(&mut rx, offer, &dst2, on).await.unwrap();
        (got, rx)
    });
    let sent = transfer::send(&mut tx, files, quiet()).await.unwrap();
    let (got, _rx) = recv.await.unwrap();

    assert!(
        sent.failed.is_empty() && got.failed.is_empty(),
        "{sent:?} {got:?}"
    );
    assert_eq!((sent.files, got.files), (304, 304));
    assert_eq!(got.bytes, total);
    assert_eq!(
        seen.load(Ordering::Relaxed),
        total,
        "final progress is complete"
    );
    same(
        &trip.join("day 1").join("IMG_1.jpg"),
        &dst.join("Trip").join("day 1").join("IMG_1.jpg"),
    );
    same(&trip.join("empty.txt"), &dst.join("Trip").join("empty.txt"));
    same(&trip.join("n299.txt"), &dst.join("Trip").join("n299.txt"));
    same(&doc, &dst.join("report.pdf"));
    let leftovers = std::fs::read_dir(dst.join("Trip"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".vutric-part")
        })
        .count();
    assert_eq!(leftovers, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn resumes_a_part_file_and_rejects_a_damaged_one() {
    let src = scratch("resume-src");
    let dst = scratch("resume-dst");
    let good = bytes(3 << 20, 4);
    let other = bytes(1 << 20, 5);
    std::fs::write(src.join("video.mp4"), &good).unwrap();
    std::fs::write(src.join("song.mp3"), &other).unwrap();
    // An earlier, interrupted transfer left the first 2 MiB of the video, and garbage for the song.
    std::fs::write(dst.join("video.mp4.vutric-part"), &good[..2 << 20]).unwrap();
    std::fs::write(dst.join("song.mp3.vutric-part"), vec![0u8; 1000]).unwrap();
    // A file of that name already exists: the new one must not overwrite it.
    std::fs::write(dst.join("video.mp4"), b"keep me").unwrap();

    let files = vec![
        Outgoing::file(src.join("video.mp4"), None).unwrap(),
        Outgoing::file(src.join("song.mp3"), None).unwrap(),
    ];
    let (mut tx, mut rx) = pair([21; 32], [22; 32]).await;
    let dst2 = dst.clone();
    let recv = tokio::spawn(async move {
        let offer = offer_of(&mut rx).await;
        let got = transfer::receive(&mut rx, offer, &dst2, quiet())
            .await
            .unwrap();
        (got, rx)
    });
    let sent = transfer::send(&mut tx, files, quiet()).await.unwrap();
    let (got, _rx) = recv.await.unwrap();

    assert_eq!(std::fs::read(dst.join("video.mp4")).unwrap(), b"keep me");
    assert_eq!(std::fs::read(dst.join("video (2).mp4")).unwrap(), good);
    assert_eq!(
        got.failed,
        vec!["song.mp3".to_owned()],
        "a damaged prefix is caught by the hash"
    );
    assert_eq!(
        sent.failed,
        vec!["song.mp3".to_owned()],
        "the sender hears about it"
    );
    assert!(
        !dst.join("song.mp3.vutric-part").exists(),
        "a damaged part is removed, so a retry starts clean"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_declined_offer_reports_declined() {
    let src = scratch("decline-src");
    std::fs::write(src.join("a.txt"), b"hi").unwrap();
    let (mut tx, mut rx) = pair([31; 32], [32; 32]).await;
    let recv = tokio::spawn(async move {
        let offer = offer_of(&mut rx).await;
        assert_eq!(offer.names(5), vec!["a.txt".to_owned()]);
        transfer::decline(&mut rx, &offer).await.unwrap();
        rx
    });
    let one = vec![Outgoing::file(src.join("a.txt"), None).unwrap()];
    let err = transfer::send(&mut tx, one, quiet()).await.unwrap_err();
    assert!(err.downcast_ref::<Declined>().is_some(), "{err:#}");
    drop(recv.await.unwrap());
}
