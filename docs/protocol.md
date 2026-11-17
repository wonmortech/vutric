# Vutric protocol v1

Normative description of what `crates/vutric-core` implements. Every device is a peer: any device can send to any
other, in both directions ([ADR-0005](decisions/0005-any-to-any-offline-first.md)); "sender" and "receiver" are per
transfer, not per platform. Transport is QUIC via iroh (TLS 1.3, peers
authenticated by Ed25519 endpoint key). ALPN: `vutric/1`.

## Pairing

The receiving side of a pairing (any device with a screen) shows a QR code with a pairing ticket:

```
vutric://pair?v=1&id=<64 hex endpoint id>&n=<percent-encoded device name>&a=<ip:port>,<ip:port>
```

- `v` must equal the protocol version; other versions are rejected, not guessed.
- `a` (optional, ≤ 8 entries) are LAN address hints for networks where mDNS/multicast is blocked.
- The name is display-only: control characters are stripped and it is cut to 64 characters.

After the other device connects with the ticket, both sides store each other's endpoint id. Later connections are
discovered via mDNS and authenticated by key; an unknown key is refused.

## Streams

| Stream | Direction | Content |
|---|---|---|
| Control (bidirectional, one per connection) | both | length-prefixed messages (below) |
| File (unidirectional, one per fling) | sender → receiver | 16-byte `FlingId`, then the raw file bytes |

### Framing

`u32` little-endian body length, then a postcard-encoded `Message`. Bodies over 128 KiB are a protocol error
detected from the header, before any allocation. Every decoded message is validated (`Message::validate`).

### Messages

| Message | From | Meaning |
|---|---|---|
| `Hello { protocol, device_name, platform }` | both, first | version check and display name |
| `FlingStart { id, file_name, mime, size, blake3, gesture, effect, preview }` | sender | a fling begins; the receiver starts the effect as soon as it arrives |
| `EffectStarted { id, receiver_micros }` | receiver | first effect frame presented (latency probe) |
| `FlingComplete { id }` | receiver | file received and BLAKE3 matched; it is now in the save folder |
| `FlingFailed { id, reason }` | both | `HashMismatch`, `DiskFull`, `Rejected`, `Cancelled`, `Paused` |

`gesture` = `{ speed (px/s ≥ 0), angle (rad, 0 = up, + = clockwise), exit_x (0..=1) }`. `preview` ≤ 96 KiB,
longest side ≤ 512 px, JPEG/WebP/PNG.

## Receiving a file

1. Write to `<save folder>/.vutric-partial/<FlingId>` while hashing with BLAKE3.
2. On size + hash match, sanitize the peer's name (`filename::sanitize`), pick a free name (`filename::unique`),
   and rename into the save folder. Only then send `FlingComplete`.
3. On mismatch delete the partial file and send `FlingFailed { HashMismatch }`.

The peer-supplied file name is hostile input: path components, Windows device names (`CON`, `NUL`, `COM1`…),
reserved characters and trailing dots/spaces are neutralised; see the tests in `crates/vutric-core/src/filename.rs`.

## Latency budget (finger release → first effect frame)

| Step | Budget |
|---|---|
| Phone: gesture end → `FlingStart` + preview written | 10 ms |
| LAN one-way (QUIC, warm connection) | 5–30 ms |
| Desktop: decode preview + upload texture | 10 ms |
| Desktop: first frame from warm overlay | ≤ 16 ms (measured 0.5–10.7 ms) |
| **Total** | **< 100 ms** |
