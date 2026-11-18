# ADR-0119: macOS menu-bar agent: receive history, mirrored notices and clipboard text

- Status: accepted for implementation (`apps/macos/Shared/History.swift`, `Agent/Sources/main.swift`,
  `Agent/Sources/Notifier.swift`); the history model is unit-checked on CI. Receiving notices and clipboard text
  needs two daemon events that do not exist yet (proposed on W2 #30).
- Date: 2026-11-18
- Stream: W5 (macOS). Backlog A10 (history), F12/F13 (clipboard, notifications) on macOS. Wire and rules:
  [ADR-0065](0065-companion-messages.md).

## Context

The Windows daemon shows receipts as toasts; desktop history windows are D4 (W2, Slint, ADR-0030), and none exist
on macOS yet. Phones can now send `Message::Notice` and `Message::Clipboard` to a paired desktop (ADR-0065). On the
Mac the agent owns the user-facing side (menu bar, notifications, questions; ADR-0111), so it is where these show.

## Decision

1. **History in the menu.** "So'nggi qabul qilinganlar" lists the last 10 receipts, newest first: label (file name,
   or "3 ta fayl" for a folder), sender when known, time. Clicking an entry reveals it in Finder. "Ro'yxatni
   tozalash" empties the list. Entries come from the daemon's `received` event (`folder`) and from the LocalSend /
   Quick Share bridge (`received.path`, ADR-0117).
2. **Stored minimally.** The `History` model keeps at most 20 entries (path, label, sender, date) in the agent's
   `UserDefaults`; no file content and no device keys. Entries older than 90 days (the T6 history period) and
   files no longer on disk are not shown. The same path received again moves to the top. Only absolute paths are
   accepted.
3. **Mirrored notices** (`event: "notice"`, fields `from`, `app`, `title`, `text`) become a macOS notification
   grouped per app (`threadIdentifier`). It has no actions and no reply, and opens nothing (ADR-0065 §3). The daemon has already
   cleaned and rate-limited it (`NoticeLimit`, `shown`); the agent cleans the text again before showing it
   (control, format and bidi characters removed, title ≤ 80 and body ≤ 400 characters).
4. **Clipboard text** (`event: "clipboard"`, fields `from`, `text`) opens a question with "Nusxalash" and "Yopish".
   The clipboard is written only on "Nusxalash", never by itself (ADR-0065 §3).
5. Both events are proposals for W2's daemon (posted on #30); until the daemon sends them, these paths are idle.

## Measured

| Check | Where | Result |
|---|---|---|
| `History`: relative paths refused; same path moves to the top; capped at 20; menu shows 10; missing files hidden; expiry at 90 days; saved and loaded through `UserDefaults`; clear | `apps/macos/Tests/main.swift` (CI job `app`, "Swift checks") | passed on `macos-15`, run [44354941609](https://github.com/wonmortech/vutric/actions/runs/44354941609) ("SendRequest, SharedItems, History: all checks passed") |

## Real-Mac checklist (owner)

- [ ] Receive two files by LocalSend (ADR-0117): both appear under "So'nggi qabul qilinganlar"; clicking one
      selects it in Finder; delete one in Finder and it disappears from the menu.
- [ ] With the daemon and a paired phone (W2 #28, #30): a Telegram notification mirrored from the phone appears
      as a macOS notification titled "Telegram: …", with no buttons.
- [ ] Phone sends clipboard text: the question appears; the clipboard is unchanged until "Nusxalash".
