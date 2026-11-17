# ADR-0003: Platform release order

- Status: accepted (supersedes the phase order in `docs/research/original-brief.md`)
- Date: 2026-11-17

## Context

The brief planned Android→Windows, then Ubuntu, then iOS, then "more". The owner later fixed the order explicitly
and added macOS as the final target.

## Decision

1. Windows 10/11 + Android
2. Ubuntu (GNOME, KDE) + Android
3. Windows + iOS
4. Linux + iOS
5. macOS (last), with both phones

## Consequences

- The protocol and core must never assume an Android peer; iOS joins in phase 3 without protocol changes.
- The overlay host trait is designed now with a macOS backend in mind (`NSPanel`, non-activating), even though it
  ships last, so phases 1–4 do not paint us into a Win32/Wayland-only corner.
- Store and packaging work follows the same order (MSIX/winget → .deb/PPA → TestFlight → … → notarised dmg).
