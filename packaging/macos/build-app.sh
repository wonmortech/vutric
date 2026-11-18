#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
# SPDX-License-Identifier: Apache-2.0
#
# Builds Vutric.app (menu-bar agent + Share extension + Finder Quick Action + the Rust helper when its crate
# exists, + the icon drawn from the brand mark) as a universal
# arm64 + x86_64 bundle, with plain swiftc: no Xcode project to keep in sync (ADR-0113).
#
#   packaging/macos/build-app.sh            # -> target/macos/Vutric.app
#   packaging/macos/build-app.sh --test     # only the Swift unit checks
#
# Env: OUT (default target/macos), VERSION (default: workspace version), BUILD (default: run number or 1),
#      ARCHS (default "arm64 x86_64"), SKIP_COMPAT=1 (leave out the LocalSend / Quick Share helpers).
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
SRC=$ROOT/apps/macos
OUT=${OUT:-$ROOT/target/macos}
VERSION=${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)}
BUILD=${BUILD:-${GITHUB_RUN_NUMBER:-1}}
ARCHS=${ARCHS:-"arm64 x86_64"}
MIN=13.0
SDK=$(xcrun --sdk macosx --show-sdk-path)

swift_build() {
  xcrun swiftc -O -swift-version 5 -sdk "$SDK" "$@"
}

run_tests() {
  mkdir -p "$OUT/tests"
  swift_build -o "$OUT/tests/send-request" "$SRC/Tests/main.swift" "$SRC/Shared/SendRequest.swift" \
    "$SRC/Shared/SharedItems.swift"
  "$OUT/tests/send-request"
  python3 "$ROOT/packaging/macos/xcstrings.py" --check "$SRC"/Shared/*.xcstrings "$SRC"/Agent/*.xcstrings \
    "$SRC"/ShareExtension/*.xcstrings "$SRC"/QuickAction/*.xcstrings
  plutil -lint "$SRC/Agent/Info.plist" "$SRC/ShareExtension/Info.plist" "$SRC/QuickAction/Info.plist" \
    "$SRC"/*/*.entitlements
}

if [[ ${1:-} == "--test" ]]; then
  run_tests
  exit 0
fi
run_tests

OBJ=$OUT/obj
APP=$OUT/Vutric.app
APPEX=$APP/Contents/PlugIns/VutricShare.appex
QAEX=$APP/Contents/PlugIns/VutricQuickAction.appex
rm -rf "$APP" "$OBJ"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APPEX/Contents/MacOS" "$APPEX/Contents/Resources" \
  "$QAEX/Contents/MacOS" "$QAEX/Contents/Resources"

# Icon: drawn from design/claude-design/logo-mark.svg by a script (ADR-0116); no icon binary is checked in.
ICONS=$OBJ/icons
mkdir -p "$ICONS"
xcrun swift "$ROOT/packaging/macos/icon/make-icon.swift" "$ROOT/design/claude-design/logo-mark.svg" "$ICONS"
iconutil -c icns -o "$APP/Contents/Resources/AppIcon.icns" "$ICONS/AppIcon.iconset"
cp "$ICONS"/QuickActionTemplate*.png "$QAEX/Contents/Resources/"

ext_build() { # arch module out sources...
  local arch=$1 module=$2 out=$3
  shift 3
  swift_build -target "$arch-apple-macos$MIN" -module-name "$module" -parse-as-library -application-extension \
    -Xlinker -e -Xlinker _NSExtensionMain -o "$out" "$@" "$SRC/Shared/SendRequest.swift" \
    "$SRC/Shared/SharedItems.swift" "$SRC/Shared/L10n.swift"
}

agents=() shares=() actions=()
for arch in $ARCHS; do
  mkdir -p "$OBJ/$arch"
  swift_build -target "$arch-apple-macos$MIN" -module-name Vutric -o "$OBJ/$arch/Vutric" \
    "$SRC"/Agent/Sources/*.swift "$SRC/Shared/SendRequest.swift" "$SRC/Shared/L10n.swift"
  ext_build "$arch" VutricShare "$OBJ/$arch/VutricShare" "$SRC"/ShareExtension/Sources/*.swift
  ext_build "$arch" VutricQuickAction "$OBJ/$arch/VutricQuickAction" "$SRC"/QuickAction/Sources/*.swift
  agents+=("$OBJ/$arch/Vutric")
  shares+=("$OBJ/$arch/VutricShare")
  actions+=("$OBJ/$arch/VutricQuickAction")
done
lipo -create -output "$APP/Contents/MacOS/Vutric" "${agents[@]}"
lipo -create -output "$APPEX/Contents/MacOS/VutricShare" "${shares[@]}"
lipo -create -output "$QAEX/Contents/MacOS/VutricQuickAction" "${actions[@]}"

rust_universal() { # package bin -> Contents/MacOS/bin
  local package=$1 bin=$2 parts=()
  for arch in $ARCHS; do
    triple=$([[ $arch == arm64 ]] && echo aarch64-apple-darwin || echo x86_64-apple-darwin)
    rustup target add "$triple" >/dev/null
    MACOSX_DEPLOYMENT_TARGET=$MIN cargo build --release --locked -p "$package" --bin "$bin" \
      --target "$triple" --manifest-path "$ROOT/Cargo.toml"
    parts+=("${CARGO_TARGET_DIR:-$ROOT/target}/$triple/release/$bin")
  done
  lipo -create -output "$APP/Contents/MacOS/$bin" "${parts[@]}"
}

# Foreground LocalSend / Quick Share (ADR-0117): the bridge the agent starts, and the adapter process it starts.
if [[ ${SKIP_COMPAT:-0} != 1 ]]; then
  rust_universal vutric-host-mac vutric-mac-compat
  rust_universal vutric-compat-localsend vutric-compat
fi

# The Rust helper (network, transfers, overlay) is W2's crate; bundled as soon as it is on the branch.
if [[ -f $ROOT/crates/vutric-daemon/Cargo.toml ]]; then
  rust_universal vutric-daemon vutric-daemon
else
  echo "::notice title=No daemon in this build::crates/vutric-daemon is not on this branch yet; Vutric.app ships the menu-bar agent and the Share extension only."
fi

fill() {
  sed -e "s/__VERSION__/$VERSION/g" -e "s/__BUILD__/$BUILD/g" "$1" >"$2"
  plutil -lint "$2" >/dev/null
}
fill "$SRC/Agent/Info.plist" "$APP/Contents/Info.plist"
fill "$SRC/ShareExtension/Info.plist" "$APPEX/Contents/Info.plist"
fill "$SRC/QuickAction/Info.plist" "$QAEX/Contents/Info.plist"
printf 'APPL????' >"$APP/Contents/PkgInfo"

python3 "$ROOT/packaging/macos/xcstrings.py" "$APP/Contents/Resources" \
  "$SRC/Shared/Localizable.xcstrings" "$SRC/Agent/InfoPlist.xcstrings" "$SRC/Agent/ServicesMenu.xcstrings"
python3 "$ROOT/packaging/macos/xcstrings.py" "$APPEX/Contents/Resources" \
  "$SRC/Shared/Localizable.xcstrings" "$SRC/ShareExtension/InfoPlist.xcstrings"
python3 "$ROOT/packaging/macos/xcstrings.py" "$QAEX/Contents/Resources" \
  "$SRC/Shared/Localizable.xcstrings" "$SRC/QuickAction/InfoPlist.xcstrings"

echo "Built $APP ($VERSION, build $BUILD, $ARCHS)"
lipo -info "$APP/Contents/MacOS/Vutric" "$APPEX/Contents/MacOS/VutricShare" "$QAEX/Contents/MacOS/VutricQuickAction"
du -sh "$APP"
