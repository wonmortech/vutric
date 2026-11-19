# ADR-0117: LocalSend and Quick Share on macOS in the foreground: a bridge the menu-bar agent starts

- Status: accepted for implementation (`crates/vutric-host-mac/src/compat.rs`, bin `vutric-mac-compat`,
  `apps/macos/Agent/Sources/CompatLink.swift`); unit tests and a start-up smoke test on CI; no real LocalSend or
  Android device has talked to it yet.
- Date: 2026-11-18
- Stream: W5 (macOS). Rows C1 (LocalSend) and C3 (Quick Share), macOS column. Uses W3's adapter unchanged:
  [ADR-0054](0054-localsend-adapter.md), [ADR-0056](0056-quick-share-receiver.md),
  [ADR-0066](0066-compat-adapters-on-the-phone.md).

## Context

On Windows the daemon starts the low-privilege `vutric-compat` process and answers its offers. There is no daemon
on macOS yet (W2 #28, #30), but the menu-bar agent runs whenever the user is logged in, and the adapter is an
ordinary child process. "Foreground" here means: while the agent runs and the user has switched it on, with the
agent's own questions and notifications.

## Options

| Option | For | Against |
|---|---|---|
| A. Wait for W2's macOS daemon | one place for every transfer | C1/C3 stay unusable on the Mac for now |
| B. The Swift agent spawns `vutric-compat` and speaks its protocol | no extra binary | re-implements in Swift the defensive reading and the path check `host::Adapter` already has (ADR-0054) |
| C. A small Rust bridge `vutric-mac-compat` uses `host::Adapter`; the agent talks to the bridge | the same spawn limits and `checked_path` as the daemon; every string cleaned in Rust and unit-tested | one more process while the switch is on |

## Decision

**C.**

1. **Bridge.** `vutric-mac-compat --alias <computer name>` starts the adapter with `Adapter::spawn` (no inherited
   handles, cleared environment, no core dumps, killed with the bridge), `Start` = LocalSend on 53317, multicast
   on, Quick Share on, KDE Connect off, staging `~/Library/Caches/io.wonmor.vutric/compat-staging`, identity
   `~/Library/Application Support/Vutric/compat-identity`, 60 s to decide.
2. **Protocol to the agent** (`vutric_host_mac::compat`): JSON lines. Commands `decide{session, accept}`,
   `send{request_id, device, paths}`, `announce`, `stop`; unknown ops are refused. Events `ready`, `devices`,
   `offer{alias, files, bytes, names≤5, code}`, `text{text, link}`, `received{name, path}`, `finished`,
   `cancelled`, `sent`, `send_failed`, `error`. Every string has been through `vutric_core::companion::shown`
   (aliases and names ≤ 80 characters, text ≤ 16 384, no control or bidi characters) before the agent sees it.
3. **Accepting.** The agent asks (alias, file count, first names, size, protocol, Quick Share's four digits). The
   answer is all files or none; the bridge keeps the offer's file ids, so the agent never chooses ids.
4. **Received files** pass `Adapter::received_file` (a plain file directly in staging, ADR-0054) and are moved
   into `~/Downloads`, never over an existing file (`photo (2).jpg`, `vutric_core::filename::unique`). The agent
   shows the usual "received" notification.
5. **Quick Share text and links** are shown in a question with "Nusxalash"; a link is never opened (ADR-0065
   rules applied to this route too).
6. **Sending** goes to LocalSend devices the adapter has seen (the menu "LocalSend qurilmasiga yuborish", files
   picked in an open panel). Sending to Android by Quick Share is not offered: the adapter receives only
   (ADR-0056).
7. **Switch.** Menu item "LocalSend va Quick Share'dan qabul qilish", off by default, remembered. Closing the
   bridge's stdin (switch off, agent quits) stops the adapter. A build without the two helpers shows the item
   disabled.
8. **Local Network.** macOS asks once for the app (the helpers are its children); `NSLocalNetworkUsageDescription`
   and the Quick Share Bonjour type are already in the agent's Info.plist (ADR-0112).
9. When W2's daemon runs on macOS it takes this over (it already hosts the adapter on Windows); the bridge then goes.

## S5 row (ADR-0061)

| Route | Before authentication | After | Code |
|---|---|---|---|
| LocalSend / Quick Share on macOS (C1, C3) | parsed only in `vutric-compat` (unchanged); the bridge reads its lines through `host::Adapter` | strings cleaned in the bridge, files checked and moved by it | LocalSend: the sender's certificate hash; Quick Share: the four digits, shown in the question |

## Measured

| Check | Where | Result |
|---|---|---|
| `compat.rs` tests: offers cleaned (U+202E and newline removed), counted, accepted whole, forgotten when finished; LocalSend-only device list, quiet when unchanged; text cleaned and links marked; unknown ops refused; `place` never overwrites | `cargo test -p vutric-host-mac` on both macOS runners | pass (run 44340716090) |
| The bundled bridge starts the bundled adapter and prints `ready` with its port, then stops on `stop` | `packaging/macos/smoke.sh` | pass (run 44340716090) |
| LocalSend and Quick Share engine tests on macOS | `cargo test -p vutric-compat-localsend -p vutric-compat-quickshare` (round 1) | pass |

## CI evidence

Run [44340716090](https://github.com/wonmortech/vutric/actions/runs/44340716090): the bundle has `vutric-mac-compat` and `vutric-compat`, both universal (x86_64 + arm64); the smoke test
started the bundled bridge with `--port 0`, which printed `{"kind":"ready","port":49203,"quickshare_port":49204}`
and exited 0 on `stop`. The `compat.rs` unit tests passed in the `overlay (macos-15-intel)` job. No offer, transfer
or Quick Share handshake was exercised (no second device on the runner).

## Real-Mac checklist (owner)

- [ ] Switch on; macOS asks for Local Network access once; allow.
- [ ] LocalSend 1.18 on a phone sees the Mac, sends two photos: the question shows the phone's name and two names;
      after "Qabul qilish" both are in Downloads and a notification says so.
- [ ] The Mac sends a file to that phone from "LocalSend qurilmasiga yuborish".
- [ ] Android Quick Share ("Nearby devices"): the Mac appears; the four digits match; a file lands in Downloads;
      a shared link shows the copy question and is not opened.
- [ ] Switch off: `pgrep vutric-compat` finds nothing.
