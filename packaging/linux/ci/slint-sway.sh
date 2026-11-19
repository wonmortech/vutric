#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
# SPDX-License-Identifier: Apache-2.0
#
# Runs W2's Slint window (M1 spike today, the real settings window later) under headless sway and records the M1
# measurement log (first frame, frame stats; ADR-0030 contract: VUTRIC_M1_LOG, VUTRIC_M1_FRAMES).
# Usage: slint-sway.sh PATH_TO_BINARY OUT_DIR
set -euo pipefail

exe=$(realpath "$1")
out=${2:-slint-m1}
mkdir -p "$out"
out=$(realpath "$out")

XDG_RUNTIME_DIR=$(mktemp -d)
chmod 700 "$XDG_RUNTIME_DIR"
export XDG_RUNTIME_DIR
export WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1
printf 'output HEADLESS-1 mode 1920x1080@60Hz\n' > "$XDG_RUNTIME_DIR/sway.conf"
sway -c "$XDG_RUNTIME_DIR/sway.conf" > "$out/sway.log" 2>&1 &
sway_pid=$!
trap 'kill "$sway_pid" 2>/dev/null || true' EXIT

sock=""
for _ in $(seq 1 100); do
    sock=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -name 'wayland-*' ! -name '*.lock' -printf '%f\n' | head -n 1)
    [ -n "$sock" ] && break
    sleep 0.1
done
[ -n "$sock" ] || { echo "sway did not start"; exit 1; }
export WAYLAND_DISPLAY=$sock XDG_SESSION_TYPE=wayland

status=0
for backend in winit-femtovg winit-software; do
    log="$out/m1-$backend.jsonl"
    rm -f "$log"
    SLINT_BACKEND=$backend VUTRIC_M1_LOG="$log" VUTRIC_M1_FRAMES=300 \
        timeout 60 "$exe" > "$out/m1-$backend.stdout" 2>&1 &
    app=$!
    for _ in $(seq 1 60); do
        if [ -f "$log" ] && grep -q '"event":"frames"' "$log"; then break; fi
        sleep 1
    done
    kill "$app" 2>/dev/null || true
    wait "$app" 2>/dev/null || true
    echo "--- $backend"
    if [ -f "$log" ] && grep -q '"event":"frames"' "$log"; then
        cat "$log"
    else
        echo "no frame stats from $backend"; tail -n 20 "$out/m1-$backend.stdout" || true
        status=1
    fi
done
exit "$status"
