# ADR-0115: macOS Share extension also takes text and links; a Finder Quick Action for files

- Status: accepted for implementation (`apps/macos/Shared/SendRequest.swift`, `Shared/SharedItems.swift`,
  `ShareExtension/`, `QuickAction/`); unit-checked and smoke-tested on CI virtual machines only.
- Date: 2026-11-18
- Stream: W5 (macOS). Extends [ADR-0112](0112-macos-share-extension-and-services.md); wire for text is
  [ADR-0065](0065-companion-messages.md) (`Message::Clipboard`). Daemon side: W2 #29.

## Context

ADR-0112 sent files only. Safari, Notes, Mail and most apps share a page as a URL (often with its title as text) or
a selection as plain text, so "Vutric bilan yuborish" did not even appear for them. ADR-0065 now gives a message for
text: `Clipboard { text }`, 1 byte to 64 KiB, accepted only from a paired device and shown in a copy prompt, never
acted on. Finder also has a faster entry than the Share menu: Quick Actions (context menu, preview pane, Touch Bar).

## Decision

1. **Text in the hand-over URL.** `vutric://send?text=<percent-encoded>`: exactly one `text` item, no `path` items
   with it, 1 byte to 64 KiB of UTF-8 after NUL characters are dropped; anything else is refused by
   `SendRequest(url:)`. The cap is ADR-0065's, so whatever the agent accepts fits one `Clipboard` message.
2. **What a share becomes** (`SharedItems.request()`): if any file URL was shared, files only (as before). Otherwise
   the item texts and the `http`/`https` links, trimmed, de-duplicated, one per line, in the order given. Over
   64 KiB: the extension says "Matn juda uzun" and cancels; nothing is cut silently. Other link schemes
   (`file:` aside) are ignored.
3. **The agent still asks.** The confirmation shows a one-line preview cleaned like ADR-0065's `shown` (control,
   format and bidi-override characters removed, at most 160 characters). A link opened by another app cannot send
   text without that click, as for files (ADR-0112).
4. **How text goes out** (TEMP until W2 #29, request proposed there): the agent sends
   `{"op":"send_text","text":…,"fallback_path":…}`. The daemon sends `Message::Clipboard` to a paired device and the
   file at `fallback_path` to anything else (a guest, LocalSend, Quick Share). The agent writes that file to
   `~/Library/Caches/io.wonmor.vutric/Outbox/Matn <yyyy-MM-dd HH.mm.ss>.txt` before sending and deletes outbox
   files older than a day. Notices (`Message::Notice`) are phone-to-desktop only and are not sent from the Mac.
5. **Finder Quick Action** `VutricQuickAction.appex`: extension point `com.apple.services`, no UI
   (`NSExtensionRequestHandling`), `NSExtensionServiceAllowsFinderPreviewItem` and `…TouchBarItem` on, role
   *Viewer* (it returns no items, so Finder changes nothing), activation rule files only (max 1000), icon a
   template image drawn from the brand mark (ADR-0116). It hands the files over as `vutric://send?path=…`, like the
   Share extension. Sandboxed, read-only access to the files the user picked, no network.
6. The activation rule of the Share extension now also has `NSExtensionActivationSupportsText` and
   `NSExtensionActivationSupportsWebURLWithMaxCount = 20`.

## Measured

| Check | Where | Result |
|---|---|---|
| `SendRequest` text round trip with `+ & = ? #` and a newline; two texts, text mixed with files, empty text, blank text refused; 64 KiB accepted, 64 KiB + 1 refused; the cap counts UTF-8 bytes | `apps/macos/Tests/main.swift`, CI job `app` step "Swift checks" | pass (run 44340716090) |
| Preview removes U+202E and BEL, joins lines, bounds length | same | pass (run 44340716090) |
| `SharedItems`: files win over text; title and link de-duplicated; too long told apart from nothing | same | pass (run 44340716090) |
| Quick Action registered with `pluginkit`; `vutric://send?text=` brings up the question | `packaging/macos/smoke.sh` | pass (run 44340716090) |

## CI evidence

Run [44340716090](https://github.com/wonmortech/vutric/actions/runs/44340716090), job "Vutric.app and .dmg" (`macos-15-intel`): `SendRequest, SharedItems: all checks passed`;
`pluginkit` lists `io.wonmor.vutric.share` and `io.wonmor.vutric.quickaction`; `open vutric://send?text=…` brought
up "Send this text to the phone?" with the cleaned one-line preview (screenshot `send-text-question.png` in the
`vutric-macos-smoke` artifact; English because the runner's locale is English). Not seen on CI: the Share sheet in
Safari or Notes, and Finder's Quick Actions menu (no UI automation on the runner).

## Real-Mac checklist (owner)

- [ ] Safari → Share → "Vutric bilan yuborish" on a page: the question shows the title and the URL.
- [ ] Notes → select text → Share: the question shows the text; 70 KB of text says "Matn juda uzun".
- [ ] Finder → right-click a file → Quick Actions → "Vutric bilan yuborish" (enable it once in System Settings →
      Privacy & Security → Extensions → Finder if it is not listed); the same from the preview pane.
- [ ] With the daemon (W2 #29): text to a paired phone arrives as the copy prompt; to LocalSend as the `.txt`.
- [ ] With the Mac in English, Finder's Quick Actions menu shows "Send with Vutric". With Uzbek first for Finder
      (Language & Region → Applications), or with a language Vutric has no translation for, it shows "Vutric
      bilan yuborish". The preview pane and Touch Bar label is "Vutric" in every language.

Update 2026-11-19 (round 3):
- The Quick Action's nested labels (`NSExtensionServiceFinderPreviewLabel`, `…TouchBarLabel`) are now the brand
  name "Vutric" and are no longer translated: `InfoPlist.strings` is not documented to reach nested keys. The
  translated title is the top-level `CFBundleDisplayName`.
- `packaging/macos/xcstrings.py` refuses an InfoPlist catalog key that is not a top-level key of the sibling
  `Info.plist`.
- `smoke.sh` reads the built `uz` / `en` titles of both extensions with `plutil`.

## Consequences

- W2's daemon gets one more request (`send_text`) with a defined fallback; until then the agent's request is answered
  with an `error` and nothing is sent.
- Images on the clipboard stay out, as in ADR-0065 §5; an image shared from an app arrives as a file.
