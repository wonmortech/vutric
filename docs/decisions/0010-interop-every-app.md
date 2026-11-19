# ADR-0010: Speak every open file-sharing protocol, so anyone can send to Vutric without installing it

- Status: accepted by the owner (2026-11-18): **all** adapters below, each shipped with its platform's phase.
- Date: 2026-11-18
- Builds on: [ADR-0003](0003-platform-order.md) (order), [ADR-0007](0007-every-route-and-storage-settings.md)
  (routes, security rules, storage), [ADR-0009](0009-native-modules-kotlin-swift.md) (layers).
- Research: [transfer-methods.md](../research/transfer-methods.md) sections 1c and 3 (`[N]` = its sources),
  [airdrop-alternativa.html](../research/airdrop-alternativa.html) and
  [vutric-stack-tavsiya.html](../research/vutric-stack-tavsiya.html) (Uzbek).
- Supersedes: the roadmap line "LocalSend protocol compatibility" under "Later", which this ADR schedules.

## Context

The owner wants every method on the market in one app: "localsend ham boshqalar ham … hammasi bo'lishi kerak".

ADR-0007 already lists our **own** routes, R0 to R10. Nothing yet covers **other apps' protocols**. Speaking those
protocols brings in a large group of users: people who already have Quick Share (every recent Android phone and
Windows PC), LocalSend (99.4k GitHub stars [55]), KDE Connect/GSConnect, Warpinator (Linux Mint), croc or
magic-wormhole. Each of them can send to a Vutric device **without installing Vutric**. No competitor reaches all of
these groups.

## Decision

### 1. The adapters

Each adapter is a crate `vutric-compat-<name>`. It maps the foreign protocol onto the same internal transfer
pipeline:

- offer → accept → save;
- the storage rules of ADR-0007 part 4;
- history;
- the overlay effect.

A file that arrives through LocalSend gets the same Halqa, folder rules and gallery copy as a Vutric file.

| Adapter | Protocol (source) | Direction | Where it runs | Phase |
|---|---|---|---|---|
| **LocalSend** | v2.1, open spec, REST over HTTPS, multicast `224.0.0.167:53317` + HTTP subnet scan, PIN [55] | send + receive | desktop daemon, Android | **1** (Windows + Android); Linux in 2; iOS in 3; macOS in 5 |
| **Browser (no app)** | our own small HTTPS page on the LAN, reached by QR or link; upload and download, LocalSend "Web share" / qrcp style | send + receive | desktop daemon (later phone) | **1** |
| **Quick Share, LAN mode** | Google Nearby Share/Quick Share as documented in NearDrop's `PROTOCOL.md` (UKEY2 + protobuf frames, mDNS) [17] | receive first, then send | desktop daemon | **1** (Windows), **2** (Linux, the biggest gap there), **5** (macOS, shows a QR as NearDrop does) |
| **KDE Connect** | KDE Connect protocol v8: UDP 1716 identity, TCP + TLS, share plugin only [66] | send + receive files | desktop daemon (Linux first), Android | **2** |
| **Warpinator** | Linux Mint Warpinator: mDNS + group code, gRPC [68] | send + receive | desktop daemon | **2** |
| **croc** | croc relay protocol: code phrase → PAKE → E2E [56] | send + receive | desktop daemon, CLI | **2** |
| **magic-wormhole** | wormhole mailbox + transit protocol, PAKE, NaCl secretbox [57] | send + receive | desktop daemon, CLI | **2** |

**Interop on the phone:**

- **Android** gets LocalSend and KDE Connect in the app, through a Kotlin `ForeignDiscovery` module that uses the
  same `NsdManager` and `MulticastLock` as R1.
- **iOS** gets LocalSend only. It must stay in the foreground (ADR-0009 part 3 limits).

Before each adapter is written, its protocol's **current** version is read again from the source repository, and
the version is noted in the adapter's README. The details in this table come from 2026-11-18 research.

### 2. What is not possible, and what we take instead

| System | Why not | What we take instead |
|---|---|---|
| AirDrop | runs on AWDL, which Apple closes to third parties. OpenDrop needs a monitor-mode Wi-Fi card [11][93] | an iPhone sends from Safari through the **browser adapter**, or installs Vutric |
| Apple Continuity / Universal Clipboard | not open to third parties | clipboard over our own session (ADR-0009 `Clipboard`) |
| Samsung Quick Share (Wi-Fi Direct), Huawei Share, Mi Share alliance (Xiaomi/OPPO/vivo) | closed, brand-only [23][24] | their LAN fallback where it exists. Wi-Fi Direct is our own R7 |
| Xender, SHAREit, Zapya, Send Anywhere, Blip, Feem | closed protocols. SHAREit had serious flaws [86] | their best idea, "no internet, the phone opens a hotspot", is our R3 |
| Phone Link, Windows Nearby Sharing | closed | the Vutric desktop daemon plays this role |
| Snapdrop / PairDrop | browser apps on their own servers | our browser adapter covers the "no app" case without a third-party server |
| Syncthing, OnionShare, Briar, Tailscale/Taildrop | different products (sync, anonymity, messaging, VPN) | lessons only: relay transparency (Syncthing), many transports and one protocol (Briar), peer relay (Tailscale) |

### 3. Rules for every adapter

1. **Clean-room from the protocol, never copied code.** Vutric is Apache-2.0. rquickshare, KDE Connect, Warpinator
   and Dukto are GPL, so we do not copy their code. We implement from the published protocol and our own
   captures. NearDrop (Unlicense) and LocalSend (Apache-2.0) documents may be followed directly. A Rust crate for
   a protocol (`localsend`, `kdeconnect-proto`) is used only after its licence and code have been checked.
2. **A foreign peer is less trusted than a paired Vutric device:**
   - every offer from a foreign protocol is **asked about**, and never auto-accepted, unless the user pins that
     peer's fingerprint;
   - the UI shows the protocol ("LocalSend orqali", "Quick Share orqali").
3. **Visible only on purpose.**
   - A foreign receiver advertises itself only while "receive mode" is on: tray toggle, Android tile.
   - "Everyone" visibility switches itself off after 10 minutes, as AirDrop does [13][22].
   - Each adapter can be turned off in Settings.
4. **Pre-auth parsing is isolated.** Quick Share and others parse protobuf before authentication. This is the class
   of bug behind QuickShell and Protocol Prying [14][15][16]. So:
   - each adapter that parses before authentication runs in its **own low-privilege process**;
   - that process talks to the daemon over IPC;
   - size limits apply before allocation;
   - a protocol-aware fuzzer runs in CI for every adapter's frame parser.
5. **Speeds are measured, not promised** (ADR-0007 part 3). The route badge names the adapter.
6. **Each adapter is tested against the real app.** For every adapter, CI or a scripted run:
   - sends to the real LocalSend, Quick Share, KDE Connect, Warpinator, croc or wormhole client;
   - receives from it;
   - records the version it was tested against.
   A protocol change upstream then shows up as a failing test, not as a user report.

### 4. Settings additions (ADR-0007 part 4 schema)

`compat: { localsend, browser, quickshare, kdeconnect, warpinator, croc, wormhole }`. Each entry has `enabled`
(defaults: LocalSend and browser on; the others off until the user turns them on, because they advertise on the
network) and `pinned` fingerprints.

## Consequences

- Vutric becomes the only receiver that every common sender can reach:
  - Android → Quick Share;
  - iPhone → browser;
  - Linux → KDE Connect or Warpinator;
  - power users → croc or wormhole;
  - anyone → LocalSend.
- Seven adapters are seven protocols to keep up with. The real-app tests in rule 6 are what makes that affordable.
- Quick Share is reverse-engineered. Google can change it, and the adapter may then stop working until it is
  updated. It is shipped labelled "eksperimental" until it has survived two Quick Share releases.
- The daemon grows by one sandboxed process per pre-auth adapter. Each adapter's process starts only while that
  adapter is enabled and receive mode is on, so it adds nothing to idle RAM (ADR-0009 target ≤ 30 MB).
