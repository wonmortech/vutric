//! Two Vutric peers that know nothing of each other's addresses meet through our own relay (no n0, no
//! lookup), exchange messages, the speed meter sees them, and the per-key cap holds.

use std::time::{Duration, Instant};

use vutric_core::{Message, Platform};
use vutric_net::{Peer, Reach, Relays, Speed};
use vutric_relay::{Config, Relay};

fn relay_config() -> Config {
    Config::parse("[http]\nbind = \"127.0.0.1:0\"").unwrap()
}

async fn peer(seed: u8, url: &str) -> Peer {
    let relays = Relays {
        urls: vec![url.to_owned()],
        n0_fallback: false,
    };
    Peer::bind_with([seed; 32], "t", Platform::Windows, Reach::Anywhere, &relays)
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn peers_meet_through_our_relay() {
    let relay = Relay::spawn(&relay_config()).await.unwrap();
    let url = format!("http://{}/", relay.http_addr().unwrap());

    let a = peer(1, &url).await;
    let b = peer(2, &url).await;
    assert!(
        a.wait_online(Duration::from_secs(10)).await,
        "a gets a home relay"
    );
    assert!(
        b.wait_online(Duration::from_secs(10)).await,
        "b gets a home relay"
    );
    assert_eq!(relay.caps.open(), 2);

    let mut ticket = b.ticket();
    assert_eq!(ticket.relay.as_deref(), Some(url.as_str()));
    ticket.addrs.clear(); // the relay is the only way in

    let accept = tokio::spawn(async move {
        let s = b.accept().await.unwrap();
        (b, s)
    });
    let t0 = Instant::now();
    let mut s = a.connect_ticket(&ticket).await.unwrap();
    let connect = t0.elapsed();
    let (b, mut sb) = accept.await.unwrap();
    let (route, rtt) = s.route();
    println!(
        "connected in {connect:?}, route {route:?}, rtt {rtt:?}, relay {:?}",
        s.relay_url()
    );

    let mut speed = Speed::default();
    speed.push(Instant::now(), s.traffic());
    for _ in 0..50 {
        s.send(&Message::PairResult { confirmed: true })
            .await
            .unwrap();
        assert_eq!(
            sb.recv().await.unwrap(),
            Message::PairResult { confirmed: true }
        );
    }
    let (up, _) = speed.push(Instant::now(), s.traffic());
    assert!(up > 0, "speed meter sees bytes");

    drop((s, sb));
    a.close().await;
    b.close().await;
    relay.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn per_key_cap_refuses_the_extra_connection() {
    let mut c = relay_config();
    c.limits.max_connections_per_key = 1;
    let relay = Relay::spawn(&c).await.unwrap();
    let url = format!("http://{}/", relay.http_addr().unwrap());

    let first = peer(3, &url).await;
    assert!(first.wait_online(Duration::from_secs(10)).await);
    let twin = peer(3, &url).await; // same key, a second app instance
    assert!(
        !twin.wait_online(Duration::from_secs(3)).await,
        "the second connection of one key is refused"
    );
    assert_eq!(relay.caps.open(), 1);
    first.close().await;
    twin.close().await;
    relay.shutdown().await.unwrap();
}
