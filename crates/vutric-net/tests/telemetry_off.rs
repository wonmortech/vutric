//! M2 telemetry (ADR-0077): while off, or with no collector chosen, sending makes no connection at all.

use std::time::Duration;

use tokio::net::TcpListener;
use vutric_core::Platform;
use vutric_core::route::Badge;
use vutric_core::telemetry::{Day, Report, Tally, TelemetrySettings};
use vutric_net::{cloud, telemetry};

#[tokio::test(flavor = "multi_thread")]
async fn off_means_no_traffic() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("https://{}/v1/routes", listener.local_addr().unwrap());
    let http = cloud::client().unwrap();
    let mut day = Day::default();
    day.tally.record(Badge::Relay, false);
    let report = Report {
        v: 1,
        day: day.day,
        platform: Platform::Windows,
        tally: Tally::default(),
    };
    for settings in [
        TelemetrySettings {
            enabled: false,
            endpoint: endpoint.clone(),
        },
        TelemetrySettings {
            enabled: true,
            endpoint: String::new(),
        },
        TelemetrySettings {
            enabled: true,
            endpoint: endpoint.replace("https://", "http://"),
        },
    ] {
        assert!(!telemetry::send(&http, &settings, &report).await.unwrap());
    }
    let accepted = tokio::time::timeout(Duration::from_millis(300), listener.accept()).await;
    assert!(accepted.is_err(), "nobody connected");
}

#[test]
fn jitter_stays_below_its_bound() {
    for _ in 0..100 {
        assert!(telemetry::jitter(Duration::from_secs(3600)) < Duration::from_secs(3600));
    }
    assert_eq!(telemetry::jitter(Duration::ZERO), Duration::ZERO);
}
