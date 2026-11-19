#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
# SPDX-License-Identifier: Apache-2.0
#
# Builds vutric_<version>_<arch>.deb from release binaries with plain dpkg-deb (ADR-0092): no packaging tool to
# trust or pin. Run after `cargo build --release` of vutric-surface (and of vutric-daemon once it is in the tree).
# Usage: packaging/linux/build-deb.sh [OUT_DIR]
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
out=${1:-$root/target/deb}
bin=$root/target/release
pkg=$root/packaging/linux
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -n 1)
arch=$(dpkg --print-architecture)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

[ -x "$bin/vutric-surface" ] || { echo "build vutric-surface first"; exit 1; }

install -Dm0755 "$bin/vutric-surface" "$stage/usr/libexec/vutric/vutric-surface"
if [ -x "$bin/vutric-daemon" ]; then
    install -Dm0755 "$bin/vutric-daemon" "$stage/usr/bin/vutric-daemon"
    has_daemon=1
else
    echo "note: vutric-daemon not built (W2 branch not merged yet); packaging the Linux pieces only"
    has_daemon=0
fi
# Optional helpers, packed when built: the compat adapter process (LocalSend, Quick Share, KDE Connect,
# magic-wormhole; ADR-0054/0056, ADR-0093 §4) and the Slint settings/pairing windows (W2, ADR-0030).
extra_depends=""
for helper in vutric-compat vutric-desktop; do
    if [ -x "$bin/$helper" ]; then
        install -Dm0755 "$bin/$helper" "$stage/usr/libexec/vutric/$helper"
    else
        echo "note: $helper not built; not packaged"
    fi
done
if [ -x "$stage/usr/libexec/vutric/vutric-desktop" ]; then
    # Slint's winit + femtovg backend on Linux.
    extra_depends=", libfontconfig1, libxkbcommon0, libegl1"
fi
for f in "$stage"/usr/libexec/vutric/* "$stage"/usr/bin/*; do
    if [ -f "$f" ]; then strip --strip-unneeded "$f"; fi
done
install -Dm0644 "$pkg/firewalld/vutric.xml" "$stage/usr/lib/firewalld/services/vutric.xml"
install -Dm0644 "$pkg/systemd/vutric.service" "$stage/usr/lib/systemd/user/vutric.service"
install -Dm0644 "$pkg/applications/io.wonmor.Vutric.desktop" "$stage/usr/share/applications/io.wonmor.Vutric.desktop"
install -Dm0644 "$pkg/metainfo/io.wonmor.Vutric.metainfo.xml" "$stage/usr/share/metainfo/io.wonmor.Vutric.metainfo.xml"
install -Dm0644 "$pkg/kio/vutric-send.desktop" "$stage/usr/share/kio/servicemenus/vutric-send.desktop"
install -Dm0755 "$pkg/nautilus/vutric-send.sh" "$stage/usr/share/vutric/nautilus/vutric-send.sh"
for icon in io.wonmor.Vutric io.wonmor.Vutric-paused; do
    install -Dm0644 "$pkg/icons/$icon.svg" "$stage/usr/share/icons/hicolor/scalable/apps/$icon.svg"
    install -Dm0644 "$pkg/icons/$icon-symbolic.svg" "$stage/usr/share/icons/hicolor/symbolic/apps/$icon-symbolic.svg"
done
ext=vutric@wonmor.io
mkdir -p "$stage/usr/share/gnome-shell/extensions"
cp -r "$root/apps/gnome-extension/$ext" "$stage/usr/share/gnome-shell/extensions/$ext"
find "$stage/usr/share/gnome-shell" -type d -exec chmod 0755 {} + -o -type f -exec chmod 0644 {} +
install -Dm0644 "$pkg/debian/copyright" "$stage/usr/share/doc/vutric/copyright"
# Native package (no Debian revision), so the changelog is changelog.gz, not changelog.Debian.gz.
gzip -9n -c "$pkg/debian/changelog" > "$stage/usr/share/doc/vutric/changelog.gz"
chmod 0644 "$stage/usr/share/doc/vutric/changelog.gz"

size=$(du -sk "$stage/usr" | cut -f1)
mkdir -p "$stage/DEBIAN"
(cd "$stage" && find usr -type f -print0 | sort -z | xargs -0 md5sum) > "$stage/DEBIAN/md5sums"
{
    echo "Package: vutric"
    echo "Version: $version"
    echo "Architecture: $arch"
    echo "Maintainer: Vutric maintainers <283333996+xnosir@users.noreply.github.com>"
    echo "Installed-Size: $size"
    echo "Depends: libc6 (>= 2.35), libwayland-client0, libvulkan1, dbus-user-session | default-dbus-session-bus$extra_depends"
    echo "Recommends: mesa-vulkan-drivers | vulkan-icd, xdg-utils, avahi-daemon"
    echo "Section: net"
    echo "Priority: optional"
    echo "Homepage: https://github.com/wonmortech/vutric"
    echo "Description: receive photos and files flung from your phone"
    echo " Vutric receives photos, videos and files thrown from an Android phone on the"
    echo " same network and lands them on the desktop with a GPU effect. Works on GNOME"
    echo " (with the bundled Shell extension) and on KDE Plasma, COSMIC and wlroots"
    echo " desktops through layer-shell."
    [ "$has_daemon" = 1 ] || echo " This build carries the Linux surface and session files only."
} > "$stage/DEBIAN/control"

mkdir -p "$out"
deb="$out/vutric_${version}_${arch}.deb"
dpkg-deb --root-owner-group --build -Zxz "$stage" "$deb"
dpkg-deb --info "$deb"
dpkg-deb --contents "$deb"
echo "$deb"
