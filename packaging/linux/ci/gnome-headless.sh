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

fail=0
check() { # name, condition result (0 = ok)
    local line="PASS  $1"
    if [ "$2" != 0 ]; then line="FAIL  $1"; fail=1; fi
    echo "$line" | tee -a "$out/summary.txt"
}
note() { echo "NOTE  $1" | tee -a "$out/summary.txt"; }
check "extension loaded, Ping answered" 0

# 1. The surface as an ordinary Wayland client of Mutter (no extension): wgpu on GNOME's compositor.
direct=0
WAYLAND_DISPLAY=wayland-0 "$bin/vutric-surface" --backend gnome --self-test 3 --cards 0 \
    --out "$out/self-test.json" > "$out/self-test.txt" 2>&1 || direct=$?
cat "$out/self-test.txt"
check "surface as a plain Mutter client plays 3 effects (self-test)" "$direct"

# 2. Launched by the extension as a trusted client and driven over the daemon protocol.
"$bin/examples/surface_driver" --gnome --runs 3 --out "$out/driver.json" || true
sleep 1
cat "$out/driver.json"
has() { jq -e "$1" "$out/driver.json" > /dev/null; echo $?; }
check "ShowCard / HideCard / SetStatus over D-Bus" "$(has '.card != null')"
check "extension launched the surface, Ready received" "$(has '.ready | test("Ready")')"
check "surface warmed (GPU up) inside the extension-launched window" "$(has '.warmed | test("Warmed")')"
adopted=0
grep -F 'Vutric: surface window adopted' "$out/gnome-shell.log" > "$out/adopted.txt" || adopted=$?
check "extension adopted the window (dock, hidden from window list)" "$adopted"

# 3. Frames after the dock retype. Headless Mutter (virtual monitor, surfaceless renderer, no GPU) sends the dock
#    window no frame callbacks, so effects time out there; whether a real GNOME session does is checklist row 1/5
#    (ADR-0090 "CI limits"). Recorded, not gated.
played=$(jq '.played | length' "$out/driver.json")
note "effects played after the dock retype on headless Mutter: $played of 3 (not gated; needs a real GNOME session)"
grep -F 'Vutric' "$out/gnome-shell.log" || true
echo "--- surface stderr"; cat "$out/surface.log" 2>/dev/null || true
[ "$fail" = 0 ] || { tail -n 40 "$out/gnome-shell.log"; exit 1; }
echo "GNOME extension + surface: all gated checks passed"
