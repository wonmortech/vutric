#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
# SPDX-License-Identifier: Apache-2.0
#
# Checks the signed Vutric.app against the sandbox and entitlements review (ADR-0120):
#   - the Share extension and the Quick Action carry exactly app-sandbox + user-selected.read-only;
#   - the agent and every helper carry no entitlement at all (no JIT, no unsigned memory, no library-validation or
#     dyld exemption, no get-task-allow);
#   - the agent's Info.plist has the Local Network purpose string (uz, and en in en.lproj) and declares exactly the
#     Bonjour types the code announces (read from the Rust sources, so the two cannot drift);
#   - with REQUIRE_RUNTIME=1 (Developer ID builds) every Mach-O has the hardened runtime flag.
#
#   packaging/macos/check-entitlements.sh target/macos/Vutric.app
set -euo pipefail

APP=${1:?Vutric.app}
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
fail=0

entitlements() { # BUNDLE_OR_BINARY OUT
  codesign --display --entitlements - --xml "$1" >"$2" 2>/dev/null || : >"$2"
}

for appex in VutricShare VutricQuickAction; do
  entitlements "$APP/Contents/PlugIns/$appex.appex" "$WORK/$appex.xml"
done
entitlements "$APP" "$WORK/agent.xml"
helpers=()
for helper in vutric-daemon vutric-mac-compat vutric-compat vutric-app; do
  if [[ -f $APP/Contents/MacOS/$helper ]]; then
    entitlements "$APP/Contents/MacOS/$helper" "$WORK/$helper.xml"
    helpers+=("$helper")
  fi
done

# Bonjour types the code announces or browses.
vutric=$(sed -n 's/^pub const MDNS_SERVICE: &str = "\(.*\)";/\1/p' "$ROOT/crates/vutric-net/src/lib.rs")
quickshare=$(sed -n 's/^pub const SERVICE_TYPE: &str = "\(.*\)\.local\.";/\1/p' \
  "$ROOT/crates/vutric-compat-quickshare/src/endpoint.rs")
plutil -convert xml1 -o "$WORK/Info.plist" "$APP/Contents/Info.plist"

python3 - "$WORK" "$vutric" "$quickshare" "$APP" ${helpers[@]+"${helpers[@]}"} <<'PY' || fail=1
import pathlib, plistlib, sys

work, vutric, quickshare, app = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3], pathlib.Path(sys.argv[4])
helpers = sys.argv[5:]
ok = True

def ents(name):
    raw = (work / f"{name}.xml").read_bytes().strip()
    return plistlib.loads(raw) if raw else {}

def report(good, line):
    global ok
    ok &= good
    print(("ok   " if good else "FAIL ") + line)

sandboxed = {"com.apple.security.app-sandbox": True, "com.apple.security.files.user-selected.read-only": True}
for appex in ("VutricShare", "VutricQuickAction"):
    e = ents(appex)
    report(e == sandboxed, f"{appex}.appex entitlements: {sorted(e) or 'none'}")
for name in ["agent", *helpers]:
    e = ents(name)
    report(not e, f"{name} entitlements: {sorted(e) or 'none'}")

info = plistlib.loads((work / "Info.plist").read_bytes())
purpose = info.get("NSLocalNetworkUsageDescription", "")
report(bool(purpose.strip()), "NSLocalNetworkUsageDescription (uz) present")
en = app / "Contents/Resources/en.lproj/InfoPlist.strings"
report(en.exists() and "NSLocalNetworkUsageDescription" in en.read_text(encoding="utf-8"),
       "NSLocalNetworkUsageDescription (en) present")
declared = sorted(info.get("NSBonjourServices", []))
wanted = sorted(([f"_{vutric}._udp"] if vutric else []) + ([quickshare] if quickshare else []))
report(len(wanted) == 2, f"service types found in the Rust sources: {wanted}")
report(declared == wanted, f"NSBonjourServices {declared} == code {wanted}")
sys.exit(0 if ok else 1)
PY

if [[ ${REQUIRE_RUNTIME:-0} == 1 ]]; then
  while IFS= read -r bin; do
    if codesign --display --verbose=2 "$bin" 2>&1 | grep -q 'flags=.*runtime'; then
      echo "ok   hardened runtime: ${bin#"$APP"/}"
    else
      echo "FAIL hardened runtime: ${bin#"$APP"/}"
      fail=1
    fi
  done < <(find "$APP" -type f -perm -u+x -path '*/MacOS/*')
fi

[[ $fail == 0 ]] && echo "entitlements review (ADR-0120): all checks passed"
exit $fail
