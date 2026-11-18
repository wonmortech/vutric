# Workstreams: three sessions in parallel, without stepping on each other

From 2026-11-18 the work runs in three parallel streams, one AI session each. Each stream owns files exclusively.
[backlog.md](backlog.md) drives everything a stream does, and the stream ticks its own rows there in the same
commit as the work.

Owner's quality bar is the best engineering in every respect:

- the fastest, the lightest, the most functional and the most flexible;
- the best UI/UX and design;
- long-term and wide in scope.

No performance claim ships without a measurement, and the measurement is written into an ADR or a report.

Working mode, also from the owner: the sessions work **without stopping** until every item of their stream is
done. An open question for the owner never blocks a stream. It goes into
[owner-questions.md](owner-questions.md), one line each, and the stream moves on to the next item.

## Ownership

| Stream | Scope | Owns (only this stream edits) | Backlog rows |
|---|---|---|---|
| **W1 · Phone** | Flutter app rebuilt on the Claude Design handoff (`design/claude-design/`), improved where it falls short; Kotlin modules; later the Swift modules | `apps/mobile-spike/**`, `crates/vutric-mobile/**`, `design/**` | A1–A14, I1–I10, T1/T3 phone UI, F1/F2/F8/F9/F19 phone parts, effects on the phone |
| **W2 · Desktop** | Rust daemon (tray, toast, IPC, autostart), overlay cards, Halqa and the other effects, Explorer menu, installer; desktop settings UI after the M1 spike; Linux hosts later | `crates/vutric-overlay/**`, `crates/vutric-host-*/**`, new `crates/vutric-daemon/**`, `apps/overlay-spike/**`, new `apps/desktop/**`, new `packaging/**` | D1–D11, E1–E5, M1, T8 |
| **W3 · Network and interop** | routes R1–R10, our own relay, trust store and security, storage engine, every adapter of ADR-0010, fuzzing, interop tests | `crates/vutric-core/**`, `crates/vutric-net/**`, new `crates/vutric-compat-*/**`, new `crates/vutric-relay/**`, `apps/net-spike/**`, new `fuzz/**`, `docs/protocol.md` | R*, C*, S*, T2/T4–T7 engine, M2–M5 |
| **W4 · Linux desktop** | Spike 0.3 and Phase 2 Linux: the layer-shell host (KDE Plasma, wlroots, COSMIC), the GNOME Shell extension and its D-Bus API, the Linux daemon pieces (systemd user unit, StatusNotifierItem, notifications, XDG autostart, Nautilus/Dolphin "Vutric bilan yuborish"), `.deb`/PPA/extensions.gnome.org packaging, Linux CI. Started 2026-11-18. Takes `crates/vutric-host-wlr` and `crates/vutric-host-gnome` out of W2's `crates/vutric-host-*/**`; W2 keeps `vutric-overlay`, `vutric-host-win` and `vutric-daemon`. W5 is macOS, W6 the Android native extras | `crates/vutric-host-wlr/**`, `crates/vutric-host-gnome/**`, `apps/gnome-extension/**`, `packaging/linux/**`, `.github/workflows/linux-*.yml` | D10 (from W2), Linux parts of D1/D2/D5, F-rows on Linux |

**Shared files.** These have rules instead of a single owner:

| File | Rule |
|---|---|
| `Cargo.toml` (workspace) | Append only: one `members` line per new crate, plus new `[workspace.dependencies]` lines. Never reorder or reformat. |
| `Cargo.lock` | Regenerated after a rebase, never hand-edited. |
| `docs/backlog.md` | Each stream edits only its own rows. A new row goes at the end of the right section, tagged `(W1)`, `(W2)` or `(W3)`. |
| `docs/roadmap.md` | Only the "Status" line and rows of your own stream. |
| `docs/owner-questions.md` | Append only, prefixed with the stream tag. |
| `.github/workflows/**` | Add new workflow files, or add a new job. Never rewrite another stream's job. |
| ADR numbers | W1 uses 0011–0029, W2 0030–0049, W3 0050–0069, W4 0090–0109, so numbers never collide. |

## The interface between streams

- **W3 owns the public Rust API** of `vutric-core` and `vutric-net`:
  - W3 changes it **additively** only: new functions, new fields with defaults.
  - A breaking change needs a short ADR, and W3 updates every caller in the same PR.
- **W1 and W2 consume that API.**
  - Sometimes they need something W3 has not built yet. They then open a GitHub issue labelled `needs-w3`.
  - Meanwhile they may build a thin temporary shim inside their own crate, marked `// TEMP until W3 #<issue>`.
  - A stream never edits another stream's files.
- **The phone bridge** (`crates/vutric-mobile`) belongs to W1. It wraps the W3 API for flutter_rust_bridge and,
  later, UniFFI (ADR-0009).
- **The daemon's IPC** (`crates/vutric-daemon`) belongs to W2. The desktop UI talks to the daemon only through it.

## Git

- Base branch: `spike/0.2-network` (PR #6). Once PR #6 is merged, the base becomes `main`.
- Each stream has **its own git worktree and branch**, so no two sessions share a working tree:

  | Stream | Worktree | Branch |
  |---|---|---|
  | W1 | `D:\Projects\wonmortech\vutric` | `w1/phone` |
  | W2 | `D:\Projects\wonmortech\vutric-w2` | `w2/desktop` |
  | W3 | `D:\Projects\wonmortech\vutric-w3` | `w3/net` |
  | W4 | `C:\vutric-w4` | `w4/linux-1` |

- **Commits are small and finished.**
  - Every commit leaves the stream's crates green: `cargo clippy --all-targets -- -D warnings` and the tests, plus
    `flutter analyze` for W1.
  - Push often, so the other streams can rebase.
- Rebase on the base branch at least once a day, and before every PR.
- **PRs.** wonmortech rules:
  - Push and open PRs with the `vutric` account token; the `wonmors` account has Actions off.
  - `xnosir` approves.
  - Merge is squash only.
  - Text is Uzbek Latin or English. No Cyrillic, no AI attribution.
- **Builds on one machine.**
  - Each worktree has its own `target/`.
  - Heavy builds (APK, release, Android NDK) run one at a time. A stream writes `BUILDING <stream> <what>` into
    `D:\Projects\wonmortech\.vutric-build.lock` before a heavy build and deletes it afterwards, and waits while
    another stream's line is there.

## Order inside each stream

**W1 · Phone**

1. Rebuild the phone UI from `design/claude-design/Vutric App v4.dc.html` as Flutter widgets:
   - theme tokens from the handoff README, with the iOS-native default plus the brand theme, light and dark;
   - S1 Home, S2 scanner, S3 Send (all 9 states) and S4 Receive (all 5 states);
   - the gallery-first flow of ADR-0008 kept, and reduced motion respected throughout.
2. The throw gesture uses the handoff numbers:
   - the threshold, 280 ms out and 560 ms back;
   - haptics, and the result pill near the thumb.
3. Onboarding and permissions, then history.
4. Kotlin modules: `ShareEntry`, `TransferJob`, `VutricTile`, `SafStore`, `Nearby` (NSD), `LinkManager` (hotspot).
5. Build the APK and write a device checklist for the owner. Record the results in a W1 ADR.

**W2 · Desktop**

1. M1 spike, which settles the desktop toolkit of ADR-0009:
   - the same settings screen and the Halqa, built in Flutter desktop, Tauri and Slint;
   - LocalSend measured as the reference;
   - RAM, cold-open time and fps recorded.
2. `vutric-daemon`:
   - tray, toast, IPC and autostart;
   - it receives with the storage rules of W3's `Settings`.
3. Overlay cards that never take focus, and the progress ring.
4. Effects: Oddiy (reduced motion), then Kometa and Pufak, then the stack for many files.
5. Explorer menu "Vutric bilan yuborish", dragging onto the Send Zone, MSIX/winget, the firewall rule.

**W3 · Network and interop**

1. Security:
   - S1 trust store;
   - S2 commit-then-reveal;
   - S3 "everyone" for 10 minutes;
   - S4 fuzzing in CI.
2. R1 stronger LAN finding:
   - every address in the QR;
   - probe, scan and multicast;
   - AP-isolation and iOS-denied detection, exposed as API states for the UIs.
3. R2 our own `iroh-relay` (config, rate limit). Also the API that reports the route and speed.
4. C1 LocalSend v2.1 adapter and C2 browser page, then C3 Quick Share (Windows). Each gets a sandboxed parser
   process, a fuzzer and a real-app test.
5. T2–T7 storage engine (free-space guard, parts cleanup, history), then the R3 hotspot core.

## When a stream is "done"

Every backlog row of the stream is in one of two states:

- `[x]`, with its tests or measurements linked;
- `[~]`, with the exact step that is left on a real device, for the owner.
