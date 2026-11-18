//! The W6 transport hook (ADR-0071): a device learned from a Wi-Fi Aware / Direct discovery message (`LinkHello`)
//! is dialed at the address the platform's link gave, here IPv6 loopback standing in for the Aware link.

use vutric_core::Platform;
use vutric_core::ble::{LinkHello, flag};
use vutric_net::{Peer, Reach};

#[tokio::test(flavor = "multi_thread")]
async fn a_link_hello_is_enough_to_dial() {
    let phone = Peer::bind([71; 32], "Pixel", Platform::Android, Reach::Lan)
        .await
        .unwrap();
    let other = Peer::bind([72; 32], "Pixel 2", Platform::Android, Reach::Lan)
        .await
        .unwrap();
    let hello = phone.link_hello(true);
    assert_eq!(hello.id, phone.id());
    assert!(hello.flags & flag::PHONE != 0);
    let wire = hello.encode();
    let heard = LinkHello::decode(&wire).expect("discovery message");

    let p = phone.clone();
    let accept = tokio::spawn(async move { p.accept().await.unwrap() });
    let s = other
        .connect_on_link(&heard, "::1".parse().unwrap(), 0)
        .await
        .expect("dial on the link");
    let ps = accept.await.unwrap();
    assert_eq!(s.remote_id(), phone.id());
    s.close();
    ps.close();
    other.close().await;
    phone.close().await;
}
