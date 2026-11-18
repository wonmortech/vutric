#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
# SPDX-License-Identifier: Apache-2.0
#
# Builds the Rust library for iPhone and generates its UniFFI Swift bindings into ios/Generated/ (ADR-0015). Run it
# on a Mac before opening Runner.xcworkspace; CI (ios-spike.yml) runs the same steps.
set -euo pipefail
root="$(cd "$(dirname "$0")/../../.." && pwd)"
ios="$root/apps/mobile-spike/ios"
cd "$root"
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-15.0}"
rustup target add aarch64-apple-ios >/dev/null
cargo build --locked --release -p vutric-mobile --target aarch64-apple-ios
mkdir -p "$ios/Rust" "$ios/Generated"
cp target/aarch64-apple-ios/release/libvutric_mobile.a "$ios/Rust/"
cargo run --locked --release -q -p vutric-uniffi-bindgen -- generate \
  --library target/aarch64-apple-ios/release/libvutric_mobile.a --language swift --out-dir "$ios/Generated"
# Clang finds the FFI module through SWIFT_INCLUDE_PATHS only under this name.
mv "$ios/Generated/vutric_mobileFFI.modulemap" "$ios/Generated/module.modulemap"
ls -la "$ios/Generated"
