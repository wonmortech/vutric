# ADR-0011: The phone UI follows the Claude Design handoff

- Status: accepted, implemented in `apps/mobile-spike` (stream W1). `flutter analyze` is clean, `flutter build apk
  --release` passes, but nothing has been tried on a real phone yet (see the device checklist below).
- Date: 2026-11-18
- Builds on: [ADR-0008](0008-gallery-first-phone-app.md) (gallery first), [ADR-0006](0006-any-network-any-file.md).
- Amends: ADR-0008 decision 1 (the home screen is now the Otish tab of a three-tab shell, not `GalleryPage`).

## Context

The owner asked (2026-11-18) for the Claude Design handoff (`design/claude-design/Vutric App v4.dc.html`) to be
"perfected and used", and for every piece of work to be done at the highest level: fastest, lightest, best UI/UX
and design, most functional, most adaptable. The handoff draws S1 Home, S2 scanner, S3 Send (9 states), S4
Receive (5 states), S6 themes, and 9 themes in 3 materials. ADR-0008 already decided that the phone opens on its
own gallery. The handoff's S1 instead shows a big hero panel with "Rasm otish" and "Qabul qilish". This ADR
reconciles the two and records how the design became Flutter code.

## Decision

| # | Decision | Why | Where |
|---|---|---|---|
| 1 | The design tokens are generated from the handoff HTML, never typed by hand. A script parses the `THEMES` array (26 values per theme) into `const vutricThemes` | One source of truth, so a new design drop is a script run | `tool/gen_themes.py` → `lib/design/themes.g.dart` |
| 2 | `VutricTokens` is a `ThemeExtension` read as `context.v`. Text styles come from the tokens. Geist and Geist Mono ship as variable TTFs (SIL OFL), and the weight is set through the `wght` axis | Exactly the handoff's typography on Android; iOS keeps SF where the theme says `system` | `lib/design/tokens.dart`, `assets/fonts/` |
| 3 | The 3 materials (glass, outline, brutal) are drawn by one `VSurface`, so every card, bar and sheet changes with the theme | 9 themes without 9 widget sets | `lib/design/widgets.dart` |
| 4 | **Shell:** three tabs (Otish · Qabul · Tarix) in the floating glass tab bar, plus a round side button (QR when idle, the peer when connected) | The handoff's tab bar, with history added as backlog A10 | `lib/screens/home_shell.dart` |
| 5 | **Gallery first is kept.** The Otish tab is S1's title ("Nimani yuboramiz?") and a compact dark "Rasm otish" panel, then the Rasmlar / Fayllar segment and the gallery grid from ADR-0008. A floating "Yuborish · N" bar appears with the first selection; with exactly one photo selected it also offers "Otish" | ADR-0008's owner decision stays, and the design's hero becomes one tap rather than the whole screen | `lib/screens/send_tab.dart` |
| 6 | **S3 throw:** all 9 states in one page with the handoff's numbers. Throw at −150 px or 500 px/s; out 280 ms `cubic(.3,.7,.4,1)`; return 560 ms `cubic(.34,1.56,.64,1)`; result pill 220 ms with an 8 px rise; card tilt `dx·0.05° − up·0.02°`, shrinking up to 14 %, portal glow `min(up/170, 1)` | The feel is the product (ADR-0005), so it is taken literally | `lib/screens/throw_page.dart` |
| 7 | **S4 receive:** choice → waiting (my QR) → connected and waiting → photo landed (flies in from where it left the thrower, carrying its tilt; "ekranda N ms") → connection lost (Qayta ulanish). Save to the gallery and share are on the landed photo | The handoff's 5 states | `lib/screens/receive_tab.dart`, `lib/screens/connect.dart` |
| 8 | **One incoming loop for the whole app.** `Session` waits for photos and file offers on any tab and hands them to the Qabul tab. Waiting holds the session, so every send runs through `Session.exclusive`, which calls the new `pause_incoming()`, waits for the loop to stop, sends, then `resume_incoming()` | Without it, sending while the receive wait held the session lock was a deadlock. `recv` is cancel-safe (buffered decoder), so nothing is lost | `lib/session.dart`, `crates/vutric-mobile/src/api.rs` |
| 9 | **UI-only preferences stay on the Flutter side:** theme and device name in `ui.json`, history (last 200) in `history.json`, both in the app support folder. Everything about transfers stays in Rust's `settings.json` (ADR-0008 decision 12) | Rust's `Settings` serde drops unknown keys, and these values never leave the phone UI | `lib/session.dart` |
| 10 | **Theme "Avto"** (default) follows the phone's light/dark setting with the iOS Native pair (`ios` / `ios-dark`); any of the 9 can be pinned in Sozlamalar (S6, each shown as a tiny phone in its own colours) | A native look out of the box; the brand themes are a choice | `lib/main.dart` (`themeFor`), `lib/settings_page.dart` |
| 11 | **Reduced motion** (`MediaQuery.disableAnimations`) replaces every throw, landing, pulse and scan sweep with a 200 ms fade or a still frame | Accessibility; a rule in the handoff | every screen, `context.reducedMotion` |
| 12 | The latency measurement moved from the home screen into Sozlamalar → Texnik | A developer tool, not a user screen | `lib/screens/measure_page.dart` |
| 13 | Renaming the device restarts the node, so the pairing link carries the new name. Renaming is off while connected | The name is in the link (`n=`) | `Session.restart()` |

Removed: `gallery_page.dart`, `fling_page.dart`, `catch_page.dart` (ported into the screens above), and the old
colour constants in `ui.dart`.

## Verified

- `flutter analyze`: no issues.
- `cargo clippy -p vutric-mobile --all-targets -- -D warnings`: clean. `cargo test -p vutric-mobile`: passed.
- `cargo ndk ... build -p vutric-mobile --release`, then `flutter build apk --release --target-platform
  android-arm64,android-x64`: passed, `app-release.apk` 76.9 MB (65.2 MB before; most of the growth is the two
  bundled fonts plus `share_plus`).

## Device checklist (not yet run)

1. First start: the gallery permission prompt appears; with "limited" access the notice shows and opens the picker.
2. All 9 themes plus Avto switch live; dark themes give light status-bar icons.
3. Select 3 photos and 1 file → "Ulanib yuborish · 4" → scan the desktop QR → progress → history row.
4. Throw: a drag below the threshold springs back; a throw lands on the desktop; the result pill shows ms.
5. Desktop throws to the phone while it sits on the Tarix tab: the app jumps to Qabul and the photo lands.
6. Send while the desktop is also sending: no hang (decision 8).
7. Turn the desktop's Wi-Fi off: the "Aloqa uzildi" state appears, and Qayta ulanish works.
8. Rename the device → the QR shows the new name on the other side.
9. System "remove animations" on: no throw or landing motion, only fades.
10. Received photo: Saqlash puts it in the gallery album; Ulashish opens the share sheet.

## Consequences

- The design can be refreshed by dropping a new HTML into `design/claude-design/` and running `tool/gen_themes.py`.
- `Session` is now the only owner of the connection state; new screens listen to it rather than passing peers.
- Open: onboarding (A2), the Kotlin modules (A3–A8), and the device run above.
