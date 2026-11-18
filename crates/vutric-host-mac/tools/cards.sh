#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
# SPDX-License-Identifier: Apache-2.0
#
# Card latency and focus on the runner (ADR-0118): TextEdit frontmost, then the cards example, with screenshots
# while each card is held on screen. Writes cards-mac.json, card-mac-*.png and screen-*.png into OUT_DIR.
#
#   crates/vutric-host-mac/tools/cards.sh EXAMPLE_BINARY OUT_DIR
#
# Fails only when the example writes no JSON; the numbers are evidence, not a gate (virtual machines).
set -uo pipefail

BIN=${1:?cards example binary}
OUT=${2:?output directory}
mkdir -p "$OUT"
open -a TextEdit 2>/dev/null && sleep 3
osascript -e 'tell application "TextEdit" to make new document' -e 'tell application "TextEdit" to activate' \
  >/dev/null 2>&1 || true
sleep 1
(
  sleep 1
  for i in $(seq 1 12); do
    screencapture -x -t png "$OUT/screen-$i.png" 2>/dev/null || true
    sleep 0.6
  done
) &
shots=$!
"$BIN" --out "$OUT/cards-mac.json" --runs 200 --hold-ms 1500 >"$OUT/cards.log" 2>&1
code=$?
wait "$shots"
echo "cards exit=$code"
if [[ ! -s $OUT/cards-mac.json ]]; then
  echo "::error::the cards example wrote no JSON (exit $code)"
  tail -n 40 "$OUT/cards.log"
  exit 1
fi
python3 - "$OUT/cards-mac.json" <<'PY' | tee "$OUT/summary.md"
import json, sys
r = json.load(open(sys.argv[1]))
print("### macOS cards (ADR-0118)\n")
print(f"os {r['os']} · scale {r['scale']} · fonts {r['fonts_load_ms']} ms · first show {r['first_show_ms']:.2f} ms · "
      f"frontmost before {r['frontmost_before']} · focus kept {r['focus_kept']} · click {json.dumps(r['click'])}\n")
print("| card | px | raster p50/p99 | show p50/p99 | total p50/p99 |")
print("|---|---|---|---|---|")
for c in r["cards"]:
    print(f"| {c['card']} | {c['px'][0]}×{c['px'][1]} | {c['raster_p50_ms']:.2f} / {c['raster_p99_ms']:.2f} | "
          f"{c['show_p50_ms']:.2f} / {c['show_p99_ms']:.2f} | {c['total_p50_ms']:.2f} / {c['total_p99_ms']:.2f} |")
PY
[[ -n ${GITHUB_STEP_SUMMARY:-} ]] && cat "$OUT/summary.md" >>"$GITHUB_STEP_SUMMARY"
exit 0
