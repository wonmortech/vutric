//! Issue #9 on loopback: a typed address resolves to the device's id and QUIC address through the beacon; a
//! wrong address, a hidden device and a code that names someone else all end in `Silent` within the wait.

use std::time::{Duration, Instant};

use vutric_core::EndpointId;
use vutric_core::code::ShortCode;
use vutric_net::beacon::BeaconService;
use vutric_net::find::{self, NotFound, Target};

#[tokio::test]
async fn typed_address_finds_the_device() {
    let phone = BeaconService::start_on(0, EndpointId([1; 32]), 4001, true).unwrap();
    let desktop = BeaconService::start_on(0, EndpointId([2; 32]), 4002, false).unwrap();

    let target = Target::parse("127.0.0.1").unwrap();
    let found = find::resolve(
        &phone,
        target,
        desktop.port(),
        Duration::from_secs(3),
        false,
    )
    .await
    .expect("desktop answers");
    assert_eq!(found.id, EndpointId([2; 32]));
    assert_eq!(found.addr, ([127, 0, 0, 1], 4002).into());
    assert!(!found.phone);
}

#[tokio::test]
async fn silence_ends_in_time() {
    let phone = BeaconService::start_on(0, EndpointId([3; 32]), 4003, true).unwrap();
    let desktop = BeaconService::start_on(0, EndpointId([4; 32]), 4004, false).unwrap();
    let wait = Duration::from_millis(900);

    // Nobody listens on this port.
    let start = Instant::now();
    let err = find::resolve(&phone, Target::parse("127.0.0.1").unwrap(), 9, wait, false)
        .await
        .unwrap_err();
    assert_eq!(err, NotFound::Silent);
    assert!(start.elapsed() < wait + Duration::from_millis(500));

    // Hidden device: heard, never answers.
    desktop.set_answering(false);
    let err = find::resolve(
        &phone,
        Target::parse("127.0.0.1").unwrap(),
        desktop.port(),
        wait,
        false,
    )
    .await
    .unwrap_err();
    assert_eq!(err, NotFound::Silent);
    assert_eq!(err.key(), "find.silent");
}

#[tokio::test]
async fn code_only_accepts_its_own_device() {
    let code = ShortCode::of(&EndpointId([9; 32]));
    assert!(code.matches(&EndpointId([9; 32])));
    // Multicast does not loop back on one host, so a code finds nothing here; it must not take a stranger.
    let phone = BeaconService::start_on(0, EndpointId([5; 32]), 4005, true).unwrap();
    let _stranger = BeaconService::start_on(0, EndpointId([6; 32]), 4006, false).unwrap();
    let err = find::resolve(
        &phone,
        Target::Code(code),
        vutric_core::beacon::PORT,
        Duration::from_millis(800),
        false,
    )
    .await
    .unwrap_err();
    assert_eq!(err, NotFound::Silent);
}
