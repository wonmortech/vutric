# Linux packaging (W4, ADR-0092)

| Path | Installed to | What |
|---|---|---|
| `systemd/vutric.service` | `/usr/lib/systemd/user/` | the daemon as a user service; enabled per user by the autostart switch |
| `applications/io.wonmor.Vutric.desktop` | `/usr/share/applications/` | launcher |
| `metainfo/io.wonmor.Vutric.metainfo.xml` | `/usr/share/metainfo/` | AppStream (GNOME Software, Discover) |
| `icons/*.svg` | `/usr/share/icons/hicolor/{scalable,symbolic}/apps/` | app, paused and tray icons |
| `kio/vutric-send.desktop` | `/usr/share/kio/servicemenus/` | Dolphin "Vutric bilan yuborish" |
| `nautilus/vutric-send.sh` | `/usr/share/vutric/nautilus/` (template) | Nautilus script, copied per user by the daemon |
| `../../apps/gnome-extension/vutric@wonmor.io` | `/usr/share/gnome-shell/extensions/` | GNOME extension (the user still turns it on once) |
| `vutric-surface` (built) | `/usr/libexec/vutric/` | the GPU / surface process |
| `vutric-daemon` (built, W2) | `/usr/bin/` | the daemon |
| `ci/*.sh` | not installed | headless sway and GNOME runs for CI |

Build: `cargo build --release -p vutric-host-linux --bin vutric-surface && packaging/linux/build-deb.sh`. CI does
this in `.github/workflows/linux-desktop.yml` (job `deb`), runs `lintian` on the result and uploads the `.deb`.

## PPA notes

A Launchpad PPA builds from a **source package** on Launchpad's builders. For Vutric that means:

1. **Toolchain.** `rust-toolchain.toml` pins 1.96; Ubuntu 24.04 ships rustc 1.75 (plus versioned `rustc-1.8x`
   packages in noble-updates). The source package must build-depend on a versioned rustc ≥ the workspace's
   `rust-version` (1.90) or the PPA must carry a rustc backport. Until then a PPA cannot build us.
2. **No network on builders.** `cargo vendor` into the source tarball (`debian/cargo-checksum` style or a
   `.cargo/config.toml` source replacement), about 300 MB unpacked today; Launchpad's limit per upload is fine, but
   every update re-uploads it.
3. **Signing.** One GPG key for the PPA, kept with the other release keys (owner decides where; not in this repo).
4. **Series.** noble (24.04) first, then the next LTS; one upload per series.

The cheaper route is our own signed APT repository that serves the `.deb` CI builds. That needs no Launchpad toolchain work. Owner question in `docs/owner-questions.md`.

## extensions.gnome.org submission checklist

Checked against the review guidelines on gjs.guide ("Review Guidelines") as understood today; `[~]` = done in
code, not yet seen by a reviewer.

- [~] All objects, signals and D-Bus exports are created in `enable()` and destroyed in `disable()`; nothing runs at
      import time.
- [~] No deprecated modules (`imports.*`, `ExtensionUtils`); ES modules only, shell-version 46-50.
- [~] No binaries in the zip. The extension launches `vutric-surface` from fixed system paths only
      (`/usr/libexec/vutric`, `/usr/lib/vutric`, `~/.local/libexec/vutric`); the description says the Vutric app is
      required. Spawning a subprocess is allowed when it is needed for the extension's purpose; be ready to explain
      why (ADR-0090 §2: Mutter only lets the launching client retype its window).
- [~] The D-Bus API validates every argument (socket path, card size) before use.
- [~] No telemetry, no network access.
- [~] Logging: one line per adopted surface window. Reviewers may ask to drop it; it is how CI proves adoption.
- [~] Licence: the repository is Apache-2.0. EGO requires a licence compatible with GNOME Shell's GPL-2.0-or-later;
      Apache-2.0 is compatible through GPL-3.0. Confirm with the reviewers, or dual-license the extension folder
      (owner question).
- [~] `metadata.json`: uuid `vutric@wonmor.io` (the domain is ours), `url` to the repo, no `settings-schema` yet.
- [ ] Screenshot of the panel menu and a card (needs a real GNOME session).
- [ ] Upload the zip from the `gnome-extension` CI artifact on https://extensions.gnome.org/upload/ with the
      owner's EGO account.

## Flatpak

Considered in ADR-0092 and left for later: the GNOME extension cannot ship inside a Flatpak, the extension would
have to launch the sandboxed surface as a trusted Wayland client through `flatpak run` (not known to pass the
client socket), and the systemd unit and file-manager entries live outside the sandbox.
