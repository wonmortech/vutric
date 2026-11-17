//! Two peers that share no address: the dialer drops the ticket's LAN hints and reaches the listener by key,
//! through the n0 relay and lookup, as a device on another network would (ADR-0006). Needs the internet, so it
//! is ignored by default: `cargo test -p vutric-net --test internet -- --ignored --nocapture`.

use std::time::{Duration, Instant};

use vutric_core::Platform;
use vutric_net::{Peer, Reach};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the internet"]
async fn reaches_a_peer_by_key_alone() {
    let t0 = Instant::now();
    let a = Peer::bind(
        vutric_net::new_identity(),
        "A",
        Platform::Windows,
        Reach::Anywhere,
    )
    .await
    .expect("bind a");
    let b = Peer::bind(
        vutric_net::new_identity(),
        "B",
        Platform::Android,
        Reach::Anywhere,
    )
    .await
    .expect("bind b");
    assert!(
        a.wait_online(Duration::from_secs(20)).await,
        "a never got a relay"
    );
    println!("online after {:?}", t0.elapsed());

    let mut ticket = a.ticket();
    assert!(ticket.relay.is_some(), "an online ticket carries its relay");
    ticket.addrs.clear();

    let listen = tokio::spawn(async move {
        let s = a.accept().await.expect("accept");
        let code = s.check_code().expect("code");
        (a, s, code)
    });
    let t1 = Instant::now();
    let s = tokio::time::timeout(Duration::from_secs(30), b.connect_ticket(&ticket))
        .await
        .expect("connect timed out")
        .expect("connect");
    println!("connected in {:?}, route {:?}", t1.elapsed(), s.route());
    let (_a, _sa, code) = listen.await.expect("listener");
    assert_eq!(s.check_code().expect("code"), code);
    assert_eq!(s.remote_name, "A");
}
