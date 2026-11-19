#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
# SPDX-License-Identifier: Apache-2.0
#
# C3 / C1 on Linux (ADR-0093 §4): starts the compat adapter with Quick Share and LocalSend on, next to a running
# avahi-daemon, and checks that Avahi sees the Quick Share advert (`_FC9F5ED42C8A._tcp`), the way an Android phone's
# share sheet would find this computer. Usage: compat-avahi.sh PATH_TO_vutric-compat OUT_DIR
set -euo pipefail

exe=$1
out=${2:-compat-avahi}
mkdir -p "$out"
work=$(mktemp -d)
pid=""
trap 'kill "$pid" 2>/dev/null || true; rm -rf "$work"' EXIT
mkdir -p "$work/staging"

fifo="$work/in"
mkfifo "$fifo"
start=$(printf '{"t":"start","alias":"vutric-ci","device_type":"desktop","port":0,"bind":"0.0.0.0","staging":"%s","identity":"%s","pin":null,"multicast":true,"decide_timeout_secs":30,"quickshare":true,"kdeconnect":false}' \
    "$work/staging" "$work/identity")
# Keep the adapter's stdin open (it exits when stdin closes).
{ echo "$start"; sleep 60; } > "$fifo" &
"$exe" < "$fifo" > "$out/adapter.jsonl" 2> "$out/adapter.stderr" &
pid=$!

found=""
for _ in $(seq 1 30); do
    if avahi-browse --parsable --resolve --terminate _FC9F5ED42C8A._tcp > "$out/avahi-browse.txt" 2>&1 \
        && grep -q '^=' "$out/avahi-browse.txt"; then
        found=1
        break
    fi
    sleep 1
done
echo "--- adapter events"; head -n 20 "$out/adapter.jsonl" || true
echo "--- avahi-browse"; cat "$out/avahi-browse.txt"
avahi-browse --all --parsable --terminate > "$out/avahi-all.txt" 2>&1 || true
[ -n "$found" ] || { echo "Avahi did not see the Quick Share advert"; exit 1; }
echo "Quick Share advert visible next to avahi-daemon: OK"
