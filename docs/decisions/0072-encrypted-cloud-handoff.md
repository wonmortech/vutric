# ADR-0072: Encrypted cloud handoff, opt-in

Status: accepted (2026-11-19) · Stream: W3 · Backlog: R8 · Builds on: ADR-0007 (route list), ADR-0064 (link secret)

## Context

Some pairs of devices never reach each other: one side is behind a carrier NAT that the relay cannot hold open
long enough for a big file, or the receiver is offline. Backlog R8 asks for a last resort that is opt-in. The file
goes to S3-compatible storage, encrypted on the device, with the key only in the link fragment, an expiry, a size
cap, and the feature off by default. The storage must learn nothing but the size and the time.

## Decision

1. **Off by default.** `Settings.cloud` (`vutric_core::cloud::CloudSettings`) has `enabled: false`. It holds the
   endpoint, region, bucket, path style, `max_bytes` (default 2 GiB, at most 5 GiB, the limit of one S3 `PUT`) and
   `expiry_hours` (default 24, 1..=168, the limit of a SigV4 presigned URL). `migrate` clamps them.
   - The field is optional, so older settings files read as off, with no version bump.
   - Credentials are never in the settings file. The host keeps them in the OS keychain and passes `Credentials`
     to each call. `Debug` prints only the access key id.
2. **Container `VTCLOUD1`** (pure, `vutric_core::cloud`):
   - **Key:** a fresh random 32-byte key for each file, used for AES-256-GCM.
   - **Header:** 20 bytes, `magic | chunk size | plain length`. It is the AAD of every chunk, so a changed length
     fails.
   - **Chunk 0:** the metadata (postcard `Meta { name, mime }`), padded to 1 KiB so the name's length is hidden.
   - **File chunks:** 64 KiB each.
   - **Nonce:** the chunk index plus a "last" bit, safe because the key is never reused. Reordering, truncation and
     appended bytes are errors.
   - `Sealer` and `Opener` stream, so a 5 GiB file never sits in memory.
3. **Link:** `vutric://cloud?v=1&u=<presigned GET>&x=<expiry>&s=<container bytes>#<key hex>`.
   - The key lives only in the fragment, which is never sent to a server.
   - `https` is required, or plain `http` to a loopback host only (tests, a local MinIO).
   - An unknown, missing or repeated field is refused, and so is an expired link, before any request is made.
4. **Network** (`vutric_net::cloud`):
   - `upload` sends one presigned `PUT` (SigV4 query signing, `UNSIGNED-PAYLOAD`, valid for 15 minutes) with a
     streamed body. It returns the link and the random object key, `vutric/<32 hex>.vtc`.
   - `download` decrypts while it downloads into `<hash>.vutric-part`, renames it to the sanitized inner name only
     after the last chunk verifies, and deletes it on any error.
   - `delete` removes the object early, after the receiver confirms or when the sender cancels.
   - HTTP is the `reqwest` and rustls already in the tree through iroh: ring, and the OS certificate verifier through
     `rustls-platform-verifier`. No global crypto provider is installed. No new crates (`cargo metadata --locked`
     passes).
   - Licences: `aes-gcm`, `hmac` and `sha2` (RustCrypto) are MIT OR Apache-2.0, and so are `reqwest`, `rustls`
     and `rustls-platform-verifier`.
5. **Expiry on the storage side.** A presigned URL dies at `x`, but the object stays until it is deleted. The ADR
   asks the user to add a bucket lifecycle rule (expire `vutric/` after 1 day, or 7 for the maximum). The sender
   also deletes the object after a receipt. Without that rule the bucket fills slowly; it does not leak, because
   it holds ciphertext.
6. **Where it sits in the route list.** It comes after LAN, hotspot, USB and relay. It is offered only when
   `settings.cloud.ready()` is true and the direct routes failed. The link travels over whatever channel still
   works: the session's companion message, a QR code, or a chat app. That wiring is the UI's (W1/W2).

## Measured (written as code, not run in this round)

Under the working mode of 2026-11-19, the tests and fuzz targets of this ADR are written but not run locally. They
run in CI on the next integration.

| Test | What it checks |
|---|---|
| `cloud::tests` (core, 7 tests, these did run: pass) | seal/open in any slicing (empty, 1 B, chunk ± 1, 3 chunks + 7); wrong key, flipped byte, changed length, truncation, appended byte, swapped chunks, over the receiver's limit; sealer refuses wrong piece sizes; link round trip and refusals (expired, no key, version, repeated or unknown field, plain http off loopback); settings off and clamped; dates; **the presigned-URL example of the AWS documentation reproduces its signature `aeeed9bb…d404`** |
| `crates/vutric-net/tests/cloud_fake_s3.rs` | in-process fake bucket: upload → download is byte-identical with the inner name; the stored object has neither plaintext nor the name; expired and oversize links refused; delete then 404; a damaged object leaves no file or part file; nothing uploaded while off or above `max_bytes` |
| fuzz `cloud_handoff` | link parse never panics and re-encodes; opening any bytes with any key never yields more than the declared length |

**Not verified:** real AWS S3, Cloudflare R2, Backblaze B2 or MinIO. The signature matches the AWS reference
vector, and virtual-host vs path style is a setting, but a real bucket has not seen a request yet. A MinIO run
needs Docker, which was not running.

## Consequences

- The owner picks the default storage (question in `docs/streams/w3-notes.md`). Until then, there is no default
  endpoint and the user must enter one.
- Android: `rustls-platform-verifier` needs its JNI initialisation before `cloud::client()` works on a phone. W1
  wires that when the phone UI offers the handoff (`needs-w1` when it is scheduled).
- A future version can add multipart upload above 5 GiB without changing the container, because chunks are
  independent.
