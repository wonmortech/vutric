# ADR-0113: macOS packaging = universal `Vutric.app` built with `swiftc`, Developer ID `.dmg`, notarytool

- Status: accepted; the unsigned path runs in CI; the signed and notarised path is written and waits for the
  owner's Apple Developer secrets (`docs/owner-questions.md`, `(W5)`).
- Date: 2026-11-18
- Stream: W5. Roadmap Phase 5 ("Notarised `.dmg`"). Backlog: D11.

## Decision

1. **No Xcode project.** `packaging/macos/build-app.sh` compiles the agent and the Share extension with `swiftc`
   for `arm64` and `x86_64` (`-target <arch>-apple-macos13.0`), joins them with `lipo`, compiles the String
   Catalogs with `xcstrings.py`, fills the version into the Info.plists and lays out the bundle. The extension is
   linked with `-application-extension -e _NSExtensionMain`. A generated `.pbxproj` would be a second source of
   truth that nobody on this Windows-first team can edit by hand. When `crates/vutric-daemon` is on the branch,
   the script also builds it for both architectures and bundles it as `Contents/MacOS/vutric-daemon`.
2. **Minimum macOS 13** (Ventura): `SMAppService` needs it; wgpu's Metal backend and `objc2-app-kit` work far below
   that. The real-Mac checklist targets macOS 15 and 26.
3. **Signing, inside out, no `--deep`** (`packaging/macos/sign-and-notarize.sh`): Share extension (with its sandbox
   entitlements), the Rust helper, then the app; hardened runtime and secure timestamp; no runtime exceptions. Then
   a compressed HFS+ `.dmg` with an `/Applications` link, signed itself, submitted with `xcrun notarytool submit
   --wait`, stapled, and checked with `spctl --assess`.
4. **Without the secrets** the same script builds the `.dmg` ad-hoc signed (`codesign -s -`), prints a GitHub notice
   that it is not notarised and how to open it, and the job stays green. A rejected notarisation fails the job and
   prints the notary log.
5. **Secrets** (repository secrets, only read by the `app` job of `.github/workflows/macos-desktop.yml`):

   | Secret | What |
   |---|---|
   | `MACOS_CERTIFICATE_P12` | base64 of the "Developer ID Application" certificate with its private key |
   | `MACOS_CERTIFICATE_PASSWORD` | the `.p12` password |
   | `APPLE_TEAM_ID` | 10-character team id |
   | `NOTARY_API_KEY_P8`, `NOTARY_API_KEY_ID`, `NOTARY_API_ISSUER_ID` | App Store Connect API key for notarytool (preferred) |
   | or `APPLE_ID`, `APPLE_APP_SPECIFIC_PASSWORD` | Apple ID fallback for notarytool |

   The certificate goes into a throw-away keychain that is deleted when the script exits.

## Consequences

- Fork PRs get no secrets, so they always build the unsigned `.dmg`; only branches of `wonmortech/vutric` can
  produce a notarised one.
- The Apple Developer Program costs 99 USD a year; the owner decides (owner question). Without it, users must
  right-click → Open (or remove the quarantine attribute) on first launch, and macOS 15 makes that harder each
  release (System Settings → Privacy & Security → "Open Anyway").
- A Homebrew cask can follow once a notarised `.dmg` has a stable URL (as the bitg tap does).
- No app icon yet: the menu-bar item uses the SF Symbol `paperplane.circle` as a template image. The real icon
  comes from the design handoff (W1 owns `design/**`).

## Real-Mac checklist (owed)

- [~] The unsigned `.dmg` from CI opens on Intel and Apple silicon after "Open Anyway"; `lipo -info` shows both
  architectures (checked in CI).
- [~] With the secrets: `spctl --assess` accepts the `.dmg` and the app, and a fresh Mac opens it with no warning.
