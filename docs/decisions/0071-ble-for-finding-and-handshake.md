# ADR-0071: BLE for finding and the handshake only

Status: accepted (2026-11-18) · Stream: W3 · Backlog: R4 (also the frame format W6 needs for Wi-Fi Aware/Direct
and the BLE GATT server) · Builds on: ADR-0007 (route list), ADR-0052 (beacon), ADR-0058 (hotspot in the link),
ADR-0064 (link secret)

## Context

BLE reaches devices that share no network at all, and it can wake a phone app that is asleep (the way Quick Share
wakes Android). It is also slow (a few KB/s in practice) and easy to abuse for tracking. Backlog R4 sets the limits.
BLE is for finding and the handshake only. IDs rotate and are never hashes of contacts. It carries the ticket and the
hotspot keys, at most 64 KB, plus a wake-up. The orchestrator added a constraint: W6 (Android native extras)
builds the phone's GATT server and Wi-Fi Aware/Direct, and needs a stable frame format and a transport hook from W3.

## Decision

### 1. Over the air (`vutric_core::ble`, pure)

| Piece | Format | Notes |
|---|---|---|
| Service | UUID `fc1be29b-cb87-4116-a6ba-5a8e1c9d918b` (random v4) | a 16-bit UUID would need Bluetooth SIG membership |
| Advertisement | service data for that UUID: `[version=1][flags][rotating id: 8 bytes]` (`Advert`, 10 bytes) | flags (3) + header (2) + UUID (16) + 10 = 31, a legacy advertisement; no extended advertising needed |
| Flags | bit 0 phone, bit 1 pairing window open, bit 2 wake request | unknown bits are 0 when sent and ignored when read; another version is ignored |
| Rotating id | `BLAKE3-keyed(key ‖ H("vutric/1 ble rid")[..16], epoch LE)[..8]`, epoch = 15 minutes | `rotating_id`, `epoch` |
| Key | `BleKey::from_identity(secret)` = `derive_key("vutric/1 ble rotating id key", identity secret)[..16]` | never stored separately; `Debug` prints `BleKey(..)` |
| GATT inbox `0f34b527-843f-43b5-8ceb-856be921f7b5` | write with response: chunks of a `BlePayload` | the other side writes our link or a wake-up |
| GATT outbox `2fffdfd4-abf9-4d7a-a6aa-377958c9f563` | read: each read returns the next chunk | a stranger in the pairing window may read the link |
| `BlePayload` | postcard `Link { uri }` (≤ 2048 bytes, `vutric://`, printable ASCII) or `Wake`; total ≤ 64 KiB | the link already carries the hotspot keys and the one-time secret |
| Chunk | `[index u16 LE][count u16 LE][bytes]`, each ≤ ATT MTU − 3 | `chunks`, `Reassembler` (any order, repeats tolerated, a changed count or a conflicting repeat refused) |
| `LinkHello` | `[version=1][flags][port u16 LE][endpoint id 32]`, 36 bytes | the Wi-Fi Aware discovery message (fits 255-byte service-specific info), and a GATT read for an immediate dial |

**Privacy.**

- The rotating id says nothing to a device without the key. A new value comes every 15 minutes, and a resolver
  tries the epoch before and after the current one.
- It is never a hash of a phone number, an e-mail address or the endpoint id. AirDrop's contact-hash design leaked
  numbers, and we do not repeat it.
- The key reaches only paired devices, through the new message `Message::BleKey { key }`:
  - it is appended at wire index 16 and goes over the authenticated session;
  - the receiver takes it only from a trusted peer (`ble::accepts_key`);
  - the receiver keeps it in `TrustedDevice.ble_key` (`TrustStore::set_ble_key`, `ble_keys`). That is an optional
    field, missing from older files, so the trust file needs no version bump.
- While the pairing window is open, a device advertises a fresh random id with the pairing flag. Strangers can then
  find it, but cannot follow it across windows.

**Security.**

- Every BLE byte is unauthenticated, like the LAN beacon.
- A `Link` is dialed and then checked by QUIC: the key in the link must answer. A stranger is also checked by
  the link's one-time secret or the six digits.
- `Wake` only asks the app to open a session to a device it already trusts.
- Every decoder is bounded and fuzzed (`ble_frames`). Proptest checks that:
  - any payload survives chunking in any order, with every chunk sent twice;
  - a rotating id never resolves to the wrong key;
  - the decoders never panic, and what they accept re-encodes to the same value.

### 2. Desktop backend: `btleplug`, feature `ble` in `vutric-net`

- `vutric_net::ble` provides `adapter`, `scan` (resolves paired devices, strongest signal first), `send` (chunks
  sized to the negotiated MTU, written to the inbox), `read` (outbox until whole) and `done`.
- `btleplug` 0.13.5:
  - Licence: BSD-3-Clause, with MIT/Apache-2.0 parts from its origin (`LICENSE.md`). This is compatible with our
    Apache-2.0.
  - Maintenance: the last release, 0.13.5 (2026-10-10), updated `bluez-async` and fixed CoreBluetooth. The repo
    has about 1.2k stars and is not archived.
- `btleplug` is a central only: it has no peripheral or advertising role. So the desktop scans and writes, and phones
  advertise and serve GATT. A Windows GATT server through `GattServiceProvider` (the `windows` crate) is possible
  later, if a phone must find a desktop that has no network. That is W2's call.
- The feature is off by default. Linux needs `libdbus-1-dev` to build `btleplug`, and the CI image has no such
  step, so CI builds without it. It was built and tested with `--features ble` on Windows (below).

### 3. The W6 hooks (stable API)

- **Frame format:** everything in `vutric_core::ble` above. It is versioned: a new format means a new version byte,
  and old readers ignore it.
- **Transport hook for Wi-Fi Aware / Wi-Fi Direct:**
  - `Peer::link_hello(phone)` gives our id and QUIC port to put in the Aware discovery message, or in the Wi-Fi
    Direct service record.
  - `Peer::connect_on_link(&hello, ip, scope)` dials the peer at the address the platform's data path gave. For
    Aware that is an IPv6 link-local address, with the Aware interface index as `scope`.
  - The iroh endpoint already listens on `[::]` and `0.0.0.0`, so the responder needs nothing more. On Android,
    W6 binds the process to the Aware or Direct `Network` for the session (`bindProcessToNetwork`), as the hotspot
    join does (ADR-0013 #6).
  - QUIC authenticates `hello.id`. The result is an ordinary `Session`, with trust, transfers and companion
    messages.
  - No socket-descriptor hook is needed. iroh owns its UDP sockets, and a scoped link-local address routes on its
    interface. If W6 finds an Android version that routes it wrong, the fallback is a per-session socket fd handed
    to iroh's custom transport, and that request goes in a `needs-w3` issue.
- **Route badge:** Aware and Direct sessions show "Hotspot" (a direct Wi-Fi link without a router). A new `Badge`
  variant would break W1's exhaustive match, so it waits until W1 and W6 ask for one.

## Measured

| Check | Result |
|---|---|
| `vutric-core` `ble::tests` (advertisement fits 31 bytes, unknown bits and versions ignored; ids change per epoch, resolve ±1 epoch, never a stranger; hex JSON; only trusted devices hand over keys; payload checks; `LinkHello` round trip and port 0 refused; reassembler refuses changed counts and conflicting repeats) + 3 proptest properties (256 cases each) | pass |
| `trust::tests::ble_keys_only_for_trusted_devices_and_kept_in_json`, `message::tests::appended_variants_keep_old_wire_indices` (index 16) | pass |
| `crates/vutric-net/tests/link_hook.rs`: a `LinkHello` encoded, decoded, and `connect_on_link` over `::1` opens a session to the right key | pass (0.2 s) |
| `cargo clippy -p vutric-net --features ble --all-targets -D warnings` and `cargo test -p vutric-net --features ble --lib ble` on Windows (`btleplug` + `windows` crate) | pass |
| fuzz `ble_frames` | stable smoke pass; libFuzzer in CI |

**Not verified:** any real radio. There was no BLE scan against a phone, and no measured GATT throughput or MTU.
That needs the phone's GATT server (W6/W1 #27) and a device run.

## Consequences

- W6 builds the Android advertiser and GATT server on this format. The scanner side of Android
  (`BluetoothLeScanner` with a service-data filter) can use the same `Advert::decode` through the bridge.
- Sending to a Quick Share device still needs Google's own BLE wake-up (C3). That is not this format.
- When a desktop peripheral role exists, the same advertisement and GATT layout apply unchanged.
