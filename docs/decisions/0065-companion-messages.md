# ADR-0065: Companion messages: mirrored notifications and clipboard text

Status: accepted (2026-11-18) · Stream: W3 · Backlog: F20 (wire for F12, F13, A12) · Issue: #12 (from W1) ·
Builds on: ADR-0009 (`Companion`, `Clipboard` modules), ADR-0050, ADR-0061

## Context

W1 built the phone half of notification mirroring and clipboard sharing (opt-in, off by default, ADR-0013
decision 11) and had no message to carry them: the clipboard went as a `.txt` file and notices stayed on the
phone. Issue #12 asks for two additive `Message` variants with caps, plus helpers so hosts never build them by hand.

## Decision

1. **Wire.** Two variants appended to `Message`, after `PairProof` (ADR-0064), so the postcard indices of every
   older variant stay (test `appended_variants_keep_old_wire_indices`):

   | Message | Caps (checked by `Message::validate` on both ends) |
   |---|---|
   | `Notice(Notice { app, title, text, at_ms })` | `app` 1–80 characters, `title` ≤ 80, `text` ≤ 400 (characters, as W1 cuts them); `at_ms` the phone's clock, display only |
   | `Clipboard { text }` | 1 byte to 64 KiB |

   Both ride the authenticated control stream after `Hello`; nothing is parsed before QUIC has authenticated the
   key, and the 128 KiB frame cap still applies first (ADR-0061).
2. **Sending.** `Session::send_notice(app, title, text, at_ms)` cuts each field to its cap on the sender, so a long
   notification never kills the session; `Session::send_clipboard(text)` refuses empty or oversized text (the UI
   says "too long").
3. **Receiving rules** (`vutric_core::companion`, pure):
   - `accepts(msg, admission)`: only from a paired device (`Admission::Trusted`). A guest in "everyone" mode or in
     the pairing window can send files that are asked about, but not notices or clipboard text.
   - A notice is shown, never acted on: no actions, no links opened, no reply.
   - Clipboard text goes into a prompt ("Nusxalash"); the host never writes the clipboard by itself.
   - Text is displayed only through `shown(raw, max_chars, newlines)`: control characters and bidi overrides
     removed, bounded, trimmed.
   - `NoticeLimit`: a burst of 10, then one per 500 ms; the rest are dropped, so a chatty phone cannot flood the
     desktop.
4. **Not lost during a transfer.** Transfers read the control stream themselves. A `Notice` or `Clipboard` that
   arrives meanwhile is now kept in the session's queue (32 at most, oldest dropped) and returned by the host's
   next `recv`, instead of being skipped as before.
5. **Images on the clipboard** are not in this ADR: they would be a third variant with its own caps, appended
   later. Until then an image goes as a file.

## S5 row (ADR-0061)

| Route | Before authentication | After | Code |
|---|---|---|---|
| Companion messages (Vutric session) | nothing new: they cannot be sent before QUIC and `Hello` | same TLS stream, bounded by `validate`; taken only from trusted devices | the device was paired by QR or six digits |

## Measured

| Check | Result |
|---|---|
| `message.rs` `companion_bounds` (80/400 multi-byte characters, 64 KiB, empty refused) and `appended_variants_keep_old_wire_indices` | pass |
| `companion.rs` `only_paired_devices_and_only_companion_messages`, `shown_text_is_clean_and_short`, `burst_then_steady_rate` | pass |
| `crates/vutric-net/tests/pair_intent.rs::neither_keeps_the_first_message_and_companion_messages_flow`: a 1000-character text arrives cut to 400; clipboard arrives; empty clipboard refused on the sender | pass |
| `…::a_notice_during_a_transfer_is_kept_for_the_host`: the receiver sends a notice before its `OfferReply`; the sender's `transfer::send` keeps it and the next `recv` returns it | pass |
| new fuzz target `companion` (round trip, caps, `shown` never returns control or bidi characters) | smoke run pass; libFuzzer in CI |

## What W1 must change (issue #12)

- Clipboard: replace the `Bufer <date>.txt` file send (the `TEMP until W3 #12` note in `companion.dart`) with a
  bridge call that runs `Session::send_clipboard(text)`; keep the 64 KiB cap check in the UI.
- Notifications: drain the `CompanionListener` queue into `Session::send_notice(app, title, text, at_ms)` while a
  session to a trusted device is open; send nothing to a guest.
- Receiving on the phone (desktop to phone clipboard): in the session loop, a `Message::Clipboard` from a trusted
  peer opens the copy prompt, text through `companion::shown` for the preview; anything `companion::accepts`
  refuses is dropped.
- The owner question about the default allowlist (every app or empty) stays W1's.

## Consequences

- W2's daemon shows notices with `NoticeLimit` and `shown`, and offers clipboard text in a toast with "Nusxalash".
- A phone running an older build sees these as decode errors only if it is sent one; a sender should send them
  only to peers on this build (all spike builds move together).
