# ADR-0051: Every parser that sees peer or file input is fuzzed, on stable and with libFuzzer

- Status: accepted, implemented (backlog S4)
- Date: 2026-11-18
- Builds on: [ADR-0007](0007-every-route-and-storage-settings.md) part 2 ("the framing and offer parser are fuzzed
  in CI with a protocol-aware fuzzer"), [ADR-0010](0010-interop-every-app.md) rule 4 (a fuzzer per adapter).

## Decision

1. **`fuzz/`** is a cargo-fuzz crate in its own workspace, so nightly flags never reach the main build.
   Each target is a plain function in `fuzz/src/lib.rs`; `fuzz_targets/<name>.rs` only hands bytes to it.

   | Target | Input it stands for | Property checked (not only "no panic") |
   |---|---|---|
   | `frame_stream` | a hostile control stream, split at input-chosen points | decoder buffers ≤ one frame + one read; every decoded message round-trips |
   | `message` | structure-aware `Message` (`arbitrary` derive, feature `vutric-core/arbitrary`) | sender validation and receiver decoding agree; accepted messages arrive unchanged |
   | `pairing_ticket` | a scanned QR or pasted link | name ≤ 64 chars without control characters, ≤ 8 hints, `https` relay; own link parses back equal and is ASCII |
   | `save_target` | offered path, MIME type, peer name, date, user rules | result stays under the save folder; every component is a valid name on Windows, Linux and macOS |
   | `filename` | a peer-supplied file name | valid on every desktop OS, length cap, `unique()` keeps it valid |
   | `settings_json`, `trust_json` | an edited or damaged settings or trust file | `migrate()` clamps; no malformed key survives; re-serialising is stable |

   Every new adapter (`vutric-compat-*`) adds its frame parser here in the same PR.

2. **Three ways to run the same checks.**
   - `cargo test --release --no-default-features` in `fuzz/` (stable, no C++ toolchain): 20 000 random inputs
     per target plus valid frames with bit flips. CI job `Fuzz / smoke` on every push and PR.
   - `cargo +nightly fuzz run <target>` (libFuzzer + AddressSanitizer). CI job `Fuzz / libfuzzer`: 60 s per target
     on PRs that touch the protocol, 600 s nightly, any length by hand. The corpus is cached between runs; a crash
     uploads its input.
   - Replay a crash file with the same command and the file path.

3. Windows: libFuzzer does not link with `-s none` on MSVC (unresolved `__start___sancov_pcs`). Local libFuzzer
   runs use Linux (CI, WSL or a container); the stable smoke run works on Windows.

## Found on the first run

The stable smoke run found two real bugs in `vutric_core::filename::sanitize`, both fixed with regression tests:

1. Cutting a long extension to 16 characters could leave trailing spaces (`a.b<20 spaces>c` became
   `a.b<15 spaces>`), which Windows silently drops, so the saved name differs from the one we checked.
2. `com1 .tar.gz` was not treated as the COM1 device; Win32 ignores spaces before the dot. The reserved list now
   also has `COM0`, `LPT0`, `COM¹–³`, `LPT¹–³`, `CONIN$` and `CONOUT$`.

## Consequences

- `vutric-core` gains an optional `arbitrary` feature; default builds and W1/W2 are unaffected.
- `Decoder::buffered()` is public, for the buffer-bound check.
- The libFuzzer numbers (executions per second, coverage) are recorded from the first CI run in the W3 report.
