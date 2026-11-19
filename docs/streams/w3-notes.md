# W3 notes (network / interop)

From 2026-11-19, W3 does not edit the shared docs (`backlog.md`, `workstreams.md`, `owner-questions.md`,
`roadmap.md`). The orchestrator moves these rows there when it integrates. Newest first within each section.

## Backlog rows

| Item | State | Evidence |
|---|---|---|
| C3 Quick Share sending to Android | [~] | ADR-0076: `vutric_compat_quickshare::wake`: QR invite (P-256 key, HKDF token, hidden-name AES-GCM, `endpoint::records`, mDNS `browse`, `send_via_qr`) for any desktop; BLE wake-up bytes `0xFE2C` for W6/W2 advertisers; tests `wake::tests`, fuzz `quickshare_frames` extended; no real phone yet (M5) |
| F15 phone as mouse and keyboard | [~] | ADR-0075: `Message::Pointer` (wire 17), `vutric_core::pointer` (bounded fields, trusted only, `PointerLimit` with releases always passing, coalescing), `Session::pointer`; tests `pointer::tests`; injection W2, UI W1 |
| R9a pairing code over sound | [~] | ADR-0074: own 16-tone FSK + Goertzel codec `vutric_core::sound` (ggwave MIT but C++, not read); ≤ 32 bytes, CRC-8; tests `sound::tests`, fuzz `sound_code`; no speaker/microphone run yet |
| R9 animated QR | [~] | ADR-0073: libcimbar (MPL-2.0, C++/OpenCV) not now; own `vutric_core::qrstream` (systematic + LT-style fountain frames over standard QR, BLAKE3 digest, peeling decoder); tests `qrstream::tests`, fuzz `qr_stream`; no camera run yet |
| R8 encrypted cloud handoff | [~] | ADR-0072: `vutric_core::cloud` (VTCLOUD1 AES-256-GCM container, link with the key only in the fragment, SigV4 presign matching the AWS vector), `Settings.cloud` off by default, `vutric_net::cloud` upload/download/delete; tests `cloud::tests`, `tests/cloud_fake_s3.rs`, fuzz `cloud_handoff`; no real bucket yet |

## Owner questions

- (W3) 2026-11-19 · Cloud handoff (ADR-0072): which storage should Vutric suggest by default? (a) none, the user
  enters their own S3 bucket (what is built); (b) Cloudflare R2 (no egress fees, 10 GB free); (c) a Wonmor-run
  bucket (we pay storage and must handle abuse reports even though we only see ciphertext). Default meanwhile: (a),
  and the feature stays off until the user turns it on.

## Cross-stream needs

- **needs-w2 (F15, ADR-0075):** inject `Message::Pointer` on the desktop: a switch "Telefondan boshqarish" off by
  default, `pointer::accepts` + a `PointerLimit` per peer, a visible indicator, `ReleaseAll` when the session ends.
  HID usage → `SendInput` / `CGEventPost` / uinput or the RemoteDesktop portal. No TEMP shim needed: until then
  the desktop ignores the message (`_ => {}`).
- **needs-w1 / needs-w2 (C3, ADR-0076):** an "Android'ga yuborish (Quick Share)" action: show `QrInvite::url()` as a
  QR code, then `wake::send_via_qr`. **needs-w6:** optional BLE advertiser of `wake::wake_service_data_random()`
  under 16-bit UUID `0xFE2C`. **needs-w2 (Windows):** check whether `BluetoothLEAdvertisementPublisher` accepts that
  service data. No shim needed: nothing calls it yet.
- **needs-w1 (F15):** a touchpad + keyboard screen sending `Session::pointer` at about 120 Hz, moves coalesced.
