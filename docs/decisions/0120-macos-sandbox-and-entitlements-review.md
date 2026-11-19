# ADR-0120: macOS sandbox and entitlements review

- Status: accepted; checked on every build by `packaging/macos/check-entitlements.sh` (job "Vutric.app and .dmg").
  Not yet checked on a Developer ID build: the Apple secrets are missing, so CI signs ad-hoc (ADR-0113).
- Date: 2026-11-19
- Stream: W5 (macOS). Reviews ADR-0111 (agent), ADR-0112/0115 (extensions), ADR-0113 (signing), ADR-0117 (bridge).

## Context

What the macOS build ships, and what each part does:

| Part | Runs as | Needs |
|---|---|---|
| `Vutric.app` (menu-bar agent, Swift) | Developer ID app, not sandboxed (ADR-0111) | Reads the files the user sends. Writes the save folder and `~/Library/Caches/io.wonmor.vutric/Outbox`. Starts the helpers. Local network for Bonjour and LocalSend multicast (through the helpers). Notifications. |
| `vutric-daemon` (W2), `vutric-mac-compat` (W5), and later `vutric-app` (the Slint windows, ADR-0030) | Helpers in `Contents/MacOS`, started by the agent | The same file and network access; the GPU (Metal) for the overlay. No JIT and no dynamic code. |
| `VutricShare.appex`, `VutricQuickAction.appex` | App extensions, which must be sandboxed | Read the items the user shares, then open `vutric://send?…`. No network. |

The questions:
1. Should the agent be sandboxed (Mac App Store style)?
2. Does any part need a hardened-runtime exception?
3. Do the privacy keys match what the code does?

## Decision

1. **The agent stays unsandboxed** (Developer ID, ADR-0111).
   - A sandboxed agent could not read a file the user picks in another app's Share sheet and then hand it to a
     helper process. Every file would need a security-scoped bookmark, plus an XPC service per helper.
   - It could not write to a save folder chosen in Settings without a bookmark that survives restarts.
   - Its helpers would need `com.apple.security.inherit` and could no longer be plain Rust binaries.
   - Mac App Store distribution is not a goal for 0.x (ADR-0113). If it becomes one, the plan is an XPC split, not
     entitlements bolted onto this design.
2. **No hardened-runtime exceptions anywhere.** The agent and every helper carry no entitlements.
   - They have no `cs.allow-jit`, `allow-unsigned-executable-memory`, `disable-library-validation`,
     `allow-dyld-environment-variables`, `disable-executable-page-protection` or `get-task-allow`.
   - wgpu uses Metal with precompiled pipelines.
   - Slint's software renderer needs no JIT.
   - Nothing loads plug-ins.
   - Developer ID builds sign every Mach-O with `--options runtime --timestamp`, inside out, without `--deep`.
3. **Extensions: exactly two entitlements,** `com.apple.security.app-sandbox` and
   `com.apple.security.files.user-selected.read-only`.
   - No `network.client`: they never talk to the daemon, they open a URL.
   - No app group: nothing is shared through a container.
4. **Privacy keys follow the code.**
   - `NSLocalNetworkUsageDescription` exists in Uzbek in `Info.plist` and in English through `InfoPlist.strings`.
   - `NSBonjourServices` lists exactly the service types the Rust code uses: `_vutric._udp` from
     `vutric_net::MDNS_SERVICE`, and `_FC9F5ED42C8A._tcp` from `vutric_compat_quickshare::endpoint::SERVICE_TYPE`.
   - LocalSend (multicast 224.0.0.167) and KDE Connect (UDP broadcast) use no Bonjour type; the Local Network
     permission covers them.
   - The check script reads the two constants from the sources, so a renamed service fails the build instead of
     silently breaking discovery under macOS 15's Local Network privacy.
5. **The Local Network permission belongs to the agent.**
   - macOS attributes the helpers' network use to the app that launched them (the responsible process), so the
     prompt names Vutric, with the agent's purpose string.
   - This is why the helpers live in `Contents/MacOS` and are launched by the agent, not by `launchd`.

## How it is checked

`packaging/macos/check-entitlements.sh APP`, step "Entitlements and privacy keys (ADR-0120)" in
`.github/workflows/macos-desktop.yml`:
- reads each bundle's and binary's signed entitlements (`codesign -d --entitlements - --xml`);
- compares them with points 2 and 3 above;
- checks the purpose strings and the Bonjour list against the Rust constants;
- when the Developer ID secret is set (`REQUIRE_RUNTIME=1`), checks that every executable has the hardened runtime
  flag.

## Consequences

- A future feature that wants an exception (for example a JIT scripting hook) needs its own ADR. The check script
  fails until that ADR changes it.
- Adding a Bonjour type in Rust without `Info.plist` (or the other way round) fails CI.

## Real-Mac checklist (owner)

- [ ] First launch on macOS 15: the Local Network prompt names "Vutric" and shows the Uzbek or English purpose
      string. After "Allow", LocalSend on a phone on the same Wi-Fi sees the Mac.
- [ ] With "Don't Allow": the agent still starts, and the compat menu says the network is unavailable instead of
      hanging.
- [ ] Developer ID build: `spctl --assess --type execute -vv Vutric.app` says "accepted, source=Notarized
      Developer ID".
