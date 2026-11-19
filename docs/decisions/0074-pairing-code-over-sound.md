# ADR-0074: Pairing code over sound

Status: accepted (2026-11-19) · Stream: W3 · Backlog: R9a · Builds on: ADR-0064 (link secret, six digits),
ADR-0073 (animated QR)

## Context

Pairing shows a QR code. Sometimes the camera cannot see the screen: a TV across the room, a laptop lid at a bad
angle, a user who cannot aim a phone. Every device has a speaker and a microphone. ggwave (MIT, C++, about 7.9k
stars, pushed 2026-04) shows that a few bytes over sound work well. Its licence is clean, but it is C++, so it
would mean a native build on every platform for a few bytes. The codec needed is small enough to write ourselves.

## Decision

1. **`vutric_core::sound`, pure Rust.** It is written from textbook signal processing: FSK symbols and a Goertzel
   filter per tone. No ggwave or other modem code was read or copied.
   - **Tones:** 16 tones, `1875 + n × 62.5 Hz` (1875 to 2812.5 Hz), one nibble per symbol. That band is audible.
     It plays through any laptop speaker and is heard by any phone microphone without special hardware.
   - **Symbols:** 50 ms each (20 per second), with 5 ms raised-cosine edges so there are no clicks. The detector
     uses the middle 80 % of the window, so ±10 % of timing error is harmless.
   - **Frame:** preamble `15 0 15 0`, then a length byte, the payload (at most 32 bytes) and a CRC-8 (poly 0x07).
     Six digits plus a 4-byte secret take `4 + 2 + 2×10 + 2 = 28` symbols, 1.4 s.
   - **Decoder:**
     - It scans the recording in eighth-symbol steps and checks only the first tone before testing the whole
       preamble.
     - It aligns on the best of 8 nearby offsets.
     - It ignores gain, which is argmax of tone power, and accepts any leading silence.
     - It looks at most 30 s of input, at sample rates of 8 to 96 kHz.
2. **What goes over the air.** The same short secret that would be on the screen: the six digits, or the link's
   one-time secret, never the identity. Sound is a broadcast to the room, like a QR code on a screen. Everyone in
   earshot can hear it, so it is only as private as the room. The user still confirms the six digits on both
   devices (ADR-0064) whenever the secret came by sound.
3. **Where it fits.** A "Tovush bilan" button next to the QR: the desktop plays, the phone listens for up to
   10 s. Playing and recording belong to W1/W2.
   - Ultrasonic tones (above 18 kHz) are a possible version 2, once real devices show that their speakers and
     microphones handle it. Many phone microphones cut off there.

## Measured (written as code, not run in this round)

| Test | What it checks |
|---|---|
| `sound::tests::round_trip_at_common_rates` | 8, 16, 44.1 and 48 kHz: the length in samples and the payload back |
| `sound::tests::heard_after_silence_quietly_and_through_noise` | leading noise or silence of up to 31 000 samples, gain 0.05, white noise up to 0.2 against a 0.5 tone |
| `sound::tests::nothing_is_heard_in_silence_noise_or_a_broken_frame` | silence, loud noise, one lost symbol (CRC), refused payload sizes and rates |
| fuzz `sound_code` | any rate and any 16-bit samples: no panic, never more than 32 bytes |

**Not verified:** a real speaker and microphone. Room echo, phone noise suppression (Android `VOICE_RECOGNITION`
vs `MIC` source, iOS voice processing) and the distance it works at were not tested. The tone band was chosen on
paper.

## Consequences

- W1 needs a microphone permission prompt, worded as "only while you pair". W2 plays the samples. That goes in
  `docs/streams/w3-notes.md` as a cross-stream need when the UI schedules it.
- If echo turns out to be the problem, the fix is longer symbols, a decision for version 2.
