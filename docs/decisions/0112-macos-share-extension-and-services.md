# ADR-0112: macOS sends from the Share menu, the Services menu and `vutric://send`, always confirmed

- Status: accepted; implemented in `apps/macos/ShareExtension` and the agent; built in CI, not run on a real Mac.
- Date: 2026-11-18
- Stream: W5. Builds on [ADR-0111](0111-macos-menu-bar-agent.md); the macOS counterpart of D5 (Explorer menu
  "Vutric bilan yuborish") and F1 (send from the Share menu).
- Backlog: F1 (macOS part), D11.

## Decision

Three ways to hand files to Vutric on macOS, all ending in the same place:

| Entry | Where the user sees it | Implementation |
|---|---|---|
| Share extension "Vutric bilan yuborish" | Share menu of Finder, Preview, Photos (exported files) and any app that shares files | `apps/macos/ShareExtension`, `com.apple.share-services`, up to 1000 files |
| Services entry "Vutric bilan yuborish" | Finder right-click → Services (and Quick Actions → Customize lists it) | `NSServices` in the agent's Info.plist, `sendFiles(_:userData:error:)` |
| `vutric://send?path=…&path=…` | other apps, scripts, the Share extension itself | `CFBundleURLTypes`, `SendRequest` (`apps/macos/Shared/SendRequest.swift`) |

- **The Share extension is sandboxed and has no network.** It collects the shared file URLs, builds a
  `vutric://send` URL and opens it; the agent (not sandboxed, ADR-0111) reads the files and passes their paths to
  the daemon (`send_files`, `needs-w2`). No App Group is needed, so this also works in the unsigned build.
- **Every hand-over that does not come from our own menu is confirmed** by the agent: "N ta faylni Vutric bilan
  yuborasizmi?" with the first five names, Yuborish / Bekor qilish. A link opened by another app therefore cannot
  send anything silently; it can at most show this question.
- `SendRequest` accepts only our scheme and host, only absolute paths, resolves `..`, and refuses more than 1000
  files; `apps/macos/Tests/main.swift` checks that (run in CI).
- Bonjour and Local Network: the agent's Info.plist lists `_vutric._udp` (our iroh mDNS service, `vutric-net`
  `MDNS_SERVICE`) and `_FC9F5ED42C8A._tcp` (Quick Share, `vutric-compat-quickshare`) in `NSBonjourServices`, and the
  Uzbek `NSLocalNetworkUsageDescription`. LocalSend's multicast (`224.0.0.167:53317`) and KDE Connect's broadcast need
  no Bonjour entry, but they are covered by the same Local Network permission on macOS 15+. macOS needs no multicast
  entitlement (that is iOS's `com.apple.developer.networking.multicast`).

## Not done, and why

- **A Finder "Quick Actions" item of our own** (an Action extension with `NSExtensionServiceAllowsFinderPreviewItem`)
  is left for after the real-Mac check: the Services entry and the Share menu already cover Finder's context menu,
  and a second extension doubles the signing and review surface. If the owner wants it in the Quick Actions submenu
  itself, it is a copy of the Share extension with another extension point.
- **Items that are data, not files** (a photo dragged out of Photos without exporting, text) are refused for now
  ("Yuboriladigan fayl topilmadi"); they would need the extension to write a temporary file into a shared container.

## Seen in CI (`packaging/macos/smoke.sh`, run 44329647191, macOS 15 on the Intel runner)

Raw: [`../measurements/2026-11-18-w5/ci-smoke.txt`](../measurements/2026-11-18-w5/ci-smoke.txt).

- The unsigned, ad-hoc signed agent launches and stays up; `pkill` ends it.
- `pluginkit` lists `io.wonmor.vutric.share` 0.0.1 from `Vutric.app/Contents/PlugIns` (the Share extension is
  registered).
- `pbs -dump_pboard` lists the "Vutric bilan yuborish" Services entry.
- `open 'vutric://send?path=/etc/hosts&path=/etc/shells'` brings up the question with both names
  ([screenshot](../measurements/2026-11-18-w5/ci-smoke-send-question.png)), in English because the runner's
  language is English. The menu bar still showed Finder: macOS 15's cooperative activation did not make the
  accessory agent frontmost, but the alert was on top. The script's System Events window check printed nothing,
  because the runner gives System Events no accessibility access; the screenshot is the evidence.

## Real-Mac checklist (owed)

- [~] Finder → Share → "Vutric bilan yuborish" with 1, 10 and 200 files: the agent asks, then the daemon sends.
- [~] Finder → Services → "Vutric bilan yuborish": the same.
- [~] `open 'vutric://send?path=/etc/hosts'` from Terminal: the question appears, nothing is sent on Bekor qilish.
- [~] English macOS: the extension and the menu read "Send with Vutric".
