# ADR-0111: On macOS the daemon role is a Swift menu-bar agent that runs the Rust helper

- Status: accepted; implemented in `apps/macos/Agent`, built in CI, not yet run on a real Mac.
- Date: 2026-11-18
- Stream: W5. Builds on [ADR-0009](0009-native-modules-kotlin-swift.md) §4 (an always-on daemon per desktop),
  [ADR-0031](0031-desktop-daemon.md) (the Rust daemon, its IPC and GPU lifecycle).
- Backlog: D11 (macOS daemon role), F7 on macOS.

## Context

On Windows one Rust process does everything resident: tray, toasts, IPC, transfers, overlay (ADR-0031). On macOS
four of those want Apple frameworks that are awkward from Rust and that macOS judges by the **app bundle**, not by
the process:

| Need | macOS API | Tied to |
|---|---|---|
| Menu-bar item, no Dock icon | `NSStatusItem`, `LSUIElement` | the app bundle |
| Start at login | `SMAppService.mainApp` (macOS 13+), listed in System Settings → Login Items | the app bundle |
| Notifications with buttons | `UNUserNotificationCenter` | the bundle id; only bundled apps may use it |
| Local Network privacy prompt (macOS 15+) | `NSLocalNetworkUsageDescription`, attributed to the responsible app | the app that launched the process |
| Share menu, Services menu, URL scheme | Share extension, `NSServices`, `CFBundleURLTypes` | the app bundle |

## Decision

`Vutric.app` is a **Swift menu-bar agent** plus the **Rust helper** inside it:

```
Vutric.app/Contents/
  MacOS/Vutric             Swift agent (this ADR)
  MacOS/vutric-daemon      W2's daemon binary: network, transfers, overlay (ADR-0031, ADR-0110)
  PlugIns/VutricShare.appex Share extension (ADR-0112)
  Resources/{uz,en}.lproj  compiled String Catalogs
```

1. **Agent** (`apps/macos/Agent/Sources/main.swift`): `LSUIElement`, `ActivationPolicy.accessory`. The menu has the
   same items as the Windows tray: Telefonni ulash, Fayl yuborish…, To'xtatib turish / Davom ettirish, Kompyuter
   yonganda ishga tushsin (login item, with a check mark), Sozlamalar (disabled until the desktop window of
   ADR-0030 exists on macOS), Chiqish. A disabled first line shows the state (Tayyor · shu tarmoqda, To'xtatilgan,
   N ta uzatish ketmoqda, or why the helper is not running).
2. **Helper.** The agent starts `Contents/MacOS/vutric-daemon --agent` as its child when nothing answers on the
   socket, so macOS attributes the helper's Local Network use to Vutric.app and shows our
   `NSLocalNetworkUsageDescription`. It stops it on quit.
3. **IPC.** The agent is one more IPC client of the daemon (`vutric_daemon::ipc` v1, JSON lines): `hello`,
   `subscribe`, `status`, `pause`, `show`, `quit` (`DaemonLink.swift`). The socket is
   `~/Library/Application Support/Vutric/daemon.sock`, passed to the helper as `VUTRIC_IPC`; `$XDG_RUNTIME_DIR`,
   which the daemon uses elsewhere, does not exist on macOS.
4. **Login item.** Registered on first launch, the Windows default (autostart on), and switchable from the menu.
   When macOS wants approval in System Settings the agent says where.
5. **Notifications.** "Tushdi" with the Windows toast's buttons, Ochish (opens the folder) and Papkada ko'rsatish
   (Finder selection), from the daemon's `received` event; failures as a plain notification.
6. **Not sandboxed**, Developer ID distribution with hardened runtime (ADR-0113): the helper reads files the user
   sends and writes to any save folder the user picks (ADR-0007 part 4), which the App Sandbox would limit to
   user-selected folders with bookmarks. Mac App Store distribution would need that rework; not planned.
7. **Copy.** Every string is a key in `apps/macos/Shared/Localizable.xcstrings`; development language Uzbek (`uz`),
   English second. `packaging/macos/xcstrings.py` compiles the catalogs and fails the build on any untranslated key.

### What the daemon needs on macOS (W2, issues labelled `needs-w2`)

- #28: listen on `$VUTRIC_IPC` (or `~/Library/Application Support/Vutric/daemon.sock`) on macOS, and a
  `--agent` mode: no tray of its own (`tray_none.rs`), no autostart writing, exit when the agent's connection closes;
- #29: a `send_files { paths }` request for the Share extension, the Services menu and "Fayl yuborish…";
- #30: the overlay on macOS through `vutric-host-mac` (one `cfg` line, ADR-0110), and instant cards on macOS (an
  `NSPanel` card window, which W5 offers to add to `vutric-host-mac`).

Until they land the agent works without a helper and says so (status line: "Qabul qilish xizmati bu versiyada yo'q").

## Consequences

- Two languages on macOS (Swift for ~500 lines of AppKit glue, Rust for everything else). ADR-0009 already puts
  Swift on Apple platforms.
- The agent is built with plain `swiftc` (no Xcode project, ADR-0113), so CI on Windows/Linux is untouched and the
  macOS job builds it in about a minute.
- The Rust daemon stays one codebase for all desktops; macOS only adds a mode and a socket path.
- The desktop windows (settings, history, devices) are Slint, not Flutter (ADR-0030), so there is no Flutter macOS
  runner to add in `apps/desktop/macos/`. Once W2's `vutric-app` window process exists, the agent's "Sozlamalar"
  starts it from `Contents/MacOS/` like the helper; that is a W5 follow-up together with a macOS run of ADR-0030's
  open-time and RAM measurement.

## Real-Mac checklist (owed)

- [~] First launch: menu-bar icon appears, no Dock icon, login item listed in System Settings → Login Items.
- [~] Local Network prompt shows the Uzbek text, names Vutric, and the helper finds a phone after "Allow".
- [~] Notification buttons open the folder and select it in Finder.
- [~] Quit stops the helper (no `vutric-daemon` left in Activity Monitor).
