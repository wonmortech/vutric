//! A client that opens QUIC but never finishes the Vutric handshake must not delay a real one: the listener
//! runs handshakes side by side.

use std::time::{Duration, Instant};

use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, RelayMode, SecretKey};
use vutric_core::{ALPN, PairingTicket, Platform};
use vutric_net::{HANDSHAKE_TIMEOUT, Peer};

#[tokio::test(flavor = "multi_thread")]
async fn stalled_client_does_not_block_a_real_one() {
    let listener = Peer::bind(vutric_net::new_identity(), "Listener", Platform::Windows)
        .await
        .expect("bind listener");
    let ticket = PairingTicket::parse(&listener.ticket().to_uri()).expect("ticket");
    let mut target =
        EndpointAddr::new(iroh::PublicKey::from_bytes(&ticket.endpoint.0).expect("key"));
    for a in &ticket.addrs {
        target = target.with_ip_addr(*a);
    }

    let accepting = tokio::spawn({
        let listener = listener.clone();
        async move { listener.accept().await }
    });

    // The bad client: a QUIC connection with the right ALPN that never opens the control stream.
    let stalled = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::from_bytes(&vutric_net::new_identity()))
        .alpns(vec![ALPN.to_vec()])
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .expect("bind stalled");
    let _quiet = stalled
        .connect(target, ALPN)
        .await
        .expect("stalled connect");

    // Give the listener time to start (and stall on) the bad handshake first.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let real = Peer::bind(vutric_net::new_identity(), "Real", Platform::Android)
        .await
        .expect("bind real");
    let t0 = Instant::now();
    let _s = real
        .connect(ticket.endpoint, &ticket.addrs)
        .await
        .expect("the real client connects while the bad one stalls");
    let waited = t0.elapsed();
    let s = accepting.await.expect("join").expect("listener accepted");
    assert_eq!(s.remote_name, "Real");
    assert!(
        waited < HANDSHAKE_TIMEOUT / 2,
        "real client waited {waited:?}"
    );

    real.close().await;
    stalled.close().await;
    listener.close().await;
}
