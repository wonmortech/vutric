# ADR-0116: macOS app icon drawn at build time from the brand mark

- Status: accepted (`packaging/macos/icon/make-icon.swift`, called by `packaging/macos/build-app.sh`).
- Date: 2026-11-18
- Stream: W5 (macOS). Source art: `design/claude-design/logo-mark.svg` (W1's folder, read only) and the site
  colours in `design/claude-design/README.md`.

## Context

Vutric.app had no icon. The repository rule is that no binary is checked in without its source, and the brand mark
already exists as an SVG with two paths. Xcode's asset catalogs are not used (no Xcode project, ADR-0113).

## Decision

1. `make-icon.swift` (run with `xcrun swift`, AppKit and Core Graphics only) reads the SVG's `viewBox` and every
   `<path d>`, accepting only the absolute commands the file uses (M, L, H, V, C, Z); anything else fails the build,
   so a redrawn logo can never be rendered half right.
2. App icon: the macOS grid (824 px tile in a 1024 px canvas, corner radius 185.4, soft drop shadow), tile in sand
   (`#F7F2E9` → `#E6DBC7`, the site's `--sand-50`/`--sand-200`), the V mark in Vutric Blue `#2969FF` shading to
   cobalt `#1A45B8` (`--cobalt-700`) at the bottom. Sizes 16–512 at 1× and 2× into `AppIcon.iconset`, then
   `iconutil` → `Contents/Resources/AppIcon.icns`, named by `CFBundleIconFile`.
3. Quick Action and Touch Bar image: the mark alone, black on transparent (a template image), 32 and 64 px.
4. `preview.png` (1024 px) and the 64 px template are uploaded with the CI smoke artifact for review.
5. The menu-bar item keeps the SF Symbol `paperplane.circle` (ADR-0111) until W1's design has a menu-bar glyph.

## CI evidence

Run [44340716090](https://github.com/wonmortech/vutric/actions/runs/44340716090): `make-icon.swift` read the mark (bounds 4.75, 3.0, 22.5 × 26.02 in SVG units), `iconutil` wrote
`AppIcon.icns` (426 683 bytes); `preview.png` and `QuickActionTemplate@2x.png` are in the `vutric-macos-smoke`
artifact, and the agent's confirmation alert shows the new icon.

## Consequences

- Changing the logo or the colours changes the icon on the next build; the workflow runs when `logo-mark.svg`
  changes.
- An Xcode-generated "Liquid Glass" icon (macOS 26 `.icon`) is not produced; macOS 26 shows the classic icon in a
  glass frame. Revisit if the design gets layered icon art.
