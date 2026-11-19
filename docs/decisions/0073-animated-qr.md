# ADR-0073: Animated QR, our own fountain code over standard QR

Status: accepted (2026-11-19) · Stream: W3 · Backlog: R9 · Builds on: ADR-0064 (pairing link), ADR-0071 (BLE)

## Context

One QR code holds a pairing link easily. Two cases need more:

- a small file (a contact, a note, a key backup) sent between devices that have no network and no Bluetooth;
- a pairing link carrying several hotspot credentials and relay hints.

Backlog R9 asks for research and a decision between libcimbar and TXQR, and a prototype only if the licence is
clean.

## Options (licences checked on GitHub, 2026-11-19; no source read)

| | libcimbar (`sz3/libcimbar`) | TXQR (`divan/txqr`) | Own format over standard QR |
|---|---|---|---|
| Licence | MPL-2.0 (file-level copyleft) | MIT | ours (Apache-2.0) |
| Language | C++, with OpenCV for decoding | Go | Rust in `vutric-core` |
| Codes | its own colour symbols (not QR) | QR frames with fountain coding | QR frames with fountain coding |
| Activity | pushed 2026-11-18, about 6.5k stars | pushed 2024-04, about 3.3k stars | |
| Scanner | its own decoder (OpenCV on the phone) | any QR scanner | any QR scanner (ML Kit, Apple Vision, desktop `rqrr`-class decoders) |

- **libcimbar** is the fastest by far, as its project reports, because it is not QR. But it needs its own camera
  decoder, written in C++ with OpenCV. That costs megabytes in the Android and iOS apps, and a native build in every
  platform's CI. MPL-2.0 would allow it if its files stay unmodified and separate, but we would carry a C++
  toolchain for one feature. **Not now.** It is worth a second look only if animated transfer becomes a main route.
- **TXQR** shows that fountain-coded frames over plain QR work with a phone camera. Its code is Go and cannot be
  used in our Rust core as it is. Its licence is clean, but nothing of it is copied or read: only its published
  idea is used, which is also older public work (LT codes, Luby 2002).
- **Decision:** a small Vutric format, `vutric_core::qrstream`, over standard QR codes. Every platform already
  has a QR scanner, so the receiver needs no new native code. The prototype is built (below).

## Decision: the format

```text
frame: "VQ" | 1 | blocks u16 | block_bytes u16 | total_len u32 | seed u32 | digest 8 | data (block_bytes)
```

- **Systematic first.** Seeds `0..blocks` carry the plain blocks. A camera that sees the whole first loop is done
  after `blocks` frames.
- **Fountain after that.** Every later seed XORs a set of blocks taken from a fixed integer degree table (mostly
  2 and 3, a few of 8 to 32) with a SplitMix64 generator seeded by `(seed, blocks)`. There is no floating point,
  so every platform derives the same set. Changing the generator would be a new version byte.
- **Peeling decoder.** It accepts frames in any order, with gaps and repeats. It keeps at most 4 pending
  equations per block. `progress()` feeds a progress bar.
- **Integrity.** `digest` (BLAKE3, 8 bytes) ties frames to one payload. A frame of another payload is refused
  (the camera moved), and the joined payload must match the digest.
- **Bounds.** Payload at most 1 MiB, at most 4096 blocks, blocks of 16 to 2048 bytes. The suggested block is 512
  bytes.
- **Not authenticated by itself**, like every unauthenticated channel in Vutric. The payload is a pairing link
  (checked by QUIC) or a file whose offer the receiver still accepts. An attacker who can show frames to the
  camera could as well show a single QR.
- **Text vs binary QR.** Frames are bytes. If a platform scanner turns out to mangle byte-mode QR, the app wraps
  each frame in Base45 (RFC 9285, QR alphanumeric mode) without changing this format. That is decided on a real
  phone.

## Measured (written as code, not run in this round)

| Test | What it checks |
|---|---|
| `qrstream::tests::whole_loop_without_loss_is_just_the_blocks` | 5000 bytes in 10 frames, exactly 10 seen |
| `qrstream::tests::any_start_and_heavy_loss_still_finish` | 1 B to 300 KB, start at frame 0 to 1000, 0–50 % of frames lost: done within 4 × blocks + 40 frames |
| `qrstream::tests::indices_are_stable_distinct_and_in_range`, `bad_frames_and_mixed_payloads_are_refused`, `a_damaged_frame_fails_the_digest` | determinism, header checks, version, another payload, digest |
| fuzz `qr_stream` | decode never panics, accepted frames re-encode byte for byte, decoder stays bounded |

**Not verified:** a real camera. The frame rate and the block size a phone reads reliably (the 512-byte
suggestion is a guess to measure) were not tested. Neither was scanning on Android ML Kit, iOS Vision, or a
desktop webcam. Rendering the loop and scanning it are the apps' job (W1/W2). The speed of this format against
TXQR's published numbers was not compared.

## Consequences

- W1/W2 can show `Encoder::frame(seed)` at a few frames per second and feed scanned bytes to `Decoder::push`. A
  `needs-w1`/`needs-w2` note goes in `docs/streams/w3-notes.md` when the UI schedules it.
- libcimbar stays a possible later route for big files without a network. That would be a separate ADR, with the
  OpenCV size measured.
