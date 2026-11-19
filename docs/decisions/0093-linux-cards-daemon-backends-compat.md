# ADR-0093: Linux cards, the daemon's Linux backends, first run, and the compat adapter on Linux

- Status: proposed. Code is in `crates/vutric-host-linux` (`cards`, `link`, `desktop`, `onboarding`, `paths`,
  `fonts`) with unit tests; written without a local build (owner's working mode of 2026-11-19), so CI is the first
  compile. The daemon side is a patch for W2 in `docs/streams/w4-notes.md` §1; nothing here ran on a real desktop.
- Date: 2026-11-19
- Stream: W4. Backlog: D10; roadmap Phase 2 ("first-run checklist that asks once to enable the extension"),
  Phase 4 (Linux × iOS); C1/C3/C4 Linux rows.
- Builds on: ADR-0090 (overlay process, GNOME extension), ADR-0091 (session), ADR-0092 (packaging), W2's ADR-0031
  (daemon, warm on connect) and ADR-0032 (focus-free CPU cards), W3's ADR-0054/0056/0062 (compat adapter).

## 1. Cards: the same CPU raster as Windows, shown by `vutric-surface` or by the extension

W2's cards are drawn on the CPU by `vutric_overlay::card::render` into BGRA plus button rectangles (ADR-0032).
Linux keeps that model unchanged and only changes who puts the pixels on screen:

- layer-shell desktops: `cards::to_surface` → `ToSurface::ShowCard`; `vutric-surface` shows it on a second
  layer surface with `keyboard_interactivity = none` and an input region only over the card, and answers
  `FromSurface::Clicked { id }`;
- GNOME: `cards::to_gnome` → the extension's `ShowCard(w, h, scale, bgra, hits)`, drawn as a `St.Widget` with a
  `Clutter.Image` in the Shell's chrome layer (never focused), answering `CardClicked(id)`.

`cards::LinuxCard` checks the buffer before it crosses a process boundary (empty, longer than 4096 px, wrong
length, scale outside 1..=4). Button ids are the three stable strings `accept` / `decline` / `close`
(`cards::button_id` maps W2's `Button` debug names), so neither the surface nor the extension depends on W2's types.

Fonts: `fonts::ui_fonts()` picks the desktop's own UI font (Adwaita Sans, Ubuntu Sans, Cantarell, Noto Sans,
DejaVu, in that order). W2's `Fonts::system` looks for DejaVu only; on Fedora that means the embedded Latin-only
font and boxes for other scripts (w4-notes §1.7).

## 2. The daemon's Linux backends live in `vutric-host-linux`

The daemon (W2) stays platform-neutral; its Linux branch calls four things:

| Need | Call | Why here |
|---|---|---|
| overlay or not, and how | `desktop::plan(&Session, &Extension)` → `LayerShell` / `Gnome` / `GnomeNeedsExtension` / `NotificationsOnly` | one decision table, tested without a desktop |
| the surface process | `link::SurfaceLink::start(Launch::Spawn(path) | Launch::Gnome, bus, 20 s)` | socket in `$XDG_RUNTIME_DIR/vutric/surface-<pid>.sock`, length-prefixed postcard; Drop sends `Quit` |
| tray | `desktop::TrayHandle::start` → SNI, else the extension's panel menu (`SetStatus` + `MenuActivated`), else none | Ubuntu's AppIndicator and Plasma speak SNI; plain GNOME has no tray |
| autostart | `desktop::autostart_via(systemd)` → the user unit (ADR-0091) or `~/.config/autostart`, never both | |

On GNOME the daemon must keep the D-Bus connection that asked for the surface: the extension kills the surface
when that bus name goes away (found in CI, ADR-0090 "CI limits").

**First run** (`onboarding`). Asked once per install; the daemon keeps the `asked` flag:

| Plan | First-run notification |
|---|---|
| `GnomeNeedsExtension(Missing)` | "Effektlar uchun GNOME kengaytmasini yoqing" with "Kengaytmani yoqish" / "Keyinroq"; the button calls `gnome::enable` (`org.gnome.Shell.Extensions.EnableExtension`), and when Shell has not loaded it (installed in this session) a second note asks for a new login |
| `GnomeNeedsExtension(TooOld)` | "update the extension", no button (the update comes from the package or extensions.gnome.org) |
| X11, or no layer-shell | one note that files arrive without the effect |
| layer-shell, GNOME ready, no display | nothing |

The settings window keeps the same "turn on" button for later; the question itself is never repeated.

## 3. Where things go on Linux (`paths`)

- received files: `XDG_DOWNLOAD_DIR` from `user-dirs.dirs` (localised: `~/Yuklanmalar`), then `Vutric/`; partial
  files in `Vutric/.incoming`, so a half file never shows up in the folder people open. W3's
  `transfer::downloads_dir` hard-codes `~/Downloads` (w4-notes §2.1).
- config `$XDG_CONFIG_HOME/vutric`, cache `$XDG_CACHE_HOME/vutric`, compat identity in the data dir.
- device name: `PRETTY_HOSTNAME` from `/etc/machine-info`, else the hostname.
- helper binaries (`vutric-surface`, `vutric-compat`, `vutric-desktop`): next to the daemon, then
  `/usr/libexec/vutric`, `/usr/lib/vutric`, `~/.local/libexec/vutric`.

## 4. Quick Share, LocalSend and KDE Connect on Linux

Nothing in the compat engines is Linux-specific, and W4 does not touch them. What Linux adds:

- the `.deb` ships `vutric-compat` in `/usr/libexec/vutric/`; `Adapter::spawn` already sets `no_new_privs`,
  `PDEATHSIG` and no core dumps on Linux. There is no memory cap on Linux yet (Windows: 512 MiB job object);
  suggested `RLIMIT_AS` patch for W3 in w4-notes §2.2.
- mDNS: every Ubuntu and Fedora desktop runs `avahi-daemon` on UDP 5353. CI job `compat` runs the engine tests with
  Avahi up and checks with `avahi-browse` that Avahi sees the Quick Share advert (`_FC9F5ED42C8A._tcp`), which is
  what a phone's share sheet relies on. Registering through Avahi's D-Bus API instead of our own responder is
  not needed as long as that check passes.
- firewall: Ubuntu has `ufw` off by default; Fedora Workstation's default zone allows 1025-65535. For stricter
  firewalld zones the package installs `firewalld/vutric.xml` (5353 udp, 53317 tcp+udp, 53319 udp,
  1714-1764 tcp+udp). Quick Share and Vutric's own transport use ephemeral ports announced over mDNS, which a
  fixed service cannot list: in a strict zone they need the network marked trusted. `ufw` users:
  `sudo ufw allow 53317,1714:1764/tcp && sudo ufw allow 5353,53317,53319,1714:1764/udp`.
- KDE Connect on Plasma: the real KDE Connect daemon already holds 1716 udp. Vutric's C4 mode must not start
  there by default (checklist row 34; a decision for W3/W2's settings, listed as an owner question below).

## 5. The Slint windows (ADR-0030) on Linux: packaging only

W4 packs W2's windows binary (`vutric-desktop`) into `/usr/libexec/vutric/` when it is built and adds Slint's
Linux runtime libraries to `Depends` (`libfontconfig1`, `libxkbcommon0`, `libegl1`). Until the binary exists, CI job
`slint-m1` (non-blocking) builds W2's M1 spike from `w2/desktop` and runs it under headless sway with both
`winit-femtovg` and `winit-software`, recording the M1 log (first frame, fps, p99).

## Not decided here (owner questions)

1. Plasma: Vutric's KDE Connect mode off by default when `kdeconnectd` runs? (W4 suggests yes.)
2. Should the `.deb` enable the firewalld service on install (`firewall-cmd --reload` from `postinst`)? W4
   suggests no: Fedora installs from RPM/Flatpak anyway, and changing a firewall from a package surprises people.
3. Compat identity location: `$XDG_DATA_HOME/vutric/compat` (W4) or the same folder W3 uses on Windows?

## Consequences

- Linux shows the same cards as Windows, pixel for pixel, with one more process hop.
- The daemon's Linux branch is about 200 lines of glue (w4-notes §1), everything else is tested in W4's crate.
- Not verifiable in CI: GNOME frames after the dock retype (ADR-0090), real notifications and tray hosts, a real
  phone over Quick Share. All of it is in `docs/checklists/linux-desktop.md` and `linux-ios.md` as `[~]`.
