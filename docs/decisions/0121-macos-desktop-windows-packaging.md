# ADR-0121: The Slint desktop windows on macOS: packaging and how the agent opens them

- Status: accepted for implementation (agent side and packaging). W2's `vutric-app` crate does not exist yet, so
  nothing here has run; the agent's Settings item stays disabled until the binary is bundled.
- Date: 2026-11-19
- Stream: W5 (macOS). Follows [ADR-0030](0030-desktop-ui-toolkit.md) (W2: desktop windows are Slint with the
  software renderer, a separate process that exits on close), which replaced the provisional Flutter desktop
  windows of ADR-0009. Builds on ADR-0111 (agent), ADR-0119 (history), ADR-0120 (entitlements).

## Decision

1. **One more helper in the bundle.** `build-app.sh` builds W2's `vutric-app` as a universal binary into
   `Vutric.app/Contents/MacOS/vutric-app` as soon as `crates/vutric-app` is on the branch. It is signed like the
   other helpers: hardened runtime, no entitlements (checked by `check-entitlements.sh`).
   - Because it lives inside the bundle, `Bundle.main` resolves to Vutric.app: the Dock shows Vutric's name and
     icon while a window is open. Its Local Network and file access are attributed to Vutric (ADR-0120 §5).
2. **How the agent opens it** (`apps/macos/Agent/Sources/WindowLink.swift`):
   - command: `vutric-app --page settings|history|devices`;
   - environment: `VUTRIC_IPC` = the daemon socket the agent uses, and `SLINT_BACKEND=winit-software` unless
     already set (ADR-0030);
   - while the process runs, a second request brings it forward (`NSRunningApplication.activate`) instead of
     starting another one. TEMP: switching to another page needs a single-instance channel in `vutric-app` (W2).
   - The process exits when its window closes (ADR-0009 §4), and the agent terminates it when the agent quits.
3. **Menu.** "Sozlamalar…" opens `settings`, and "Butun tarix…" at the bottom of the history submenu opens
   `history`. While `vutric-app` is not bundled, "Sozlamalar…" is disabled with the tooltip "Sozlamalar oynasi
   keyingi versiyada", and "Butun tarix…" is not shown. The short history list in the menu (ADR-0119) stays either
   way.
4. **TEMP stand-in, opt-in only.** `WINDOWS_SPIKE=1 build-app.sh` bundles W2's M1 settings spike
   (`apps/desktop/m1-spike/slint`, its own workspace and lock file) as `vutric-app`, so the owner can try the
   path on a real Mac. CI never does this, because W2 keeps the spike out of CI. The spike ignores `--page`.

## Asked of W2 (`docs/streams/w5-notes.md`)

- `vutric-app` accepts `--page <settings|history|devices>` and `VUTRIC_IPC`.
- On macOS, run with the accessory activation policy and activate on open. The window then takes the keyboard,
  and no second menu bar or Dock icon remains after the window closes.
- A single-instance channel, so a second `--page` switches the open window instead of being ignored.

## Real-Mac checklist (owner)

- [ ] Build with `WINDOWS_SPIKE=1` (until `vutric-app` exists): "Sozlamalar…" opens the window in under 400 ms
      with typing focus; the process exits on close; Activity Monitor shows about 20 MB.
- [ ] Choosing "Sozlamalar…" again while it is open brings the window forward and does not open a second one.
- [ ] Quitting the agent closes the window.
