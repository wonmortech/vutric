//! The phone's catch API (desktop → phone, phone → phone) against a real second peer in this process: the
//! "phone" side is the blocking API the Flutter app calls, the thrower is a plain `vutric-net` peer.

use std::sync::mpsc;
use std::time::Duration;

use vutric_core::{Gesture, Platform};
use vutric_mobile::api;
use vutric_net::{Peer, probe};

fn tiny_jpeg() -> Vec<u8> {
    let img = image::RgbImage::from_fn(64, 48, |x, y| image::Rgb([x as u8 * 4, y as u8 * 5, 120]));
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80)
        .encode_image(&img)
        .expect("encode");
    out
}

#[test]
fn phone_catches_a_thrown_photo_and_skips_probes() {
    let (link_tx, link_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let phone = std::thread::spawn(move || {
        let link = api::start("Test phone".into(), api::new_identity()).expect("start");
        link_tx.send(link).expect("send link");
        let c = api::accept_session().expect("accept");
        assert_eq!(c.peer_name, "Thrower");
        let caught = api::next_fling().expect("next fling");
        api::shown(caught.id.clone()).expect("shown");
        // Closing at once would drop the unsent answer; the thrower says when it has it.
        done_rx
            .recv_timeout(Duration::from_secs(20))
            .expect("thrower done");
        api::disconnect();
        caught
    });

    let link = link_rx
        .recv_timeout(Duration::from_secs(20))
        .expect("phone link");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");
    rt.block_on(async {
        let ticket = vutric_core::PairingTicket::parse(&link).expect("ticket");
        let peer = Peer::bind(vutric_net::new_identity(), "Thrower", Platform::Windows)
            .await
            .expect("bind");
        let mut s = peer
            .connect(ticket.endpoint, &ticket.addrs)
            .await
            .expect("connect");
        // A latency probe (not an image) is answered by next_fling itself and never reaches the screen.
        probe::send_one(&mut s, 1).await.expect("probe answered");
        let gesture = Gesture {
            speed: 2400.0,
            angle: 0.2,
            exit_x: 0.7,
        };
        let msg = probe::photo("cat.jpg", tiny_jpeg(), 64, 48, gesture);
        tokio::time::timeout(Duration::from_secs(10), probe::round_trip(&mut s, &msg))
            .await
            .expect("no timeout")
            .expect("shown() closes the round trip");
        done_tx.send(()).expect("done");
        s.close();
        peer.close().await;
    });

    let caught = phone.join().expect("phone thread");
    assert_eq!(caught.file_name, "cat.jpg");
    assert_eq!((caught.width, caught.height), (64, 48));
    assert!((caught.exit_x - 0.7).abs() < 1e-6);
    assert_eq!(caught.id.len(), 16);
}
