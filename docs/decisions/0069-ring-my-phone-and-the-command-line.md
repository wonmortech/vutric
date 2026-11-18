# ADR-0069: "Ring my phone" and the desktop command line

Status: accepted (2026-11-18) · Stream: W3 · Backlog: F14 (wire and engine), F10 (desktop half) ·
Builds on: ADR-0050 (trust), ADR-0064 (pairing link secret), ADR-0065 (companion messages)

## Context

KDE Connect's "Ring my phone" is one of the features people miss most when they leave it (ST §9, backlog F14):
the phone is somewhere in the flat, on silent, and the desktop makes it ring. Vutric had no message for it.

Backlog F10 asks for automation. On the phone that is Siri/Shortcuts (I3, W1). On the desktop, power users and
scripts want a command line that drives everything the app does. `vutric-net-spike` already sent files and
latency probes; it could not send clipboard text, notices or a ring, it trusted nobody, and it printed for people
only.

## Decision

### 1. The wire: `Message::Ring` (F14)

- One variant appended after `Clipboard`, so every older postcard index stays (wire index 15, test
  `appended_variants_keep_old_wire_indices`). It carries `vutric_core::ring::Ring`:

  | Ring | Meaning | Check (`Message::validate`, both ends) |
  |---|---|---|
  | `Start { secs }` | ring loudly for `secs` seconds; the ringing device stops by itself afterwards | `secs` in 1..=60 (`MAX_RING_SECS`) |
  | `Stop` | either way: the sender says "never mind", the ringing device says "found it" | — |

- `Ring::start(secs)` clamps; `DEFAULT_RING_SECS` = 30.
- Rules (`vutric_core::ring`, pure, the host passes admission and clock):
  - `accepts(msg, admission)`: only from a paired device. A guest in "everyone" mode or in the pairing window can
    never make a phone ring.
  - `RingLimit`, one per peer: a burst of 3 starts, then one per 20 s. A `Stop` always passes.
- `Session::ring(secs)` and `Session::stop_ring()`. A ring that arrives during a file transfer is kept for the
  host's next `recv`, like notices (ADR-0065 part 4).
- **Not in the wire:** the volume, the sound and "on silent". Those are the ringing device's choice. Android may
  ring at alarm volume through `AudioAttributes.USAGE_ALARM`, which plays in silent mode but not under every
  Do Not Disturb policy. iOS cannot override the silent switch without the Critical Alerts entitlement, which Apple
  grants only for health and safety apps, so the iPhone rings at the ringer volume (W1 issue).

### 2. The command line (F10, desktop half)

`vutric-net-spike` becomes the desktop command line for scripts. New and changed commands:

| Command | What it does |
|---|---|
| `receive [--dir DIR] [--json] [--yes]` | shows the QR **with a one-time secret** (ADR-0064) for a 10-minute pairing window, then only paired devices get in; one task per device; saves files from paired devices (`--yes`: guests too), prints clipboard text and notices, rings the terminal bell on a ring; a six-digit pairing is confirmed at the terminal |
| `ring [--seconds N] [--stop]` | rings the target, waits until it says "found" (exit 0) or the time is up (exit 3) |
| `clipboard (TEXT... \| -)` | text to the target's copy prompt; `-` reads standard input |
| `notice --app APP [--title T] (TEXT... \| -)` | one notice, as a phone mirrors it |
| `send PATH...` | the same as `files PATH...` (without paths, `send` still runs the latency probes) |
| `devices [--json]` | the devices this command line has paired with |

- Every connecting command takes `--ticket LINK` or `--discover`. A link with a secret is proven
  (`Session::prove_link`) and the target is stored as `ticket` in the command line's own trust file,
  `net-spike-trusted.json` next to the settings. It is not the daemon's `trusted.json`, because the command line
  has its own key (`net-spike.key`).
- `--json`: one JSON object per line on standard output (`{"event": "clipboard", "from": ..., "text": ...}`).
  Status, the QR and the link go to standard error, so a script reads only events.
- The receiving rules are the engine's: `companion::accepts`, `ring::accepts`, `NoticeLimit`, `RingLimit`,
  `companion::shown` before printing. A guest's companion message is reported as `dropped`.
- The command line never writes the clipboard; printing the text is its "prompt". A script pipes it where it wants.

## S5 row (ADR-0061)

| Route | Before authentication | After | Code |
|---|---|---|---|
| Ring (Vutric session) | nothing new: it cannot be sent before QUIC and `Hello` | same TLS stream, bounded by `validate`; taken only from trusted devices, rate-limited | the device was paired by QR or six digits |

## Measured

| Check | Result |
|---|---|
| `vutric-core` `ring::tests` (clamp, validity, trusted only, burst 3 then one per 20 s, stop always passes) and `message::tests::ring_bounds`, `appended_variants_keep_old_wire_indices` | pass |
| `crates/vutric-net/tests/ring.rs`: four rings of 500 s arrive clamped to 60 s, the fourth over the limit, the stop passes, "found" comes back; a guest's ring is not accepted; a ring during a transfer is kept | 3 pass (0.2 s) |
| `apps/net-spike/tests/cli.rs`: `receive --json` in one process; `clipboard`, `notice` (bidi override removed), `ring --stop`, `send FILE` from another; first link pairs both, `devices --json` lists the receiver as `Ticket`; an unanswered `ring --seconds 1` exits 3 | 2 pass (7.5 s) |
| fuzz target `companion` extended with `Ring` (round trip, range, never accepted from a guest) | stable smoke pass; libFuzzer in CI |

## What W1 and W2 do (issues)

- W1: on `Message::Ring` from a trusted peer, play a loud sound and vibrate (Android: `USAGE_ALARM`, full-screen
  intent or heads-up notification with "Topdim"; iOS: ringer volume, local notification). Stop on `Ring::Stop`,
  after `secs`, or when the user taps "Topdim", which sends `Session::stop_ring()` back. A "Telefonni jiringlatish"
  button for desktops.
- W2: a "Telefonni topish" tray item and device-page button that call `Session::ring`, a toast "jiringlayapti..." that
  turns into "topildi" on the returned `Stop`; the desktop rings too when a phone rings it.

## Consequences

- A phone on an older build sees `Ring` as a decode error only if it is sent one; spike builds move together.
- Scripts get a stable surface now (`--json` events, exit codes). When W2's daemon lands, the same commands can
  talk to it over its IPC instead of opening their own endpoint; the command names stay.
