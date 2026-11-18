# Vutric protocol v2

Normative description of what `crates/vutric-core` (messages) and `crates/vutric-net` (streams, transfer)
implement. Every device is a peer: any device can send to any other, in both directions
([ADR-0005](decisions/0005-any-to-any-offline-first.md)); "sender" and "receiver" are per transfer, not per
platform. Transport is QUIC via iroh (TLS 1.3, peers authenticated by Ed25519 endpoint key), on the same network
or over the internet, direct or through an encrypted relay ([ADR-0006](decisions/0006-any-network-any-file.md)).
ALPN: `vutric/1`. Protocol version 2 added file offers; a v1 peer is refused at `Hello`.

## Pairing

The receiving side of a pairing (any device with a screen) shows a QR code with a pairing ticket:

```
vutric://pair?v=2&id=<64 hex endpoint id>&n=<percent-encoded device name>&a=<ip:port>,<ip:port>&r=<relay url>
```

- `v` must equal the protocol version; other versions are rejected, not guessed.
- `a` (optional, ≤ 8 entries) are LAN address hints for networks where mDNS/multicast is blocked, best first
  (wired, Wi-Fi, tethering, other, virtual adapters last). A link-local IPv6 hint is written without a zone
  (`[fe80::1]:4000`); the dialer tries it on each of its own interfaces ([ADR-0052](decisions/0052-lan-finding-beacon-and-diagnosis.md)).
- `r` (optional) is the `https://` relay the device is reachable through from other networks.
- The name is display-only: control characters are stripped and it is cut to 64 characters. It is never
  published to the lookup service.
- `ws`, `wk`, `wp`, `wh` (optional, together) are a hotspot to join first: SSID, `wpa2`/`wpa3`/`open`,
  passphrase (both percent-encoded) and `wh=1` for a hidden network
  ([ADR-0058](decisions/0058-hotspot-in-the-pairing-link.md)). A half hotspot is an error; older apps ignore the
  keys and pair as before.
- `s` (optional) is a one-time pairing secret, 32 hex characters (16 random bytes), present only while the
  device's pairing window is open ([ADR-0064](decisions/0064-listener-knows-how-the-dialer-pairs.md)). A dialer
  that read it proves it right after `Hello` (`PairProof`, below). A malformed or repeated `s` makes the link
  invalid; older apps ignore the key.

### Without the QR: typed address or short code ([ADR-0059](decisions/0059-connect-by-typed-address-or-code.md))

Next to the QR the device shows its LAN address and a short code: the first 40 bits of its endpoint id in
Crockford base32, `ABCD-EFGH` (`vutric_core::code::ShortCode`; typing forgives case, dashes, `O`/`0`, `I`/`L`/`1`).
The other device sends beacon queries (unicast to a typed IPv4 address; multicast plus the opt-in scan for a code)
and takes the first answer from that address, or whose id starts with the code. That answer is unauthenticated,
so the connection must finish the 6-digit comparison below and is stored as `PairMethod::Code`, never `Ticket`.

After the other device connects with the ticket, both sides store each other's endpoint id in their trust store
(`PairMethod::Ticket`). Later connections are discovered via mDNS, or looked up by key, and authenticated by key.

### Admission ([ADR-0050](decisions/0050-trust-store-and-pairing-code.md))

A listener decides on every incoming connection as soon as QUIC has authenticated the peer's key, before it reads
the control stream:

| Visibility | Trusted key | Unknown key |
|---|---|---|
| `off` | refused | refused |
| `trusted_only` (default) | admitted | refused |
| `everyone(until)`, at most 10 min | admitted | admitted, every offer asked about |
| `pairing(until)`, at most 10 min | admitted | admitted, to pair |

A refused connection is closed with QUIC application error code `403` ("not trusted"); nothing the peer sent is
parsed.

### Pairing from the nearby list (no QR)

Commit-then-reveal numeric comparison on the control stream, right after `Hello`. `x` = 32 bytes of TLS exporter
output with label `EXPORTER-vutric/1 pairing sas` and an empty context; `a`, `b` are 32 random bytes.

| Step | From | Message |
|---|---|---|
| 1 | dialer | `PairCommit { commit = BLAKE3-derive-key("vutric/1 2026-11-18 pairing commitment", x ‖ a) }` |
| 2 | listener | `PairNonce { nonce = b }`, only after step 1 arrived |
| 3 | dialer | `PairReveal { nonce = a }`; the listener checks it against the commitment |
| 4 | both | show `code = u64_le(BLAKE3-derive-key("vutric/1 2026-11-18 pairing code", x ‖ a ‖ b)[..8]) mod 10^6`, six digits |
| 5 | both | `PairResult { confirmed }`; the key is trusted (`PairMethod::Code`) only when both are `true` |

Any other message in steps 1–3, or a reveal that does not open the commitment, aborts the pairing as tampered.

### How the listener knows which check runs ([ADR-0064](decisions/0064-listener-knows-how-the-dialer-pairs.md))

A dialer that pairs sends its first message right after `Hello`:

| Dialer found the device by | First message | Listener stores |
|---|---|---|
| the QR or link with `s` | `PairProof { proof = BLAKE3-derive-key("vutric/1 2026-11-18 pairing link proof", x ‖ s) }` | `ticket`, when the proof matches its current secret |
| mDNS, a typed address or a short code | `PairCommit` (step 1 above) | `code`, when both users confirm |
| a link without `s`, or nothing | anything else | nothing: a guest, every offer asked about |

The listener waits up to 3 s for that first message (`Session::pair_intent`); a message that is neither proof nor
commitment is kept for the host. A proof that does not match (an old QR) is not trusted.

### LAN beacon ([ADR-0052](decisions/0052-lan-finding-beacon-and-diagnosis.md))

A UDP datagram of exactly 41 bytes, to `224.0.0.167:53319` (multicast) or to one host on port 53319 (probe,
opt-in scan):

| Bytes | Field |
|---|---|
| 0-3 | `VTRC` |
| 4 | version, `1` |
| 5 | kind: `0` announce ("I am here"), `1` query ("who is here?") |
| 6-37 | endpoint id |
| 38-39 | QUIC port, little-endian, never 0 |
| 40 | flags: bit 0 phone; other bits 0 when sent, ignored when read |

A visible device answers a query with a unicast announce to the sender, at most once per source per 500 ms. The
address to dial is the datagram's source IP with the announced port; the key is then checked by QUIC as always.
Any other datagram is dropped unread.

## Streams

| Stream | Direction | Content |
|---|---|---|
| Control (bidirectional, one per connection) | both | length-prefixed messages (below) |
| Data (unidirectional, one per offered file) | sender → receiver | header, file bytes from the offset, BLAKE3 |

### Framing

`u32` little-endian body length, then a postcard-encoded `Message`. Bodies over 128 KiB are a protocol error
detected from the header, before any allocation. Every decoded message is validated (`Message::validate`).

### Messages

| Message | From | Meaning |
|---|---|---|
| `Hello { protocol, device_name, platform }` | both, first | version check and display name |
| `FlingStart { id, file_name, mime, size, blake3, gesture, effect, preview }` | sender | a thrown photo; the receiver starts the effect as soon as it arrives |
| `EffectStarted { id, receiver_micros }` | receiver | first effect frame presented (latency probe) |
| `FlingComplete { id }` | receiver | reserved |
| `FlingFailed { id, reason }` | both | `HashMismatch`, `DiskFull`, `Rejected`, `Cancelled`, `Paused` |
| `Offer { id, first, entries, total_files, total_bytes }` | sender | files on offer, ≤ 256 entries per message |
| `OfferReply { id, accept, have }` | receiver | yes or no; `have[i]` = bytes of entry `i` already held |
| `OfferDone { id, received, failed }` | receiver | one offer's files are in; `failed` = transfer-wide indices |
| `PairCommit { commit }` | dialer | pairing step 1 (above) |
| `PairNonce { nonce }` | listener | pairing step 2 |
| `PairReveal { nonce }` | dialer | pairing step 3 |
| `PairResult { confirmed }` | both | pairing step 5 |
| `PairProof { proof }` | dialer | it read the listener's pairing link (above) |
| `Notice { app, title, text, at_ms }` | phone | a mirrored notification: app and title ≤ 80 characters, text ≤ 400, `app` not empty; shown, never acted on |
| `Clipboard { text }` | either | text for the other device's "copy" prompt, 1 byte to 64 KiB; sent only on a user action, never written to the clipboard without the user |
| `Ring(Start { secs })` / `Ring(Stop)` | either | "ring my phone": ring loudly for 1–60 s; `Stop` ends it, and the ringing device sends `Stop` back when its user found it |

`Notice` and `Clipboard` are taken only from a trusted (paired) device and shown only after control and bidi
characters are removed (`vutric_core::companion`); notices are limited to a burst of 10, then one per 500 ms
([ADR-0065](decisions/0065-companion-messages.md)). One that arrives during a file transfer is kept for the host,
not dropped.

`Ring` is taken only from a trusted device, at most a burst of 3 starts and then one per 20 s per peer; a `Stop`
always passes. The ringing device stops by itself after `secs` ([ADR-0069](decisions/0069-ring-my-phone-and-the-command-line.md)).

`gesture` = `{ speed (px/s ≥ 0), angle (rad, 0 = up, + = clockwise), exit_x (0..=1) }`. `preview` ≤ 96 KiB,
longest side ≤ 512 px, JPEG/WebP/PNG.

`OfferEntry` = `{ path, size, mime }`; `path` is relative and `/`-separated (≤ 1024 bytes, folders kept),
`mime` ≤ 128 bytes.

## Sending files

1. A selection of more than 256 files, or with long names, is split into several `Offer`s with the same `id`;
   `first` is the transfer-wide index of the offer's first entry. The receiver asks its user once, at
   `first == 0`, and accepts the following offers of that transfer without asking.
2. After `OfferReply { accept: true }` the sender opens one data stream per entry, at most 4 at a time:
   `id [16] | index u32 LE | offset u64 LE`, the file's bytes from `offset = have[i]`, then the BLAKE3 hash of the
   **whole** file (32 bytes). Bytes the receiver already holds are hashed by the sender, not sent.
3. After the offer's last data stream, the receiver sends `OfferDone`; then the next offer follows.
4. A file the sender cannot read has its stream reset; the receiver lists it in `failed`.

## Receiving a file

1. Map `path` under the save folder: every component through `filename::sanitize`, `.`, `..` and empty
   components dropped, at most 32 levels. Two entries that map to one name get distinct part files.
2. Write to `<target>.vutric-part`, truncated to `have[i]`, while hashing the held prefix and every new byte.
3. On hash match, pick a free name (`filename::unique`: `photo (2).jpg`) and rename into place. A file only ever
   appears complete and verified.
4. On mismatch delete the part file (a retry starts from 0) and list the entry in `OfferDone.failed`.
5. A part file left by an interrupted transfer is resumed when the same path is offered again; its prefix is
   still covered by the final hash, so a stale part cannot slip through.

The peer-supplied path is hostile input: path components, Windows device names (`CON`, `NUL`, `COM1`…),
reserved characters and trailing dots/spaces are neutralised; see the tests in `crates/vutric-core/src/filename.rs`
and `crates/vutric-net/src/transfer.rs`.

## Latency budget (finger release → first effect frame)

| Step | Budget |
|---|---|
| Phone: gesture end → `FlingStart` + preview written | 10 ms |
| LAN one-way (QUIC, warm connection) | 5–30 ms |
| Desktop: decode preview + upload texture | 10 ms |
| Desktop: first frame from warm overlay | ≤ 16 ms (measured 0.5–10.7 ms) |
| **Total** | **< 100 ms** |
