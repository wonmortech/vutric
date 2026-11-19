# ADR-0076: Sending to Android over Quick Share

Status: accepted (2026-11-19) · Stream: W3 · Backlog: C3 (sending side) · Builds on: ADR-0010 (adapter rules),
ADR-0056 (Quick Share receiver), ADR-0071 (BLE)

## Context

Vutric already receives from Android's Quick Share sheet over the LAN (ADR-0056), and `client::send` speaks the
sending side to our own receiver. A real Android phone, though, does not advertise its receiver over mDNS until
something asks it to. `NearDrop`'s `PROTOCOL.md` (Unlicense, commit `f75f5fb`, re-read 2026-11-19, protocol prose
only, no code) documents two triggers:

1. a BLE advertisement with 16-bit service UUID `0xFE2C` and service data `fc 12 8e 01 42`, nine zero bytes and
   ten random bytes;
2. a QR code `https://quickshare.google/qrcode#key=<url-safe base64>` scanned by the phone's camera. The key is
   `00 00 | 02/03 | X` of a throw-away P-256 key. The phone answers in its mDNS endpoint info with a record of type
   1 holding either `HKDF-SHA256(key, "advertisingContext")[..16]` (visible), or its name sealed with AES-GCM under
   `HKDF-SHA256(key, "encryptionKey")[..16]` with that token as AAD (hidden).

## Decision

`vutric_compat_quickshare::wake` provides both, behind the existing experimental label:

| Path | Who can do it | Built here |
|---|---|---|
| **QR invite** (primary for desktops) | any desktop; needs only the screen and the phone's camera | `QrInvite::new` / `url` / `from_url` / `matches` (visible token or hidden name decrypted), `endpoint::records`, `browse` (mDNS, blocking), `send_via_qr` (browse in 2 s windows until a receiver answers, then `client::send`) |
| **BLE wake-up** | a Vutric phone (W6 advertiser), and Windows if `BluetoothLEAdvertisementPublisher` accepts service data for `0xFE2C` (W2 to check). Not macOS (no API), and not through `btleplug` (central only) | `wake_service_data(random)`, `wake_service_data_random()`, `is_wake` (31-byte legacy advertisement) |

- The QR invite key is never the connection key. The UKEY2 handshake and the 4-digit code that follow are the
  existing `client::send`.
- `is_wake` also lets our own receiver notice that a phone nearby is looking for receivers. That can drive "show me
  in the share sheet" without advertising all the time (ADR-0010 §3.3).
- A hidden phone's decrypted name goes through the same display filter as endpoint names: no controls, no bidi
  overrides, at most 64 characters.
- C8 rules hold: clean-room (prose only), the parsers are fuzzed (`quickshare_frames` now also covers
  `endpoint::records` and `QrInvite::matches`), the process is the sandboxed adapter, and the protocol was re-read
  before writing.

## Measured (written as code, not run in this round)

| Test | What it checks |
|---|---|
| `wake::tests::wake_up_bytes` | the documented bytes, random tail, `is_wake`, fits 31 bytes |
| `wake::tests::qr_invites_round_trip_and_match_visible_and_hidden_answers` | URL form (35-byte key, `00 00`, prefix 2/3), visible token match with name, hidden sealed name (bidi override stripped), another invite matches neither, too-short key refused |
| `wake::tests::derivation_is_hkdf_sha256_with_an_empty_salt` | ring's empty salt equals RFC 5869's zero salt |
| fuzz `quickshare_frames` (extended) | records never exceed the input; `matches` on hostile info |

**Not verified:** a real Android phone. Neither trigger has been tried. It is not known whether base64 padding
matters in the QR URL (we send none and accept both), nor whether Windows lets an app advertise `0xFE2C` service
data. That waits for M5, real-app interop.

## Consequences

- W1/W2: a "Android'ga yuborish (Quick Share)" action that shows `invite.url()` as a QR code with the line
  "Telefon kamerasi bilan skanerlang", then calls `send_via_qr`. W6: optionally advertise `wake_service_data` from
  a Vutric phone to wake another Android. Noted in `docs/streams/w3-notes.md`.
