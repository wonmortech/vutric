#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
# SPDX-License-Identifier: Apache-2.0
#
# Runs the layer-shell overlay on headless sway (wlroots) and records frame stats (ADR-0090, spike 0.3).
# Usage: packaging/linux/ci/sway-headless.sh OUT_DIR   (after `cargo build --release -p vutric-host-linux
#        --bin vutric-surface --example surface_driver`)
set -euo pipefail

out=${1:-linux-overlay}
mkdir -p "$out"
bin=target/release

XDG_RUNTIME_DIR=$(mktemp -d)
chmod 700 "$XDG_RUNTIME_DIR"
export XDG_RUNTIME_DIR
export WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1

conf="$XDG_RUNTIME_DIR/sway.conf"
printf 'output HEADLESS-1 mode 1920x1080@60Hz\n' > "$conf"
sway -c "$conf" > "$out/sway.log" 2>&1 &
sway_pid=$!
trap 'kill "$sway_pid" 2>/dev/null || true' EXIT

sock=""
for _ in $(seq 1 100); do
    sock=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -name 'wayland-*' ! -name '*.lock' -printf '%f\n' | head -n 1)
    [ -n "$sock" ] && break
    sleep 0.1
done
[ -n "$sock" ] || { echo "sway did not start"; cat "$out/sway.log"; exit 1; }
export WAYLAND_DISPLAY=$sock XDG_SESSION_TYPE=wayland XDG_CURRENT_DESKTOP=sway

{
    sway --version
    vulkaninfo --summary 2>/dev/null | grep -E 'deviceName|driverName|apiVersion' || true
    uname -srm
} | tee "$out/versions.txt"

SWAYSOCK=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -name 'sway-ipc.*.sock' | head -n 1)
export SWAYSOCK
swaymsg -t get_outputs -r > "$out/outputs.json" || true

"$bin/vutric-surface" --self-test 5 --cards 50 --out "$out/self-test.json" | tee "$out/self-test.txt"
"$bin/examples/surface_driver" --spawn "$bin/vutric-surface" --runs 3 --out "$out/driver.json"
echo "layer-shell overlay on sway: OK"
