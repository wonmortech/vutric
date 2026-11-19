# ADR-0075: Phone as mouse and keyboard

Status: accepted (2026-11-19) · Stream: W3 · Backlog: F15 · Builds on: ADR-0069 (ring, companion rules)

## Context

A paired phone is already on a fast, authenticated session with the desktop. Using it as a touchpad and keyboard
(a presentation clicker, a couch with a TV laptop, typing a long password from the phone's password manager) needs
only a message and the desktop's input injection. That makes it the most powerful companion message: anyone who
can send it can type on the desktop. So the protocol side has to be strict.

## Decision

1. **`Message::Pointer(Pointer)`**, appended at wire index 17 (test `appended_variants_keep_old_wire_indices`).
   `vutric_core::pointer::Pointer` is one of:
   - `Move { dx, dy }`, a relative move, at most ±2000 per message;
   - `Scroll { dx, dy }`, where 120 is one notch;
   - `Button { Left | Right | Middle | Back | Forward, down }`;
   - `Key { usage, down }`, a USB HID usage id on page 0x07, `0x04 ..= 0xE7`, with modifiers at 0xE0–0xE7. It is
     independent of the keyboard layout; the host maps it to the OS key code;
   - `Text { text }`, at most 256 bytes, with no control characters except tab and newline, so no terminal
     escape sequences;
   - `ReleaseAll`.

   `Message::validate` refuses anything out of range.
2. **Who may send it.** `pointer::accepts` takes it only from `Admission::Trusted`. The desktop host (W2) also
   needs:
   - its own switch, "Telefondan boshqarish", off by default;
   - a visible indicator while input comes from a phone;
   - to never inject into a secure desktop (the Windows UAC/lock screen, the macOS secure input field), which the
     OS blocks anyway.
3. **Rate.** `PointerLimit` (one per peer) allows 250 events/s with bursts of 500, and `Text` costs 25 tokens.
   **Releases always pass**: button up, key up and `ReleaseAll`. Dropping a press is harmless; dropping a release
   would leave a key held. The host also releases everything when the session ends. `Pointer::coalesce` merges
   moves and scrolls when the host falls behind. The sender should send at about 120 Hz, not per touch event.
4. **Transport.** It uses the same control stream as the other companion messages; `Session::pointer(p)` sends
   one. During a transfer, pointer messages are kept with the other companion messages (`keep_companion`).
   Latency over the relay is not hidden; the badge already shows the route.

## Measured (written as code, not run in this round)

| Test | What it checks |
|---|---|
| `pointer::tests::fields_are_bounded` | step limits incl. `i16::MIN`, usage range, text length and control characters |
| `pointer::tests::moves_and_scrolls_coalesce_and_clamp` | merging, clamping, no merge across kinds |
| `pointer::tests::only_trusted_devices_and_releases_always_pass` | admission, burst of 500, releases at an empty bucket, refill, text cost |
| `message::tests::appended_variants_keep_old_wire_indices` | index 17; an invalid key usage is refused by `validate` |
| fuzz `message` (existing) | decodes and validates every `Message`, now including `Pointer` |

**Not verified:** injection on any OS, which is W2's work, and the feel of the latency over LAN or relay.

## Consequences

- W2: input injection (`SendInput` / `CGEventPost` / uinput or the XDG RemoteDesktop portal) behind the switch.
  W1: the touchpad and keyboard screen. Both are listed in `docs/streams/w3-notes.md`.
