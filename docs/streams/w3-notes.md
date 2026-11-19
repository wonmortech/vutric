# W3 notes (network / interop)

From 2026-11-19, W3 does not edit the shared docs (`backlog.md`, `workstreams.md`, `owner-questions.md`,
`roadmap.md`). The orchestrator moves these rows there when it integrates. Newest first within each section.

## Backlog rows

| Item | State | Evidence |
|---|---|---|
| M2 direct vs relay share | [~] | ADR-0077: `vutric_core::telemetry` (off by default, `Tally` per UTC day by last badge, bucketed counts, report = `{v, day, platform, tally}` only), `Settings.telemetry`, `vutric_net::telemetry::send` + `jitter`; tests `telemetry::tests`, `tests/telemetry_off.rs`; no collector yet (owner question) |
| C8 adapter rules audit | [~] | Audit below (2026-11-19, from the code on `w3/net-4`): rules 1, 5 and 7 hold for all four adapters; rule 2's pinned shortcut is stored but not wired (every offer is asked, which is the safe side); rule 4 is a separate low-privilege process, not yet an OS sandbox; rule 6 only magic-wormhole runs against the real app in CI (M5 for the rest) |
| C3 Quick Share sending to Android | [~] | ADR-0076: `vutric_compat_quickshare::wake`: QR invite (P-256 key, HKDF token, hidden-name AES-GCM, `endpoint::records`, mDNS `browse`, `send_via_qr`) for any desktop; BLE wake-up bytes `0xFE2C` for W6/W2 advertisers; tests `wake::tests`, fuzz `quickshare_frames` extended; no real phone yet (M5) |
| F15 phone as mouse and keyboard | [~] | ADR-0075: `Message::Pointer` (wire 17), `vutric_core::pointer` (bounded fields, trusted only, `PointerLimit` with releases always passing, coalescing), `Session::pointer`; tests `pointer::tests`; injection W2, UI W1 |
| R9a pairing code over sound | [~] | ADR-0074: own 16-tone FSK + Goertzel codec `vutric_core::sound` (ggwave MIT but C++, not read); ≤ 32 bytes, CRC-8; tests `sound::tests`, fuzz `sound_code`; no speaker/microphone run yet |
| R9 animated QR | [~] | ADR-0073: libcimbar (MPL-2.0, C++/OpenCV) not now; own `vutric_core::qrstream` (systematic + LT-style fountain frames over standard QR, BLAKE3 digest, peeling decoder); tests `qrstream::tests`, fuzz `qr_stream`; no camera run yet |
| R8 encrypted cloud handoff | [~] | ADR-0072: `vutric_core::cloud` (VTCLOUD1 AES-256-GCM container, link with the key only in the fragment, SigV4 presign matching the AWS vector), `Settings.cloud` off by default, `vutric_net::cloud` upload/download/delete; tests `cloud::tests`, `tests/cloud_fake_s3.rs`, fuzz `cloud_handoff`; no real bucket yet |

## C8 audit: adapter rules (2026-11-19)

Read from the code, not from the ADRs. "Holds" means the evidence named is in the tree.

| Rule | LocalSend (C1) | Quick Share (C3) | KDE Connect (C4) | magic-wormhole (C7) |
|---|---|---|---|---|
| 1. Clean-room, no GPL code | holds: protocol `README.md` `62bd340` only (crate README) | holds: `NearDrop` `PROTOCOL.md` prose `f75f5fb`, UKEY2 doc, `.proto` field numbers (BSD-3) | holds: `kdeconnect-meta/protocol.md` only, the GPL clients not read (`lib.rs`) | holds: `magic-wormhole-protocols` `566ee9e` docs only (`lib.rs`) |
| 2. Foreign peers asked unless pinned | asked: every transfer is an `ipc::Event::Offer` decided by the daemon. **Gap:** `Settings.compat.*.pinned` / `Adapter::is_pinned` exist (storage.rs) but no adapter or host calls them, so pinned peers are still asked | asked (same `Offer` path) | asked; pairing shows `alias` + `code` and waits for `Decide` | asked (`Offer` with session `wh-<id>`); code-based, so no pinning |
| 3. Visible only in receive mode; "everyone" ends after 10 min | discovery runs only while receive mode is on; `EVERYONE_FOR_SECS = 600` in `trust.rs` | mDNS `Advert` lives only while held (`mdns.rs`); `wake::is_wake` can trigger it on demand | **not checked in code** this round: the KDE Connect UDP identity broadcast should follow receive mode too; to verify | not advertised at all (short code) |
| 4. Sandboxed process for pre-auth parsing | all four parse inside the separate `vutric-compat` process (ADR-0054). **Gap:** low-privilege process only; an OS sandbox (AppContainer, seccomp/landlock, App Sandbox) is still open (ADR-0054 "left open") | same | same | same |
| 5. Fuzzer per adapter | `localsend_prepare`, `localsend_announce`, `localsend_ipc` | `quickshare_frames` (now incl. `endpoint::records`, `QrInvite::matches`), `quickshare_secure` | `kdeconnect_packet` | `wormhole_messages` |
| 6. Real-app interop test | loopback + browser tests only; real app = M5 | our own client/receiver only (`tests/transfer.rs`); real phone = M5 | loopback, our client plays the phone (`tests/interop.rs`); real app = M5 | **holds**: `tests/interop.rs` runs the Python `wormhole` tool in `.github/workflows/interop.yml` (`VUTRIC_WORMHOLE_BIN`) |
| 7. Protocol re-read before writing | re-read 2026-11-18 | re-read 2026-11-18, and 2026-11-19 for ADR-0076 | read for ADR-0062 (date in the ADR) | read at `566ee9e` for ADR-0067 |

**Follow-ups:**

- (a) Wire `is_pinned` into the host's offer decision. That is W2's daemon, or the compat host in
  `vutric-mobile` for W1. The fingerprint for each adapter is the LocalSend cert SHA-256, the KDE Connect cert,
  and the Quick Share UKEY2 key.
- (b) Check that KDE Connect's broadcast stops when receive mode ends.
- (c) Choose the OS sandbox per platform. That needs its own ADR.
- (d) M5 real-app runs for LocalSend, Quick Share and KDE Connect.

## Owner questions

- (W3) 2026-11-19 · Cloud handoff (ADR-0072): which storage should Vutric suggest by default? (a) none, the user
  enters their own S3 bucket (what is built); (b) Cloudflare R2 (no egress fees, 10 GB free); (c) a Wonmor-run
  bucket (we pay storage and must handle abuse reports even though we only see ciphertext). Default meanwhile: (a),
  and the feature stays off until the user turns it on.

- (W3) 2026-11-19 · Route telemetry (ADR-0077) needs a collector: (a) a tiny endpoint on the relay host that keeps only daily sums and never logs IPs (what the ADR assumes), (b) a hosted privacy-first analytics service, or (c) none, keep M2 off for phase 1. Default meanwhile: (c), the endpoint stays empty, so nothing can be sent.

## Cross-stream needs

- **needs-w2 (F15, ADR-0075):** inject `Message::Pointer` on the desktop: a switch "Telefondan boshqarish" off by
  default, `pointer::accepts` + a `PointerLimit` per peer, a visible indicator, `ReleaseAll` when the session ends.
  HID usage → `SendInput` / `CGEventPost` / uinput or the RemoteDesktop portal. No TEMP shim needed: until then
  the desktop ignores the message (`_ => {}`).
- **needs-w1 / needs-w2 (C3, ADR-0076):** an "Android'ga yuborish (Quick Share)" action: show `QrInvite::url()` as a
  QR code, then `wake::send_via_qr`. **needs-w6:** optional BLE advertiser of `wake::wake_service_data_random()`
  under 16-bit UUID `0xFE2C`. **needs-w2 (Windows):** check whether `BluetoothLEAdvertisementPublisher` accepts that
  service data. No shim needed: nothing calls it yet.
- **needs-w1 / needs-w2 (M2, ADR-0077):** a "Anonim statistika yuborish" switch, off by default, with the exact
  report shown; count each ended session with `Tally::record(session.badge(), started_relayed)` into a `Day` kept
  in a small file; on start, `Day::roll(today)` and send the finished day after `telemetry::jitter(6 h)`.
- **needs-w1 (F15):** a touchpad + keyboard screen sending `Session::pointer` at about 120 Hz, moves coalesced.
