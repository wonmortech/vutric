//! Backlog Rx on a real iroh session: two local-only peers on this machine show "To'g'ridan-to'g'ri · LAN" once a
//! path is chosen, and the badge never claims the internet for a LAN-only endpoint.

use std::time::Duration;

use vutric_core::Platform;
use vutric_core::route::Badge;
use vutric_net::{Peer, Reach, lan};

#[test]
fn picker_sets_the_reach() {
    use vutric_core::route::Picker;
    assert_eq!(Reach::for_picker(Picker::Auto), Reach::Anywhere);
    assert_eq!(Reach::for_picker(Picker::Internet), Reach::Anywhere);
    for p in [Picker::LanOnly, Picker::Offline, Picker::Bluetooth] {
        assert_eq!(Reach::for_picker(p), Reach::Lan, "{p:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn lan_session_shows_the_lan_badge() {
    let desktop = Peer::bind([31; 32], "Studio PC", Platform::Windows, Reach::Lan)
        .await
        .expect("bind desktop");
    let phone = Peer::bind([32; 32], "Pixel 8", Platform::Android, Reach::Lan)
        .await
        .expect("bind phone");
    let ticket = desktop.ticket();
    let accept = tokio::spawn({
        let desktop = desktop.clone();
        async move { desktop.accept().await.expect("accept") }
    });
    let s = phone
        .connect(ticket.endpoint, &ticket.addrs)
        .await
        .expect("connect");
    let desktop_s = accept.await.unwrap();

    let locals = lan::local_addrs();
    let mut badge = Badge::Connecting;
    for _ in 0..50 {
        badge = s.badge_with(&locals);
        if badge != Badge::Connecting {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(badge, Badge::DirectLan);
    assert!(!badge.crosses_internet());
    assert_eq!(desktop_s.badge(), Badge::DirectLan);

    s.close();
    desktop_s.close();
    phone.close().await;
    desktop.close().await;
}
