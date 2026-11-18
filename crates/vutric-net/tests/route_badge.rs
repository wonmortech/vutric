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

/// Backlog R5a: two computers on one cable have only self-assigned addresses, and the QR carries link-local IPv6
/// without its zone. Here the ticket keeps only its zone-less `fe80::` hints; the dialer expands them to its own
/// interfaces and connects. Skipped (with a note) on a host with no link-local IPv6 address.
#[tokio::test(flavor = "multi_thread")]
async fn zone_less_link_local_hint_connects() {
    let desktop = Peer::bind([33; 32], "Laptop A", Platform::Windows, Reach::Lan)
        .await
        .expect("bind");
    let other = Peer::bind([34; 32], "Laptop B", Platform::Linux, Reach::Lan)
        .await
        .expect("bind");
    let mut ticket = desktop.ticket();
    ticket.addrs.retain(|a| match a {
        std::net::SocketAddr::V6(v6) => lan::is_link_local_v6(*v6.ip()) && v6.scope_id() == 0,
        std::net::SocketAddr::V4(_) => false,
    });
    if ticket.addrs.is_empty() {
        eprintln!("skipped: no link-local IPv6 address on this host");
        desktop.close().await;
        other.close().await;
        return;
    }
    let accept = tokio::spawn({
        let desktop = desktop.clone();
        async move { desktop.accept().await.expect("accept") }
    });
    let s = tokio::time::timeout(Duration::from_secs(15), other.connect_ticket(&ticket))
        .await
        .expect("in time")
        .expect("connect over link-local IPv6");
    let desktop_s = accept.await.unwrap();
    let mut badge = Badge::Connecting;
    for _ in 0..50 {
        badge = s.badge();
        if badge != Badge::Connecting {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let remote = s.remote_ip();
    println!("hints {:?}, badge {badge:?}, path {remote:?}", ticket.addrs);
    assert!(
        matches!(remote, Some(std::net::IpAddr::V6(v6)) if lan::is_link_local_v6(v6)),
        "the path is the link-local hint, not something mDNS found"
    );
    assert!(!badge.crosses_internet());
    s.close();
    desktop_s.close();
    other.close().await;
    desktop.close().await;
}
