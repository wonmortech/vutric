# ADR-0132: BLE plumbing on Android for finding and handshake

- Status: accepted, implemented in Kotlin (stream W6) against an interface. The frame codec is W3's (#24); a TEMP
  codec stands in. Compiles; nothing has been tried on a real phone yet.
- Date: 2026-11-18
- Builds on: [ADR-0007](0007-every-route-and-storage-settings.md) (BLE for finding and handshake only),
  [ADR-0009](0009-native-modules-kotlin-swift.md) part 2 (`Nearby`), [ADR-0130](0130-w6-share-shortcuts-and-transfer-job.md).
- Backlog: R4 (phone side), A7 (BLE part), F3.

## Context

R4 says: BLE finds devices and carries the handshake (ticket, hotspot keys, wake-up, at most 64 KB), with rotating
ids and never a contact hash. W3 is defining the frame format on `w3/net-3` (PR #20, R4 still open there). The
Android radio work does not depend on the format, only on its sizes, so W6 builds it now behind `BleCodec`.

## Decision

| # | Decision | Why | Where |
|---|---|---|---|
| 1 | **Kotlin never reads a payload.** `BleCodec` has three calls: `advert()` (bytes + `rotationMs`), `onHeard(address, rssi, serviceData)`, `onChunk(link, chunk) -> replies` | The format, the rotating id and the trust check live in Rust (ADR-0009 rule 1) | `BleNearby.kt` |
| 2 | Our 128-bit service UUID `7144296c-8360-4d5c-bf1b-edf2e74ccebc` goes in the advert; the codec's bytes go in the **scan response** as service data under the same UUID: **at most 13 bytes** (31 − 2 − 16) | Works on every BLE phone (legacy advertising); extended advertising only if W3 needs more (#24) | `BleRules.MAX_ADVERT`, `fit` |
| 3 | The advertiser restarts with fresh bytes every `rotationMs` (at least 30 s; default 15 min). Android also rotates the random MAC by itself | Rotating ids on the air, as R4 asks | `BleRules.rotation` |
| 4 | The scanner filters on our service UUID in the controller, `SCAN_MODE_BALANCED` | Other devices' adverts never reach the app; less battery | `BleNearby.scan` |
| 5 | GATT: one primary service, `rx` `45ad6c06-d329-4bcb-9037-aad8a3c03e8e` (write, write without response) and `tx` `8e24cee5-90ac-4895-99bc-94973a331eb8` (notify + CCCD). Both roles are built: server (we advertise) and client (`connect` / `send` to a device we heard). Chunks are `mtu − 3` bytes; the client asks for MTU 517 | Phone ⇄ phone and phone ⇄ PC, whichever side advertises | `BleNearby` server and client parts |
| 6 | Notifications and writes go **one at a time** (next on `onNotificationSent` / `onCharacteristicWrite`); a failed one drops that link's queue and reports `failed` | Android's GATT stack loses data when writes overlap | `pumpServer`, `pumpClient` |
| 7 | **TEMP until W3 #24:** `TempBleCodec` advertises 13 random bytes, resolves nothing and answers nothing | Nothing of the identity or the trust list goes on the air before the real codec | `TempBleCodec` |
| 8 | Permissions: `BLUETOOTH_SCAN` with `neverForLocation`, `BLUETOOTH_ADVERTISE`, `BLUETOOTH_CONNECT` on 12+; legacy `BLUETOOTH` / `BLUETOOTH_ADMIN` up to 11 plus fine location for the scan there (already declared up to 12) | Only what each role needs; `ble_scan` and `ble_advertise` can be asked apart | `W6Permissions` |

## What W1 wires

1. A Rust-backed `BleCodec` (through the bridge, later UniFFI) set into `BleNearby.codec`.
2. `w6.bleStart` while the "find nearby" screen is open (and, once UniFFI exists, from the tile); `w6.bleStop` when it closes.
3. On `w6.ble` `heard` for a paired device: `w6.bleConnect`; the codec's replies then flow by themselves.

## Verified

- `BleRulesTest` (4): adverts never exceed 13 bytes, rotation bounds, chunks fit every MTU and join back, the TEMP
  codec rotates and answers nothing.

## Device checklist (owner)

- [~] 1. Two Android 12+ phones, both `w6.bleStart`: each logs `w6.ble` `heard` for the other within 5 s.
- [~] 2. nRF Connect on a third phone: the advert shows only the service UUID; the scan response 13 bytes that change after the rotation.
- [~] 3. `w6.bleConnect` from A to B: `linked` on both; a 2 KB `w6.bleSend` arrives as chunks of `mtu − 3`.
- [~] 4. Android 11 phone: scanning asks for location; Android 12+: no location prompt.
- [~] 5. Bluetooth off: `failed` `bluetooth_off`, no crash.

## Consequences

- The radio work is done once; when W3's codec lands only `BleNearby.codec` changes.
- Wake-up of a sleeping phone by BLE needs the scan to run without the app (UniFFI + a foreground or companion
  path); not built here.
