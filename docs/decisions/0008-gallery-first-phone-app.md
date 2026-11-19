# ADR-0008: The phone app opens on its own gallery

- Status: accepted, implemented in `apps/mobile-spike` (not yet tried on a real phone).
- Date: 2026-11-18
- Builds on: [ADR-0006](0006-any-network-any-file.md) (any file, many at once), [ADR-0007](0007-every-route-and-storage-settings.md) part 4 (storage settings; this ADR implements the phone side).
- Supersedes: nothing.

## Context

The owner asked (2026-11-18): the app's first screen must be the phone's gallery and files, **inside Vutric**.
You tap as many items as you like, then one button sends them. No detour through the system picker. The spike's
old first screen was a list of measurement buttons, which is a developer tool and not something a user wants.

## Decision

| # | Decision | Why | Where |
|---|---|---|---|
| 1 | The home screen is `GalleryPage` with two tabs: **Galereya** and **Fayllar** | Sending is the main job, so it comes first | `lib/gallery_page.dart`, `lib/main.dart` (`VutricHome`) |
| 2 | Our own grid built on `photo_manager`. Thumbnails come from the OS cache at 200 px, the grid builds only visible cells, and the library loads 120 items per page | Thousands of photos open fast and use little memory | `gallery_page.dart` (`_page`, `_Cell`) |
| 3 | Selection keeps **tap order**, shows a number on each cell, and files are sent in that order | The receiver sees them in the order the user chose | `_picked` (ordered map) |
| 4 | The send button connects first when there is no session: **"Ulanib yuborish · N ta"** | One button and no dead end | `_send()` → `widget.connect()` |
| 5 | One connect sheet with three ways: scan the computer's QR, find on the same Wi-Fi (mDNS), or show my QR | These are the same routes as ADR-0006, in one place | `main.dart` `_connect()` |
| 6 | The device starts once in `VutricHome._boot`. The identity key is shared (`loadIdentity`), and the measurement page reuses the link with no restart | Two starts would break the open session | `main.dart` |
| 7 | Receive, photo throw, **Sozlamalar** and Texnik o'lchov sit in the "⋯" menu | They are secondary jobs and stay reachable | `main.dart` `_menu()` |
| 8 | The Fayllar tab offers files from the system picker, a folder, and **files received through Vutric** | You can forward what you received | `_filesTab()` |
| 9 | An iCloud or cloud original is downloaded only when sending (`originFile`). If it is missing, the item is counted and reported | Nothing breaks silently | `_send()` `missing` |
| 10 | The folder picker uses `getDirectoryPath`, which is deprecated. The deprecation is ignored on purpose | Rust reads only real paths. A `content://` URI from `pickDirectory()` cannot be opened | `_pickFolder()` |
| 11 | Receiving follows the settings. `offer.autoAccept` means no question is asked. The save folder uses the phone's date (`localDate()`). Gallery, album and `keep_copy` come from `settings.json` | ADR-0007 part 4 | `catch_page.dart`, `transfer_ui.dart` |
| 12 | The Sozlamalar screen edits the JSON from `getSettings` and writes it back whole with `putSettings`. Rust rejects invalid JSON and clamps out-of-range values | The schema has one owner (Rust) | `lib/settings_page.dart` |

## Known limits

- On Android, a folder can come back that the app cannot read. The send then fails, and the error shows in the bottom bar. A SAF-based reader is the fix, left for later.
- With limited photo access on iOS, the grid shows only the chosen photos. A banner offers "Ko'proq" to choose more.
- The Sozlamalar screen does not yet edit per-device overrides (`devices`), the save folder path, or `min_free_bytes`.

## Verification

- `flutter analyze`: clean, 0 issues.
- `cargo test -p vutric-core -p vutric-net`: every test passed (30 + 8 + 3 + 1 + 1).
- `flutter build apk --release`: passed (`app-release.apk`, 65.2 MB) after two build fixes on Windows:
  - `android/gradle.properties`: `kotlin.incremental=false`. With the project on D: and the pub cache on C:, the build failed with "Could not close incremental caches".
  - `android/build.gradle.kts`: the plugins' `lintVital*` tasks are off. Lint of `image_picker_android` crashes on JDK 17 because it calls Java 21's `List.removeLast`. That lint checks only plugin code. `flutter analyze` checks the Dart side.
- Not yet tried on a real phone.
