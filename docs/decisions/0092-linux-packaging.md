# ADR-0092: Linux packaging: a plain dpkg-deb package first, extensions.gnome.org for the extension, Flatpak later

- Status: accepted; `packaging/linux/build-deb.sh` builds the package in CI (job `deb`), `lintian` runs on it.
  Not installed on a real machine yet.
- Date: 2026-11-18
- Stream: W4. Backlog: D10 (`.deb` + PPA, extensions.gnome.org).

## Decision

1. **`.deb` built with `dpkg-deb` from a staging tree** (`packaging/linux/build-deb.sh`). No cargo-deb or nfpm:
   the package carries two binaries from two crates (W2's `vutric-daemon`, W4's `vutric-surface`) plus data files
   from several folders, which a per-crate tool models badly, and a 60-line script has nothing to pin or audit.
   Contents and paths: [`../../packaging/linux/README.md`](../../packaging/linux/README.md).
   - `vutric-surface` goes to `/usr/libexec/vutric/` (allowed by Debian policy since 4.6; one of the extension's
     three fixed search paths).
   - The GNOME extension is installed system-wide (`/usr/share/gnome-shell/extensions/`), so a `.deb` user only
     has to switch it on once. The same folder is the extensions.gnome.org upload.
   - `Depends: libwayland-client0, libvulkan1` (both loaded at run time: `wayland-backend` uses `dlopen`, wgpu loads
     Vulkan), a session bus; `Recommends` a Vulkan driver and `xdg-utils`.
   - Until W2's daemon is in the tree the package holds the Linux pieces only and says so in its description.
2. **Distribution:** our own signed APT repository fed by CI is the recommended route; a Launchpad PPA is possible but
   needs a rustc ≥ 1.90 on the builders and a vendored source tarball (README "PPA notes"). Owner decides.
3. **extensions.gnome.org** for the extension, from the CI artifact, with the checklist in the packaging README.
4. **Fedora** (GNOME 50 target of the checklist): an `.rpm` comes later from the same staging tree (`rpmbuild` with
   the same file list); the overlay itself does not depend on the distribution.
5. **Flatpak: not in Phase 2.** Rejected for now because the extension cannot be inside it, the trusted-client spawn
   through `flatpak run` is unproven, and the unit and file-manager entries are host files. Revisit with a real
   machine: a Flatpak for KDE/wlroots users only (layer-shell works inside the sandbox) is plausible.

## Consequences

- One `.deb` per architecture (amd64 first; arm64 when CI builds it).
- Updating the extension on EGO and in the `.deb` are separate releases; the D-Bus `API` number lets the daemon tell
  the user when the extension is too old (`gnome::judge`).
