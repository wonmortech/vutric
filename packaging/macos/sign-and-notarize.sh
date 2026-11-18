#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
# SPDX-License-Identifier: Apache-2.0
#
# Signs Vutric.app with a Developer ID certificate (hardened runtime, secure timestamp), packs it into a .dmg,
# signs the .dmg, notarises it with notarytool and staples the ticket (ADR-0113).
#
#   packaging/macos/sign-and-notarize.sh target/macos/Vutric.app target/macos/Vutric-<version>.dmg
#
# Without the Apple secrets it still produces a .dmg, ad-hoc signed and NOT notarised, and says so. Secrets:
#   MACOS_CERTIFICATE_P12      base64 of the "Developer ID Application" certificate + private key (.p12)
#   MACOS_CERTIFICATE_PASSWORD password of that .p12
#   APPLE_TEAM_ID              10-character team id
#   and for notarytool, either an App Store Connect API key (preferred)
#     NOTARY_API_KEY_P8        base64 of AuthKey_<id>.p8
#     NOTARY_API_KEY_ID        key id
#     NOTARY_API_ISSUER_ID     issuer UUID
#   or an Apple ID
#     APPLE_ID, APPLE_APP_SPECIFIC_PASSWORD
set -euo pipefail

APP=${1:?path to Vutric.app}
DMG=${2:?output .dmg path}
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
APPEX=$APP/Contents/PlugIns/VutricShare.appex
SHARE_ENT=$ROOT/apps/macos/ShareExtension/ShareExtension.entitlements
AGENT_ENT=$ROOT/apps/macos/Agent/Agent.entitlements
WORK=${RUNNER_TEMP:-$(mktemp -d)}/vutric-sign

make_dmg() {
  local staging=$WORK/dmg
  rm -rf "$staging" "$DMG"
  mkdir -p "$staging"
  cp -R "$APP" "$staging/"
  ln -s /Applications "$staging/Applications"
  hdiutil create -quiet -volname Vutric -srcfolder "$staging" -fs HFS+ -format UDZO -ov "$DMG"
}

# Inside out: nested code first, the app last; --deep is not used (Apple TN2206).
sign_all() {
  local id=$1
  shift
  local flags=(--force --sign "$id" "$@")
  codesign "${flags[@]}" --entitlements "$SHARE_ENT" "$APPEX"
  if [[ -f $APP/Contents/MacOS/vutric-daemon ]]; then
    codesign "${flags[@]}" "$APP/Contents/MacOS/vutric-daemon"
  fi
  codesign "${flags[@]}" --entitlements "$AGENT_ENT" "$APP"
  codesign --verify --strict --verbose=2 "$APP"
}

mkdir -p "$WORK"

if [[ -z ${MACOS_CERTIFICATE_P12:-} || -z ${MACOS_CERTIFICATE_PASSWORD:-} || -z ${APPLE_TEAM_ID:-} ]]; then
  echo "::notice title=Unsigned .dmg::Apple Developer secrets are not set (MACOS_CERTIFICATE_P12, MACOS_CERTIFICATE_PASSWORD, APPLE_TEAM_ID), so this .dmg is ad-hoc signed and not notarised. Gatekeeper will block it on other Macs: right-click -> Open, or xattr -dr com.apple.quarantine Vutric.app. See docs/owner-questions.md (W5)."
  sign_all -
  make_dmg
  echo "Built $DMG (ad-hoc signed, not notarised)"
  exit 0
fi

KEYCHAIN=$WORK/vutric.keychain-db
KEYCHAIN_PASSWORD=$(openssl rand -hex 24)
cleanup() {
  security delete-keychain "$KEYCHAIN" 2>/dev/null || true
  rm -f "$WORK/cert.p12" "$WORK/AuthKey.p8"
}
trap cleanup EXIT

echo "$MACOS_CERTIFICATE_P12" | base64 --decode >"$WORK/cert.p12"
security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
security set-keychain-settings -lut 3600 "$KEYCHAIN"
security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
security import "$WORK/cert.p12" -k "$KEYCHAIN" -P "$MACOS_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple: -s -k "$KEYCHAIN_PASSWORD" "$KEYCHAIN" >/dev/null
# shellcheck disable=SC2046 # the existing search list is a list of quoted paths
security list-keychains -d user -s "$KEYCHAIN" $(security list-keychains -d user | tr -d '"')

IDENTITY=$(security find-identity -v -p codesigning "$KEYCHAIN" |
  awk -v team="($APPLE_TEAM_ID)" '/Developer ID Application/ && index($0, team) { print $2; exit }')
if [[ -z $IDENTITY ]]; then
  echo "::error::No 'Developer ID Application' identity for team $APPLE_TEAM_ID in MACOS_CERTIFICATE_P12"
  exit 1
fi

sign_all "$IDENTITY" --options runtime --timestamp
make_dmg
codesign --force --sign "$IDENTITY" --timestamp "$DMG"

if [[ -n ${NOTARY_API_KEY_P8:-} && -n ${NOTARY_API_KEY_ID:-} && -n ${NOTARY_API_ISSUER_ID:-} ]]; then
  echo "$NOTARY_API_KEY_P8" | base64 --decode >"$WORK/AuthKey.p8"
  auth=(--key "$WORK/AuthKey.p8" --key-id "$NOTARY_API_KEY_ID" --issuer "$NOTARY_API_ISSUER_ID")
elif [[ -n ${APPLE_ID:-} && -n ${APPLE_APP_SPECIFIC_PASSWORD:-} ]]; then
  auth=(--apple-id "$APPLE_ID" --password "$APPLE_APP_SPECIFIC_PASSWORD" --team-id "$APPLE_TEAM_ID")
else
  echo "::warning title=Signed, not notarised::No notarytool credentials (NOTARY_API_KEY_* or APPLE_ID + APPLE_APP_SPECIFIC_PASSWORD); the .dmg is Developer ID signed only."
  exit 0
fi

xcrun notarytool submit "$DMG" "${auth[@]}" --wait --timeout 30m --output-format json | tee "$WORK/notary.json"
if ! grep -q '"status" *: *"Accepted"' "$WORK/notary.json"; then
  id=$(sed -n 's/.*"id" *: *"\([^"]*\)".*/\1/p' "$WORK/notary.json" | head -1)
  [[ -n $id ]] && xcrun notarytool log "$id" "${auth[@]}" || true
  echo "::error::Notarisation was not accepted"
  exit 1
fi
xcrun stapler staple "$DMG"
xcrun stapler validate "$DMG"
spctl --assess --type open --context context:primary-signature --verbose=2 "$DMG"
echo "Built $DMG (Developer ID signed, notarised, stapled)"
