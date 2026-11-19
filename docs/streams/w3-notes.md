# W3 notes (network / interop)

From 2026-11-19, W3 does not edit the shared docs (`backlog.md`, `workstreams.md`, `owner-questions.md`,
`roadmap.md`). The orchestrator moves these rows there when it integrates. Newest first within each section.

## Backlog rows

| Item | State | Evidence |
|---|---|---|
| R9 animated QR | [~] | ADR-0073: libcimbar (MPL-2.0, C++/OpenCV) not now; own `vutric_core::qrstream` (systematic + LT-style fountain frames over standard QR, BLAKE3 digest, peeling decoder); tests `qrstream::tests`, fuzz `qr_stream`; no camera run yet |
| R8 encrypted cloud handoff | [~] | ADR-0072: `vutric_core::cloud` (VTCLOUD1 AES-256-GCM container, link with the key only in the fragment, SigV4 presign matching the AWS vector), `Settings.cloud` off by default, `vutric_net::cloud` upload/download/delete; tests `cloud::tests`, `tests/cloud_fake_s3.rs`, fuzz `cloud_handoff`; no real bucket yet |

## Owner questions

- (W3) 2026-11-19 · Cloud handoff (ADR-0072): which storage should Vutric suggest by default? (a) none, the user
  enters their own S3 bucket (what is built); (b) Cloudflare R2 (no egress fees, 10 GB free); (c) a Wonmor-run
  bucket (we pay storage and must handle abuse reports even though we only see ciphertext). Default meanwhile: (a),
  and the feature stays off until the user turns it on.

## Cross-stream needs

(none yet this round)
