//! The ADB route of ADR-0070 on one machine: `adb reverse` makes the phone's `127.0.0.1:<port>` the computer's,
//! so both ends meet through a relay on loopback. Here both peers run on this machine and dial that relay
//! directly, with no LAN address at all. The session must say "Kabel", not "relayed", and big files are allowed.

use std::time::Duration;

use vutric_core::route::{Badge, Bulk};
use vutric_core::{Message, Platform};
use vutric_net::{Peer, Reach, Relays};
use vutric_relay::{Config, Relay};

#[tokio::test(flavor = "multi_thread")]
async fn a_loopback_relay_is_a_cable() {
    let relay = Relay::spawn(&Config::parse("[http]\nbind = \"127.0.0.1:0\"").unwrap())
        .await
        .unwrap();
    let url = format!("http://{}/", relay.http_addr().unwrap());
    let relays = Relays {
        urls: vec![url.clone()],
        n0_fallback: false,
    };
    let desktop = Peer::bind_with(
        [61; 32],
        "Ofis",
        Platform::Windows,
        Reach::Anywhere,
        &relays,
    )
    .await
    .unwrap();
    let phone = Peer::bind_with(
        [62; 32],
        "Pixel",
        Platform::Android,
        Reach::Anywhere,
        &relays,
    )
    .await
    .unwrap();
    assert!(desktop.wait_online(Duration::from_secs(10)).await);
    assert!(phone.wait_online(Duration::from_secs(10)).await);

    let mut ticket = desktop.ticket();
    assert_eq!(ticket.relay.as_deref(), Some(url.as_str()));
    ticket.addrs.clear(); // over adb, the loopback relay is the only way in
    let d = desktop.clone();
    let accept = tokio::spawn(async move { d.accept().await.unwrap() });
    let mut s = phone.connect_ticket(&ticket).await.unwrap();
    let mut ds = accept.await.unwrap();
    s.send(&Message::PairResult { confirmed: true })
        .await
        .unwrap();
    assert_eq!(
        ds.recv().await.unwrap(),
        Message::PairResult { confirmed: true }
    );

    // The path may turn direct (both ends are on this machine); either way it is never "relayed".
    assert!(s.relay_url().is_none_or(|u| u.contains("127.0.0.1")));
    assert!(!s.is_relayed(), "a loopback relay is a cable");
    assert_eq!(s.bulk(20 * 1024 * 1024 * 1024), Bulk::Go);
    if s.relay_url().is_some() {
        assert_eq!(s.badge(), Badge::Cable);
    }
    s.close();
    ds.close();
    phone.close().await;
    desktop.close().await;
    relay.shutdown().await.unwrap();
}
