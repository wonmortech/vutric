#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
# SPDX-License-Identifier: Apache-2.0
#
# Launches the built Vutric.app on a CI runner and records what can be observed without a person:
# the agent stays up, macOS registers the Share extension and the Finder Quick Action, the Services entry is known,
# `vutric://send` brings up the confirmation question for files and for text (screenshots), the icon is in the
# bundle, and the LocalSend / Quick Share bridge starts its adapter. Writes smoke.txt and PNGs into OUT_DIR.
#
#   packaging/macos/smoke.sh target/macos/Vutric.app OUT_DIR
set -uo pipefail

APP=${1:?Vutric.app}
OUT=${2:?output directory}
mkdir -p "$OUT"
LOG=$OUT/smoke.txt
fail=0
say() { echo "$*" | tee -a "$LOG"; }

dest=$HOME/Applications/Vutric.app
mkdir -p "$HOME/Applications"
rm -rf "$dest"
cp -R "$APP" "$dest"
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$dest"

open "$dest"
sleep 5
if pgrep -x Vutric >/dev/null; then say "agent running: yes (pid $(pgrep -x Vutric))"; else say "agent running: NO"; fail=1; fi
screencapture -x "$OUT/menubar.png" 2>/dev/null || true

pluginkit -a "$dest/Contents/PlugIns/VutricShare.appex" 2>&1 | tee -a "$LOG"
sleep 2
if pluginkit -m -v -i io.wonmor.vutric.share 2>&1 | tee -a "$LOG" | grep -q io.wonmor.vutric.share; then
  say "share extension registered: yes"
else
  say "share extension registered: NO"
  fail=1
fi

pluginkit -a "$dest/Contents/PlugIns/VutricQuickAction.appex" 2>&1 | tee -a "$LOG"
sleep 2
if pluginkit -m -v -i io.wonmor.vutric.quickaction 2>&1 | tee -a "$LOG" | grep -q io.wonmor.vutric.quickaction; then
  say "quick action registered: yes"
else
  say "quick action registered: NO"
  fail=1
fi

if [[ -s $dest/Contents/Resources/AppIcon.icns ]]; then
  say "app icon: yes ($(wc -c <"$dest/Contents/Resources/AppIcon.icns") bytes)"
  sips -s format png "$dest/Contents/Resources/AppIcon.icns" --out "$OUT/AppIcon.png" >/dev/null 2>&1 || true
else
  say "app icon: NO"
  fail=1
fi

/System/Library/CoreServices/pbs -update 2>/dev/null || true
if /System/Library/CoreServices/pbs -dump_pboard 2>/dev/null | grep -q "Vutric bilan yuborish"; then
  say "services entry known to pbs: yes"
else
  say "services entry known to pbs: not listed (pbs output varies by macOS release; informational)"
fi

open "vutric://send?path=/etc/hosts&path=/etc/shells"
sleep 3
screencapture -x "$OUT/send-question.png" 2>/dev/null || true
frontmost=$(osascript -e 'tell application "System Events" to get name of first application process whose frontmost is true' 2>/dev/null || echo "?")
say "frontmost after vutric://send: $frontmost"
windows=$(osascript -e 'tell application "System Events" to get name of every window of process "Vutric"' 2>/dev/null || echo "?")
say "Vutric windows: $windows"

osascript -e 'tell application "System Events" to key code 53' >/dev/null 2>&1 || true
open "vutric://send?text=Salom%20dunyo%0Ahttps%3A%2F%2Fvutric.wonmor.io%2F"
sleep 3
screencapture -x "$OUT/send-text-question.png" 2>/dev/null || true
say "text question shown: see send-text-question.png"

pkill -x Vutric || true
sleep 1
if pgrep -x Vutric >/dev/null; then say "agent quit: NO"; else say "agent quit: yes"; fi
# The bridge on its own: it must start the adapter and say ready; a stop line ends it.
bridge=$dest/Contents/MacOS/vutric-mac-compat
if [[ -x $bridge ]]; then
  { sleep 6; echo '{"op":"announce"}'; sleep 2; echo '{"op":"stop"}'; } |
    HOME=$OUT/bridge-home "$bridge" --alias "Vutric CI" --port 0 >"$OUT/bridge.jsonl" 2>"$OUT/bridge.err"
  say "bridge exit: $?"
  if grep -q '"kind":"ready"' "$OUT/bridge.jsonl"; then
    say "bridge ready: yes ($(grep -m1 '"kind":"ready"' "$OUT/bridge.jsonl"))"
  else
    say "bridge ready: NO"
    head -c 2000 "$OUT/bridge.jsonl" "$OUT/bridge.err" | tee -a "$LOG"
    fail=1
  fi
else
  say "bridge: not in this build"
fi

log show --last 2m --predicate 'process == "Vutric"' --style compact 2>/dev/null | tail -n 40 >"$OUT/agent-log.txt" || true
[[ -n ${GITHUB_STEP_SUMMARY:-} ]] && { echo '### Vutric.app smoke test'; echo '```'; cat "$LOG"; echo '```'; } >>"$GITHUB_STEP_SUMMARY"
exit $fail
