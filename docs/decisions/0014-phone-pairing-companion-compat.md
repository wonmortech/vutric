# ADR-0014: The phone on W3's answers to #11, #12, #13: pairing intent, companion messages, compat process

- Status: accepted, implemented in `apps/mobile-spike` and `crates/vutric-mobile` (stream W1). `flutter analyze
  --fatal-infos` is clean, `flutter test` and `cargo test -p vutric-mobile` pass, the release APK builds. Nothing
  has been tried on a real phone yet.
- Date: 2026-11-18
- Builds on: [ADR-0013](0013-phone-on-the-w3-api.md) (its decisions 9, 10 and 11 were TEMP shims; this ADR
  replaces them). Uses W3's [ADR-0064](0064-listener-knows-how-the-dialer-pairs.md),
  [0065](0065-companion-messages.md), [0066](0066-compat-adapters-on-the-phone.md),
  [0067](0067-magic-wormhole-adapter.md), [0068](0068-captive-portals-big-files-and-cable-hints.md).
- Backlog: A12, A13, F12, F13, N1; the phone parts of S8, R11, F20, C1, C4, C7, C10, R1a, Rb.

## Context

ADR-0013 left three `TEMP until W3 #n` shims: the listener trusted any stranger in its pairing window (#13), the
clipboard went as a `.txt` file and notices were never sent (#12), and LocalSend / KDE Connect devices were only
listed (#11). W3 answered all three (PR #15). Their rows for captive portals (R1a), big files through the relay
(Rb) and magic-wormhole (C7) also needed screens.

## Decision

| # | Decision | Why | Where |
|---|---|---|---|
| 1 | **The QR carries a one-time secret (S8, ADR-0064).** Opening the pairing window (`set_visibility("pairing")`) makes a fresh 16-byte `LinkSecret`; `current_link()` and `hotspot_link()` then return the link with `pairing::with_secret`. Closing the window drops it, so an old screenshot of the QR proves nothing. `start()` returns the plain link. The phone's QR page draws the QR only after the window is open and the link refreshed | A QR on screen is the only thing that proves the dialer was in the room | `api.rs` (`secret_slot`, `window_secret`, `shown_link`), `MyQrPage`, `hotspot_page.dart` |
| 2 | **Dialing proves the secret.** A link dial uses `Peer::connect_link` (LAN) or `connect` + `prove_link` (internet-only route), so a desktop or phone that shows our QR knows we read it | Same rule both ways | `connect_link`, `run_link` |
| 3 | **The listener asks how the dialer pairs.** After `accept_with`, an untrusted session waits up to `PAIR_INTENT_WAIT` (3 s) for `Session::pair_intent`: `Link` → trusted as `ticket` at once; `Code` → `Connected.needs_check`, the phone opens `PairCheckPage` (six digits, "Bir xil" / "Farq qiladi") and keeps the session only when both said yes; `WrongLink` → closed and logged, the listener waits for the next one; `Neither` → a guest: not trusted, its file offers are asked about, its notices and clipboard are dropped. Phone ↔ phone by short code now works instead of timing out | ADR-0064; no stranger is trusted without proof (the W3 answer to ADR-0013's owner question) | `accept_session`, `admit_peer`, `pairing_of`; `Session.connect(verify:)`, `checkDigits`, `PairRefused` |
| 4 | **Companion messages (A12, F12, F13, F20, ADR-0065).** Clipboard is `Session::send_clipboard` (64 KiB, empty refused); notices are `send_notice` (app, title, text, time). Both only on a trusted session; a guest gets `companion.untrusted` and the "Buferni yuborish" button is hidden for it. Incoming notices and clipboard text come only from a trusted peer, pass `companion::accepts`, a per-peer `NoticeLimit`, and `companion::shown` (80 / 80 / 400 characters, control characters out); the phone shows a notice as a snackbar (or a heads-up notification when in the background) and clipboard text as a sheet with "Nusxa olish". The `.txt` workaround is gone | The engine owns the caps and the trust rule; the UI only words it | `api.rs` (`companion_incoming`, `send_clipboard`, `send_notice`), `companion.dart`, `handleIncoming` |
| 5 | **The Companion allowlist starts empty, and empty mirrors nothing.** Sozlamalar → "Qaysi ilovalar" lists launcher apps (`<queries>` for MAIN/LAUNCHER, label and package), with search and checkboxes; `CompanionListener` forwards a notice only when its package is on the list. The owner question from ADR-0013 (mirror everything, or start empty?) is unanswered, so the safer choice is the default | A notice can hold one-time codes and private messages; nothing should leave the phone without a choice made per app | `app_picker_page.dart`, `Companion.kt`, `MainActivity.installedApps`, manifest, owner question |
| 6 | **Compat adapters in a `:compat` process (C10, C1, C4, ADR-0066).** `CompatService` is a non-exported service in `android:process=":compat"` (same UID, separate address space). `CompatHost` makes `ParcelFileDescriptor.createSocketPair()`, binds the service and hands one end over in a single Binder transaction (`writeFileDescriptor`; intents cannot carry descriptors); the service detaches it and calls `CompatNative.serve(fd)` on its own thread with a multicast lock. That JNI entry (no `jni` crate, `catch_unwind` so a panic never unwinds into the JVM) runs `serve::serve_socket_fd`. The app keeps the other end and passes the raw descriptor to Rust's `compat_start`, which runs `Adapter::attach_socket_fd`. The `:compat` process never opens Vutric's identity, `trusted.json` or `settings.json`: it gets only its own identity files (`compat/` in app data) and a staging folder in the cache | ADR-0010: bytes from unauthenticated foreign apps are parsed outside the process that holds our keys | `CompatService.kt`, `compat_service.rs`, `api.rs` (`compat_*`), `MainActivity` (`compatOpen` / `compatClose`) |
| 7 | **"Boshqa ilovalar" screen.** Opened from the "Yaqindagi boshqa ilovalar" list in the connect sheet and from Sozlamalar; the adapters run while it is open. It lists LocalSend and KDE Connect devices the adapters see ("LocalSend orqali" / "KDE Connect orqali"); a tap picks files and sends (`compat_send` / `compat_kde_send`), answered by `sent` / `send_failed`. Every foreign offer and every KDE Connect pair request is asked about (name, count, size, app; "Qabul qilish" / "Rad etish"). A `received` path is checked again in the app process with `host::checked_path` before Dart sees it; finished files move from staging into Vutric's folder (and the gallery, by the settings). KDE Connect sends only to a device that paired from its own app first (the adapter only answers pairing). Passive `ForeignDiscovery` stays for the connect sheet; both bind UDP 53317 with address reuse | One screen owns the foreign session and its questions; foreign apps never land files silently | `foreign_page.dart`, `compat.dart`, `ForeignList` |
| 8 | **magic-wormhole (C7, ADR-0067).** On the same screen, "Kod bilan yuborish" (one file; the code is shown to read out, never logged) and "Kod bilan qabul qilish" (type the code; the file arrives as an offer and is asked about). Labelled as going over the internet through public servers, and hidden unless the route may use the internet (`auto`, `internet`); Rust refuses it otherwise with `wormhole.lan_only` | Same rule as the privacy line: never show a way that breaks the chosen route | `ForeignPage`, `compat_wormhole_*`, `wormholeAllowed` |
| 9 | **Why a connect failed (R1, R1a, ADR-0052, ADR-0068).** After a link dial fails (not a refusal), `diagnose_link` probes the link's addresses with the beacon (1.5 s), checks for a Wi-Fi login page against our own relay's `/generate_204` (never a third party, only when the route allows the internet), and returns `lan::diagnose`'s key and next routes. The failure screen then says "Bu Wi-Fi avval kirish sahifasini talab qiladi" with the login URL when the network named one, or the other reasons, and the routes to try next | The user learns what to fix instead of "could not connect" | `diagnose_link`, `diagnosisText`, `ConnectingPage` |
| 10 | **Big batches through the relay (Rb, ADR-0068).** Before a send the phone sums the sizes and asks `bulk_verdict`: over 2 GiB on a relayed path asks ("Relay orqali 3.22 GB": hotspot or cable is faster, "Baribir yuborish"); over 10 GiB is refused with the same advice. If the path still is relayed when the send starts, Rust's own guard refuses with `bulk.too_big` | Never push tens of GB through the relay | `bulk_verdict`, `bulkAllowed`, `BulkDialog` |
| 11 | **`catch.rs` is no longer flaky.** Its silent peer (scenario 3) lived a fixed 8 s, racing the 5 s round-trip timeout plus a slow CI connect; it now stays up until the test signals it is done. New scenarios: phone ↔ phone by code, an old QR's secret refused then admitted as a guest, companion both ways with caps, a stranger refused and the secret gone after the window closes | Fix the cause, not the timeout | `crates/vutric-mobile/tests/catch.rs` |

## Verified

- `flutter analyze --fatal-infos`: no issues. `flutter test`: 71 tests pass. New: `test/companion_compat_test.dart`
  (clipboard sheet, captive-portal notice, both big-batch dialogs, wormhole code dialog and entry, the app picker,
  in all 9 themes at 320 dp with text 2x, no overflow; 48 dp labelled tap targets; the picker's empty-list line,
  search and save; every new key has Uzbek), and in `test/pure_test.dart` clipboard caps in bytes, the paused
  marker, the diagnosis text, compat event parsing (caps, control characters, malformed lines) and state,
  wormhole hidden off-internet.
- `cargo clippy -p vutric-mobile --all-targets -- -D warnings` (Windows host) and
  `cargo ndk -t arm64-v8a clippy -p vutric-mobile -- -D warnings` (Android target, the `cfg(unix)` compat code):
  clean. `cargo test -p vutric-mobile`: unit tests plus `tests/catch.rs`, 6.1 s locally (scenario 3 takes
  ~5.4 s, the engine's own 5 s round-trip timeout; every other scenario ~0.1 s).
- `cargo ndk -t arm64-v8a -t x86_64 --platform 24 ... build -p vutric-mobile --release` then `flutter build apk --release
  --target-platform android-arm64,android-x64` at commit 15acf0e: passed, Kotlin compiled, 86.7 MB (90 940 122 bytes;
  ADR-0013's was 78.2 MB, the difference is the compat adapters in `libvutric_mobile.so`). `llvm-nm -D` shows
  `Java_io_wonmor_vutric_1spike_CompatNative_serve` exported; `aapt2 dump xmltree` shows `CompatService` with
  `exported=false`, `process=":compat"`; `classes.dex` keeps the `io/wonmor/vutric_spike/CompatNative` name.

## Not verified (needs a real phone)

The `:compat` process starting and the Binder descriptor handoff on a device, LocalSend and KDE Connect sending
and receiving against the real apps, KDE Connect pairing from its app, magic-wormhole against the public servers
from a phone, the captive-portal message on a real hotel or guest Wi-Fi, the big-batch dialog on a relay-only
network, the app picker on Android 11+ package visibility, notices and clipboard against the W2 daemon.

## Device checklist (adds to ADR-0013's; replaces its items 25–27)

28. Desktop shows its QR; phone scans it: connected and trusted as "QR orqali" with no digits.
29. Phone A "Mening QR kodim"; phone B scans it with Vutric: trusted on both, no digits. Phone B types A's short code instead: both show the same six digits, "Bir xil" on both → trusted "Raqam bilan".
30. Take a screenshot of phone A's QR, close and reopen "Mening QR kodim", scan the screenshot from phone B: A refuses it (the window's secret changed).
31. Kompyuter bilan bogʻlash → "Buferni yuborish" on → copy text → peer sheet (trusted desktop) → "Buferni yuborish": the desktop offers the text, no `.txt` file. A guest session has no button.
32. Notifications on, "Qaysi ilovalar" with nothing ticked: a chat notification arrives, nothing reaches the desktop. Tick the chat app: the next one shows on the desktop with title and text cut to 80 / 400 characters.
33. The desktop sends clipboard text: a sheet "… bufer matnini yubordi" with "Nusxa olish"; with Vutric in the background, a heads-up notification instead.
34. LocalSend on a laptop, same Wi-Fi: Sozlamalar → Boshqa ilovalar lists it "LocalSend orqali"; tap → pick a file → it arrives in LocalSend; send from LocalSend to the phone: the phone asks, "Qabul qilish" → the file is in Vutric's folder (and the gallery for a photo).
35. KDE Connect on another device: pair it from KDE Connect → the phone asks with the code → "Juftlash"; then a tap sends a file to it.
36. Route "Avtomatik": "Kod bilan yuborish" shows a code; `wormhole receive <code>` on a laptop gets the file. Route "Faqat shu tarmoq": the wormhole rows are gone.
37. Hotel or guest Wi-Fi with a login page, scan a desktop QR before signing in: the failure says "Bu Wi-Fi avval kirish sahifasini talab qiladi" (with the login URL when known).
38. On a relayed session pick more than 2 GiB: the relay question appears; more than 10 GiB: refused with the hotspot advice.

## Consequences

- No `TEMP until W3 #n` note is left in W1 code.
- The compat adapters run only while "Boshqa ilovalar" is open: a LocalSend sender elsewhere sees the phone only
  then. Running them in the background would need a foreground service and its notification; not done.
- KDE Connect pairing can be started only from the KDE Connect side (the adapter answers, it does not ask).
