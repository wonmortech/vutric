#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
# SPDX-License-Identifier: Apache-2.0
#
# Runs vutric-mac-probe in each window mode with TextEdit frontmost (the focus check needs an app to keep), takes
# screenshots while the effect plays, and writes one JSON per run plus summary.md into OUT_DIR.
#
#   crates/vutric-host-mac/tools/measure.sh PROBE_BINARY OUT_DIR
#
# Never fails on the numbers: CI runners are virtual machines, so the JSON is evidence, not a gate. It fails only
# when the probe crashes without writing its JSON.
set -uo pipefail

PROBE=${1:?probe binary}
OUT=${2:?output directory}
mkdir -p "$OUT"

{
  echo "date: $(date -u +%FT%TZ)"
  sw_vers
  sysctl -n machdep.cpu.brand_string 2>/dev/null || true
  system_profiler SPDisplaysDataType 2>/dev/null | sed -n '1,40p'
} >"$OUT/machine.txt"

open -a TextEdit 2>/dev/null && sleep 3
osascript -e 'tell application "TextEdit" to make new document' -e 'tell application "TextEdit" to activate' \
  >/dev/null 2>&1 || true
sleep 1

status=0
for spec in "panel status" "winit status" "panel screen-saver"; do
  read -r mode level <<<"$spec"
  name="$mode-$level"
  (
    sleep 1.2
    for i in 1 2 3 4 5 6; do
      screencapture -x -t png "$OUT/$name-$i.png" 2>/dev/null || true
      sleep 0.35
    done
  ) &
  shots=$!
  "$PROBE" --runs 5 --window "$mode" --level "$level" --json "$OUT/$name.json" >"$OUT/$name.log" 2>&1
  code=$?
  wait "$shots"
  echo "$name exit=$code" | tee -a "$OUT/exits.txt"
  if [[ ! -s $OUT/$name.json ]]; then
    echo "::error::$name: the probe wrote no JSON (exit $code)"
    tail -n 40 "$OUT/$name.log"
    status=1
  fi
done

python3 - "$OUT" >"$OUT/summary.md" <<'EOF'
import json, pathlib, sys
out = pathlib.Path(sys.argv[1])
print("### macOS overlay probe\n")
m = (out / "machine.txt").read_text(errors="replace").splitlines()
print("```\n" + "\n".join(m[:8]) + "\n```\n")
print("| run | os | refresh | adapter | alpha | level | fps | p50 | p99 | max | missed | first | focus kept | app inactive | never key | pass | error |")
print("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|")
for f in sorted(out.glob("*.json")):
    r = json.loads(f.read_text())
    runs = r.get("runs") or [{}]
    win = r.get("window") or {}
    for i, x in enumerate(runs, 1):
        def v(k, fmt="{:.1f}"):
            val = x.get(k)
            return "–" if val is None else (fmt.format(val) if isinstance(val, float) else str(val))
        print(f"| {f.stem} #{i} | {r.get('os','')} | {r.get('refresh_hz','')} | {r.get('adapter') or '–'} | "
              f"{r.get('alpha_mode') or '–'} | {win.get('level','–')} | {v('fps')} | {v('p50_ms','{:.2f}')} | "
              f"{v('p99_ms','{:.2f}')} | {v('max_ms','{:.2f}')} | {v('missed_refreshes')} | "
              f"{v('first_frame_ms','{:.2f}')} | {v('focus_kept')} | {v('app_never_active')} | "
              f"{v('panel_never_key')} | {r.get('passed')} | {r.get('error') or ''} |")
EOF
cat "$OUT/summary.md"
[[ -n ${GITHUB_STEP_SUMMARY:-} ]] && cat "$OUT/summary.md" >>"$GITHUB_STEP_SUMMARY"
exit $status
