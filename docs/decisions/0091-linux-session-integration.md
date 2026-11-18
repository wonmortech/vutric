# ADR-0091: Linux session integration: systemd user unit, StatusNotifierItem, freedesktop notifications, file-manager entries

- Status: accepted; code and files in `crates/vutric-host-linux` and `packaging/linux/`, D-Bus parts tested on a
  private session bus in CI. The daemon wiring is W2's (`needs-w2` issue). Nothing verified on a real desktop yet.
- Date: 2026-11-18
- Stream: W4. Backlog: D10 (and the Linux parts of D1, D5).
- Builds on: [ADR-0031](0031-desktop-daemon.md) (tray, toasts, IPC, autostart on Windows), [ADR-0090](0090-linux-overlay-layer-shell-and-gnome.md).

## Decision

| Piece | Linux implementation | Where | Tested |
|---|---|---|---|
| App id | `io.wonmor.Vutric` (reverse DNS of wonmor.io): desktop entry, icons, notification `desktop-entry` hint. The GNOME effect window is `io.wonmor.Vutric.Surface` | `packaging/linux/applications/`, `icons/` | file tests |
| Autostart | **systemd user unit** `vutric.service` (`WantedBy=graphical-session.target`, `PartOf=` the same, `Restart=on-failure`, light hardening). The settings switch runs `systemctl --user enable/disable`. Without a systemd user manager it falls back to W2's XDG autostart entry. The package never enables it for a user | `packaging/linux/systemd/vutric.service`, `files::set_autostart` | unit text, quoting |
| Tray | StatusNotifierItem + `com.canonical.dbusmenu` over zbus: icon, title, tooltip, the four rows of the Windows tray (Telefonni ulash, Sozlamalar, To'xtatib turish / Davom ettirish, Chiqish), left click = `activate`; re-registers when Plasma's watcher restarts | `sni.rs`, `menu.rs` | `tests/dbus.rs`: registration, `GetLayout`, `Event`, `Activate`, `IconName` after a change |
| Tray on GNOME | no SNI host on plain GNOME: `Tray::spawn` fails at `RegisterStatusNotifierItem`, and the daemon calls the extension's `SetStatus`, which shows the same menu in the top bar | `extension.js` `Indicator` | headless GNOME job (SetStatus) |
| Notifications | `org.freedesktop.Notifications`: summary/body from the daemon's `toast.*` keys, actions Ochish / Papkada ko'rsatish plus `default`, `desktop-entry`, `category=transfer.complete`, `image-path` for a single photo; body escaped only when the server announces `body-markup`. "Papkada ko'rsatish" = `org.freedesktop.FileManager1.ShowItems` (Nautilus, Dolphin, Nemo), else `xdg-open` on the folder | `notify.rs` | `tests/dbus.rs` against a stand-in server |
| File managers | Dolphin: KIO service menu installed system-wide by the package (`/usr/share/kio/servicemenus/`), per user (executable, as KF6 requires) otherwise. Nautilus has no system folder for scripts: the daemon installs `~/.local/share/nautilus/scripts/Vutric bilan yuborish` (name localised) once. Both run `vutric-daemon send -- <files>` | `packaging/linux/kio/`, `nautilus/`, `files::install_file_manager_entries` | file tests |
| Copy | Linux-only strings in `vutric-host-linux::i18n` (`linux.*` keys), Uzbek default, English fallback; the extension uses the daemon's `tray.*` keys | `i18n.rs`, `i18n.js` | catalog tests (no Cyrillic, unique keys) |

### Rejected

- **XDG autostart only.** It works, but a crash leaves the user without Vutric until the next login; the unit restarts
  it within 2 s and gives `journalctl --user -u vutric` for support. systemd ≥ 246 also turns XDG entries into units,
  so having both enabled would start the daemon twice; the switch therefore owns exactly one of them.
- **D-Bus activation of the daemon.** Nothing needs to start it on demand; the file-manager entries call
  `vutric-daemon send`, which starts it if needed (second-launch rule of ADR-0031).
- **`libappindicator` / GTK tray, `notify-rust`, `ksni`:** C dependencies, a GTK loop, or a licence outside
  MIT/Apache/BSD (ADR-0090 §4).
- **nautilus-python extension** for a richer menu: not installed by default on Ubuntu or Fedora; the script is.

## Needs from W2 (daemon), filed as a `needs-w2` issue

1. On Linux, use `vutric_host_linux::sni::Tray` instead of `tray_none.rs`, and `notify::show` instead of the WinRT
   toast; on GNOME without a tray host, `gnome::ShellProxy::set_status` and the `MenuActivated` signal.
2. `autostart::set` on Linux calls `vutric_host_linux::files::set_autostart` when `files::systemd_available()`.
3. `vutric-daemon send -- <paths>` plus `ipc::Request::Send { paths }` (D5), used by both file-manager entries.
4. Overlay on Linux: spawn `vutric-surface` (layer-shell) or call `ShellProxy::spawn_surface` (GNOME), and send
   `proto::ToSurface` instead of using the in-process `Gpu`. `examples/surface_driver.rs` is the working reference.
5. Cards on Linux: send the `card::render` raster to `ToSurface::Card` (layer-shell) or `ShellProxy::show_card`
   (GNOME) and map `Clicked` / `CardClicked` ids to `Button`.

## Consequences

- The daemon never needs GTK, Qt or a C D-Bus library on Linux.
- A user without systemd (Alpine, some Gentoo) gets XDG autostart and no automatic restart.
- The Nautilus script appears under "Skriptlar" / "Scripts" in the context menu, one level down; that is the cost of
  needing no extra package.

## Still unverified (real machines)

Tray on Plasma 6 and on Ubuntu's AppIndicator; the notification buttons on GNOME 46/50 and Plasma; ShowItems on
Nautilus 46 and Dolphin 24; the Nautilus script name with an apostrophe-free Uzbek label; `systemctl --user enable`
from inside the session. Rows 10-19 of the checklist.
