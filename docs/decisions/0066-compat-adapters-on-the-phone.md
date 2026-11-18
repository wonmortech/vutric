# ADR-0066: The compat adapters on the phone: the same loop, in a separate `:compat` process

Status: accepted (2026-11-18) · Stream: W3 · Backlog: C10 · Issue: #11 (from W1, ADR-0013 decision 9) ·
Builds on: ADR-0010 (rule 4: pre-auth parsing isolated), ADR-0054, ADR-0056, ADR-0062

## Context

ADR-0010 promises LocalSend and KDE Connect inside the Android app. The adapters run today only as the desktop
process `vutric-compat`, started by the daemon and driven over JSON lines on stdin/stdout. An Android app cannot
start a binary of its own as a child process in a supported way, so the phone can only list foreign devices
(W1's `ForeignDiscovery`, ADR-0013 decision 9). W1 proposed an `isolatedProcess` service that parses while the app
process does the sockets.

## Options

| Option | Isolation | Cost | Verdict |
|---|---|---|---|
| A. Parsers in the app process | none: a parser bug reaches the Vutric identity, the trust list and every file | small | breaks ADR-0010 rule 4 |
| B. `isolatedProcess` parses, app process does sockets (W1's proposal) | strongest: no permissions, no files, no network | every adapter split into a socket half and a parser half; HTTP, TLS and UKEY2 parse while they do I/O (the TLS handshake itself is pre-auth parsing), so each request becomes a Binder round trip. A rewrite of four adapters and their tests | too costly now, kept as a later hardening |
| C. The whole compat loop in a separate, non-exported `:compat` process, IPC over a socket pair | a separate address space with nothing of Vutric's in it: no Vutric key, no trust list, no settings; files only through the staging folder | no adapter changes: the loop the desktop runs becomes a library call | **chosen** |

## Decision

1. **The loop is a library.** `vutric_compat_localsend::serve` holds what the `vutric-compat` binary ran:
   - `serve(input, output)`: async, any byte stream; the binary is now `serve(stdin, stdout)`.
   - `serve_blocking(input, output)`: on its own two-thread runtime.
   - `serve_socket_fd(fd)` (Unix, `unsafe`: takes ownership of a connected stream socket) for the Android service.
   The desktop keeps spawning the binary (ADR-0054 limits unchanged).
2. **The app side attaches.** `host::Adapter::attach(reader, writer, start)` and `Adapter::attach_socket_fd(fd,
   start)` give the same `Adapter` as `spawn`: the same `decide`, `send`, `share`, `announce`, new `kde_send`,
   `received_file` (path check) and `stop`. The app process treats the `:compat` process as untrusted exactly as
   the daemon does: lines capped at 1 MiB, closed enums, files moved only through `checked_path`.
3. **Android shape** (W1 builds it, below):
   - a `CompatService` with `android:process=":compat"`, `android:exported="false"`; it loads the Rust library and
     runs `serve_socket_fd` on a thread, and stops itself when that returns;
   - the app process creates `ParcelFileDescriptor.createSocketPair()`, passes one end to the service when it
     binds, keeps the other and calls `Adapter::attach_socket_fd` through the bridge;
   - the `:compat` process gets only `Start`: its own LocalSend and KDE Connect identity files, a staging folder in
     the app cache, ports. It never opens the Vutric identity, `trusted.json` or `settings.json`;
   - it runs only while receive mode or a foreign send is active (ADR-0010 rule 3), so it costs nothing idle.
4. **What isolation remains, said plainly.** The `:compat` process runs under the app's UID, like the desktop
   adapter runs under the user's account. A memory-safety bug in a parser could read the app's private files.
   That is the same boundary the desktop has (separate process, same user); the extras the desktop has (job
   object memory cap) have no Android equivalent that is safe with ART in the process. W1's `isolatedProcess`
   parser for passive LocalSend datagrams stays as it is, and option B stays the next step if a parser bug is
   ever found in the field.
5. **iOS** (LocalSend only, ADR-0010): an iOS app has one process; the loop would run in-process behind the same
   `Adapter::attach` over an in-memory stream. Decided when phase 3 starts; until then iOS has no adapters.

## S5 row (ADR-0061)

| Route | Before authentication | After | Code |
|---|---|---|---|
| LocalSend / KDE Connect on Android (C10) | parsed only in the `:compat` process, same caps and fuzzers as the desktop adapter (`localsend_*`, `kdeconnect_packet`, `localsend_ipc` for what the app reads back) | as ADR-0054 / ADR-0062 | as ADR-0054 / ADR-0062 (deviation listed there) |

## Measured

| Check | Result |
|---|---|
| `crates/vutric-compat-localsend/tests/library_mode.rs::attached_over_an_in_memory_stream`: `serve` on one end of a stream, `Adapter::attach` on the other; a LocalSend sender's 70 KB file arrives through `decide` and `received_file`, then the phone side sends a 50 KB file to a LocalSend receiver (`Sent` event), a path outside staging is refused, `Stop` ends the loop | pass (Windows) |
| `…::first_line_must_be_start` | pass |
| `…::served_on_a_socket_pair_like_the_android_service` (Unix only: `UnixStream::pair`, `serve_socket_fd` on a thread, `attach_socket_fd`) | runs in CI on Ubuntu |
| existing `roundtrip.rs` through the spawned binary (now a thin `serve` caller) | pass |

| `cargo clippy -p vutric-compat-localsend --all-targets --target aarch64-linux-android -- -D warnings` (NDK 28.2 clang for `ring`; compiles the Unix-only code and the test above for Android) | clean |

Not verified: linking it into W1's APK, and the service on a real phone.

## What W1 must change (issue #11)

1. `crates/vutric-mobile/Cargo.toml`: depend on `vutric-compat-localsend` (path `../vutric-compat-localsend`).
2. A JNI entry in `vutric-mobile`, outside flutter_rust_bridge (the service process has no Flutter engine), e.g.
   `Java_..._CompatService_serve(env, class, fd: jint)` that calls `unsafe { serve::serve_socket_fd(fd) }` and
   returns when the loop ends.
3. Kotlin `CompatService` (`android:process=":compat"`, not exported) that receives a `ParcelFileDescriptor` in
   its bind intent or a Binder call, `detachFd()`s it and runs the JNI entry on a background thread; it stops
   itself when the call returns.
4. Bridge calls in the app process: `compat_start(start_json)` creates the socket pair, hands one end to the
   service, and calls `Adapter::attach_socket_fd(other_fd, start)`; events (`Offer`, `PairRequest`, `Received`,
   `Sent`, …) go to Dart as a stream; `compat_decide`, `compat_send` (LocalSend), `compat_kde_send`, `compat_stop`.
   Received files are moved out of staging only through `Adapter::received_file`, then into the gallery/SAF rules.
5. Replace the "sending opens later" rows (`TEMP until W3 #11` in `ForeignDiscovery.kt`) with real sends; offers
   from foreign devices are always asked about unless pinned (ADR-0010 rule 2), and the row shows "LocalSend
   orqali" / "KDE Connect orqali".

## Consequences

- One adapter code base for desktop and phone; a fix or a fuzzer finding lands on both.
- Backlog C1 and C4 gain an Android part that W1's device checklist covers (LocalSend and KDE Connect on the phone
  each way).
