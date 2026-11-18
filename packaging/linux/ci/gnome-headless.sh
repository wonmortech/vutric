#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
# SPDX-License-Identifier: Apache-2.0
#
# Loads the Vutric extension into a headless GNOME Shell (the runner's version, 46 on ubuntu-24.04), calls its
# D-Bus API, lets it launch vutric-surface and plays effects through it (ADR-0090 §2). Run inside
# `dbus-run-session`.
set -euo pipefail

out=${1:-linux-gnome}
mkdir -p "$out"
out=$(realpath "$out")
bin=$(realpath target/release)
uuid=vutric@wonmor.io

XDG_RUNTIME_DIR=$(mktemp -d)
chmod 700 "$XDG_RUNTIME_DIR"
export XDG_RUNTIME_DIR GSETTINGS_BACKEND=dconf
export VUTRIC_SURFACE_LOG="$out/surface.log" RUST_BACKTRACE=1

ext_dir="$HOME/.local/share/gnome-shell/extensions/$uuid"
rm -rf "$ext_dir"
mkdir -p "$(dirname "$ext_dir")" "$HOME/.local/libexec/vutric"
cp -r "apps/gnome-extension/$uuid" "$ext_dir"
install -m 0755 "$bin/vutric-surface" "$HOME/.local/libexec/vutric/vutric-surface"

gsettings set org.gnome.shell disable-user-extensions false
gsettings set org.gnome.shell enabled-extensions "['$uuid']"
gnome-shell --version | tee "$out/versions.txt"

gnome-shell --headless --wayland --no-x11 --virtual-monitor 1920x1080 > "$out/gnome-shell.log" 2>&1 &
shell_pid=$!
trap 'kill "$shell_pid" 2>/dev/null || true' EXIT

ping() {
    gdbus call --session --dest org.gnome.Shell --object-path /io/wonmor/Vutric/Shell \
        --method io.wonmor.Vutric.Shell1.Ping 2>/dev/null
}
ok=""
for _ in $(seq 1 150); do
    if ping > "$out/ping.txt"; then ok=1; break; fi
    sleep 0.2
done
if [ -z "$ok" ]; then
    echo "the extension never answered Ping"
    gnome-extensions info "$uuid" || true
    tail -n 80 "$out/gnome-shell.log"
    exit 1
fi
cat "$out/ping.txt"
gnome-extensions info "$uuid" | tee "$out/extension-info.txt" || true

"$bin/examples/surface_driver" --gnome --runs 3 --out "$out/driver.json" || {
    sleep 1
    echo "--- surface stderr"; cat "$out/surface.log" 2>/dev/null || true
    echo "--- shell log (tail)"; tail -n 40 "$out/gnome-shell.log"
    exit 1
}
grep -F 'Vutric: surface window adopted' "$out/gnome-shell.log" | tee "$out/adopted.txt"
echo "GNOME extension + surface: OK"
