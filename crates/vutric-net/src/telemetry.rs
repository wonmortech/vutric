//! Sends a finished day's route report (backlog M2, ADR-0077), only when the user turned telemetry on and a
//! collector is set. One small JSON `POST`; no cookies, no identifiers, a fixed user agent without a version.

use std::time::Duration;

use anyhow::{Context as _, Result, ensure};
use vutric_core::telemetry::{Report, TelemetrySettings};

/// Posts `report` to the collector. `Ok(false)` without any network traffic when telemetry is off.
pub async fn send(
    http: &reqwest::Client,
    settings: &TelemetrySettings,
    report: &Report,
) -> Result<bool> {
    if !settings.ready() {
        return Ok(false);
    }
    let body = serde_json::to_vec(report)?;
    let resp = http
        .post(&settings.endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .timeout(Duration::from_secs(20))
        .body(body)
        .send()
        .await
        .context("telemetry")?;
    ensure!(
        resp.status().is_success(),
        "collector answered {}",
        resp.status()
    );
    Ok(true)
}

/// A random delay of up to `max` before sending, so reports do not all arrive at midnight UTC and the time of
/// the request says nothing about when the sessions were.
#[must_use]
pub fn jitter(max: Duration) -> Duration {
    let r = u64::from_le_bytes(
        iroh::SecretKey::generate().to_bytes()[..8]
            .try_into()
            .expect("8 bytes"),
    );
    let ms = u64::try_from(max.as_millis()).unwrap_or(u64::MAX).max(1);
    Duration::from_millis(r % ms)
}
