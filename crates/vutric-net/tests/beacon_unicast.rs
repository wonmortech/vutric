//! Two beacons on loopback: a unicast query is heard and answered, a hidden device stays silent, and answers
//! are rate-limited per source.

use std::net::SocketAddr;
use std::time::Duration;

use vutric_core::EndpointId;
use vutric_net::beacon::{BeaconService, Heard};

async fn next(rx: &mut tokio::sync::broadcast::Receiver<Heard>, ms: u64) -> Option<Heard> {
    tokio::time::timeout(Duration::from_millis(ms), rx.recv())
        .await
        .ok()
        .and_then(Result::ok)
}

#[tokio::test]
async fn query_is_answered_unless_hidden() {
    let a = BeaconService::start_on(0, EndpointId([1; 32]), 4001, false).unwrap();
    let b = BeaconService::start_on(0, EndpointId([2; 32]), 4002, true).unwrap();
    let mut a_rx = a.subscribe();
    let mut b_rx = b.subscribe();
    let to_b: SocketAddr = ([127, 0, 0, 1], b.port()).into();

    assert!(a.query_to(to_b).await);
    let heard_by_b = next(&mut b_rx, 2000).await.expect("b hears the query");
    assert_eq!(heard_by_b.id, EndpointId([1; 32]));
    assert_eq!(heard_by_b.addr.port(), 4001);
    let heard_by_a = next(&mut a_rx, 2000).await.expect("b answers");
    assert_eq!(heard_by_a.id, EndpointId([2; 32]));
    assert_eq!(heard_by_a.addr, SocketAddr::from(([127, 0, 0, 1], 4002)));
    assert!(heard_by_a.phone);

    // A second query inside the rate-limit window is heard but not answered.
    assert!(a.query_to(to_b).await);
    assert!(next(&mut b_rx, 2000).await.is_some());
    assert!(next(&mut a_rx, 200).await.is_none(), "rate-limited");

    // Hidden: heard, never answered.
    tokio::time::sleep(Duration::from_millis(600)).await;
    b.set_answering(false);
    assert!(a.query_to(to_b).await);
    assert!(next(&mut b_rx, 2000).await.is_some());
    assert!(
        next(&mut a_rx, 300).await.is_none(),
        "hidden device answered"
    );
    assert!(b.health().heard >= 3);
}

#[tokio::test]
async fn garbage_is_ignored() {
    let b = BeaconService::start_on(0, EndpointId([2; 32]), 4002, false).unwrap();
    let mut b_rx = b.subscribe();
    let s = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let to_b: SocketAddr = ([127, 0, 0, 1], b.port()).into();
    for junk in [&b"VTRC"[..], &[0u8; 41][..], &[0xffu8; 1500][..]] {
        s.send_to(junk, to_b).await.unwrap();
    }
    assert!(next(&mut b_rx, 300).await.is_none());
    assert_eq!(b.health().heard, 0);
}
