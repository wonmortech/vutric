# Linux desktop: real-device checklist (W4, ADR-0090..0092)

Nothing here is `[x]` until it has run on the named real machine. `[~]` = built and, where possible, exercised in
CI on a headless compositor with software rendering; the step left is the real-device run in the last column.

Machines:

- **U46** Ubuntu 24.04 LTS, GNOME 46, Wayland session (default "Ubuntu" session).
- **K6** Kubuntu 25.10 or Fedora KDE, Plasma 6, Wayland.
- **F50** Fedora 43+ Workstation, GNOME 50.
- One Intel iGPU machine at least; AMD and NVIDIA (proprietary driver) when available.

Prepare on each: install `vutric_*.deb` (U46, K6 on Kubuntu) or copy `vutric-surface` to `~/.local/libexec/vutric/`
and the extension folder to `~/.local/share/gnome-shell/extensions/` (F50). Log out and in once on GNOME.

## Overlay (spike 0.3)

| # | Check | U46 | K6 | F50 | What CI already showed / what is left |
|---|---|---|---|---|---|
| 1 | `vutric-surface --self-test 5 --cards 50 --out st.json` exits 0 and prints 5 runs | [~] | [~] | [~] | sway headless (K6 path) and GNOME 46 headless (U46 path) in CI; run on the machine, attach `st.json` |
| 2 | ≥ 59 fps, p99 ≤ 20 ms, 0 hitches during the effect (1080p and 1440p) | [~] | [~] | [~] | CI is software Vulkan, numbers not meaningful; measure on Intel, AMD, NVIDIA |
| 3 | Type in a text editor during the self-test: no keystroke lost, caret never leaves the editor | [~] | [~] | [~] | layer: `keyboard_interactivity = none`; GNOME: dock + empty input region. Verify by hand |
| 4 | Click and drag on the desktop/an app **through** the effect while it plays | [~] | [~] | [~] | empty input region set on both surfaces; verify by hand |
| 5 | Effect window never appears in Alt+Tab, the overview, the dock or the taskbar | [~] | [~] | [~] | layer surfaces are never listed; GNOME `hide_from_window_list` (46: client API, 50: window API) |
| 6 | Transparent: the desktop is visible through the portal glow, no black rectangle | [~] | [~] | [~] | premultiplied alpha required by the code, else it refuses to start |
| 7 | Effect above a maximised window; behaviour over a fullscreen video/game noted | [~] | [~] | [~] | layer `overlay` vs GNOME dock + above; record what happens |
| 8 | Two monitors, different scale (100 % + 200 %, and 125 % fractional on K6): effect on the chosen monitor, sharp | [~] | [~] | [~] | integer `wl_output` scale only (ADR-0090 consequences) |
| 9 | First frame after `Play` (warm) < 16 ms; cold GPU creation time | [~] | [~] | [~] | `surface_driver` prints both; CI numbers are software |

## Cards, tray, notifications (ADR-0091)

| # | Check | U46 | K6 | F50 | Left |
|---|---|---|---|---|---|
| 10 | Card at bottom-right, sharp at 100 % and 200 % | [~] | [~] | [~] | K6: layer surface; GNOME: Shell chrome (`St.ImageContent`, the 46 vs 47+ `set_bytes` signature is feature-tried) |
| 11 | Click "Qabul qilish" while typing in another app: the click arrives, the caret stays | [~] | [~] | [~] | by hand |
| 12 | Tray icon with the four rows; left click opens Vutric; paused icon greys out | [~] | [~] | [~] | K6: SNI (D-Bus test in CI); U46: AppIndicator extension or our panel menu; F50: our panel menu |
| 13 | Restart plasmashell: the tray icon comes back | – | [~] | – | re-registration on watcher restart is in code |
| 14 | "photo.jpg keldi" notification with Ochish / Papkada ko'rsatish; both work | [~] | [~] | [~] | D-Bus test against a stand-in server only |
| 15 | Papkada ko'rsatish selects the file in Nautilus / Dolphin | [~] | [~] | [~] | `FileManager1.ShowItems` |

## Session (ADR-0091)

| # | Check | U46 | K6 | F50 | Left |
|---|---|---|---|---|---|
| 16 | Autostart on: `systemctl --user is-enabled vutric` = enabled, daemon runs after re-login | [~] | [~] | [~] | needs W2's daemon wiring (`needs-w2`) |
| 17 | `kill -SEGV` the daemon: back within 2 s (`Restart=on-failure`) | [~] | [~] | [~] | needs the daemon |
| 18 | Nautilus: right click → Skriptlar → "Vutric bilan yuborish" sends the file | [~] | – | [~] | needs `vutric-daemon send` (W2, D5) |
| 19 | Dolphin: right click → "Vutric bilan yuborish" | – | [~] | – | same |
| 20 | GNOME without the extension: one notification asking to turn it on, files still received | [~] | – | [~] | needs the daemon |

## Packaging (ADR-0092)

| # | Check | U46 | K6 | F50 | Left |
|---|---|---|---|---|---|
| 21 | `sudo apt install ./vutric_*.deb`, then remove: no leftovers outside `$HOME` | [~] | [~] | – | CI installs and removes on the runner |
| 22 | GNOME Software / Discover show the AppStream entry with the icon | [~] | [~] | – | by hand |
| 23 | Extension from extensions.gnome.org (after review) works the same as the bundled one | [~] | – | [~] | not submitted yet |
