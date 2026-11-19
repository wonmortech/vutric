//! Desktop BLE backend (backlog R4, ADR-0071), behind the `ble` feature: `btleplug` as the central. It scans for
//! Vutric phones, recognises paired ones by their rotating id, and writes a [`BlePayload`] (our pairing link, or
//! a wake-up) to the phone's GATT inbox, so the phone dials us over the LAN, the hotspot or the relay. Files never
//! go over BLE. `btleplug` has no peripheral role, so desktops do not advertise; phones do (W1/W6).
//!
//! Formats and rules are `vutric_core::ble`; only the radio is here.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use btleplug::api::{Central as _, Manager as _, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::{Adapter, Manager, Peripheral};
use uuid::Uuid;
use vutric_core::EndpointId;
use vutric_core::ble::{
    Advert, BleKey, BlePayload, INBOX_UUID, MIN_MTU, OUTBOX_UUID, Reassembler, SERVICE_UUID,
    chunks, resolve,
};

/// The Vutric advertisement in a device's service data, if it has a valid one.
#[must_use]
pub fn advert_of<S: std::hash::BuildHasher>(
    service_data: &HashMap<Uuid, Vec<u8>, S>,
) -> Option<Advert> {
    Advert::decode(service_data.get(&Uuid::from_u128(SERVICE_UUID))?)
}

/// A Vutric device in radio range.
#[derive(Debug, Clone)]
pub struct Seen {
    pub advert: Advert,
    /// The paired device behind the rotating id, if we have its key.
    pub id: Option<EndpointId>,
    pub rssi: Option<i16>,
    peripheral: Peripheral,
}

/// The first Bluetooth adapter, or an error the UI can show ("Bluetooth o'chiq yoki yo'q").
pub async fn adapter() -> Result<Adapter> {
    let manager = Manager::new().await.context("Bluetooth")?;
    manager
        .adapters()
        .await
        .context("Bluetooth adapters")?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no Bluetooth adapter"))
}

/// Scans for `wait` and returns the Vutric devices heard, paired ones resolved with `keys` at Unix second `now`.
pub async fn scan(
    adapter: &Adapter,
    wait: Duration,
    keys: &[(EndpointId, BleKey)],
    now: u64,
) -> Result<Vec<Seen>> {
    adapter
        .start_scan(ScanFilter {
            services: vec![Uuid::from_u128(SERVICE_UUID)],
        })
        .await
        .context("start BLE scan")?;
    tokio::time::sleep(wait).await;
    let _ = adapter.stop_scan().await;
    let mut out = Vec::new();
    for p in adapter.peripherals().await? {
        let Ok(Some(props)) = p.properties().await else {
            continue;
        };
        let Some(advert) = advert_of(&props.service_data) else {
            continue;
        };
        out.push(Seen {
            id: resolve(&advert.rid, now, keys.iter().map(|(i, k)| (*i, k))),
            advert,
            rssi: props.rssi,
            peripheral: p,
        });
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.rssi.unwrap_or(i16::MIN)));
    Ok(out)
}

async fn connected(s: &Seen) -> Result<()> {
    if !s.peripheral.is_connected().await.unwrap_or(false) {
        s.peripheral
            .connect_with_timeout(Duration::from_secs(10))
            .await
            .context("BLE connect")?;
    }
    s.peripheral
        .discover_services_with_timeout(Duration::from_secs(10))
        .await
        .context("BLE services")?;
    Ok(())
}

fn characteristic(s: &Seen, uuid: u128) -> Result<btleplug::api::Characteristic> {
    s.peripheral
        .characteristics()
        .into_iter()
        .find(|c| {
            c.uuid == Uuid::from_u128(uuid) && c.service_uuid == Uuid::from_u128(SERVICE_UUID)
        })
        .ok_or_else(|| anyhow!("not a Vutric GATT service"))
}

/// Writes `payload` to the device's inbox, in chunks that fit the negotiated MTU.
pub async fn send(s: &Seen, payload: &BlePayload) -> Result<()> {
    connected(s).await?;
    let inbox = characteristic(s, INBOX_UUID)?;
    let mtu = usize::from(s.peripheral.mtu()).max(MIN_MTU);
    for c in chunks(&payload.encode()?, mtu)? {
        s.peripheral
            .write(&inbox, &c, WriteType::WithResponse)
            .await
            .context("BLE write")?;
    }
    Ok(())
}

/// Reads the device's outbox: each read returns its next chunk until the payload is whole.
pub async fn read(s: &Seen) -> Result<BlePayload> {
    connected(s).await?;
    let outbox = characteristic(s, OUTBOX_UUID)?;
    let mut r = Reassembler::default();
    for _ in 0..=u16::MAX {
        let chunk = s.peripheral.read(&outbox).await.context("BLE read")?;
        if let Some(bytes) = r.push(&chunk)? {
            return Ok(BlePayload::decode(&bytes)?);
        }
    }
    bail!("BLE outbox never completed")
}

/// Ends the GATT connection (the phone also drops it after a short idle time).
pub async fn done(s: &Seen) {
    let _ = s.peripheral.disconnect().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_service_data_is_found_and_others_ignored() {
        let a = Advert {
            flags: vutric_core::ble::flag::PHONE,
            rid: [3; 8],
        };
        let mut data = HashMap::new();
        data.insert(Uuid::from_u128(1), vec![1, 2, 3]);
        assert_eq!(advert_of(&data), None);
        data.insert(Uuid::from_u128(SERVICE_UUID), a.encode().to_vec());
        assert_eq!(advert_of(&data), Some(a));
        assert_eq!(
            Uuid::from_u128(SERVICE_UUID).to_string(),
            "fc1be29b-cb87-4116-a6ba-5a8e1c9d918b"
        );
    }
}
