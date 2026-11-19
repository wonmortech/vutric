# ADR-0007: Every route that works, and the user decides where files go

- Status: accepted (plan). Nothing in this ADR is implemented yet unless the table says so.
- Date: 2026-11-18
- Builds on: [ADR-0005](0005-any-to-any-offline-first.md), [ADR-0006](0006-any-network-any-file.md).
- Research: [transfer-methods.md](../research/transfer-methods.md) (Uzbek). It covers every known way two devices
  find each other and move bytes, with 103 sources. The `[N]` numbers below point to its source list.

## Context

The owner asked for two things on 2026-11-18:

1. Build **everything** the transfer-methods research recommends. Every route must be strong and convenient,
   with no flaws.
2. The user decides **where received files are saved**. This must be fully configurable, with settings on the
   phone **and** on the computer.

The research also sets the honest limits:

- iOS has no Wi-Fi Direct and cannot open a hotspot from an app. It gives apps no AWDL socket and no Bluetooth
  Classic [11][17][34].
- BLE moves 400–600 kbit/s on an iPhone [27].
- iPhone ⇄ Android Wi-Fi Aware does not pair today [7][9].
- Over 90 % of mobile networks are behind carrier NAT [46], so a relay is required.

## Decision

### 1. Routes, in build order

Every route runs the same protocol (docs/protocol.md). A route only changes how the QUIC connection is reached;
resume, hashing, offers and the trust list stay the same everywhere.

| # | Route | Closes | Platforms | Exit criteria |
|---|---|---|---|---|
| R0 | Same network + internet direct/relay (n0) | same Wi-Fi, different networks | all | **done** (spike 0.2) |
| R1 | **Stronger LAN finding**: every address in the QR (link-local IPv6, 169.254, tethering subnets), a unicast probe of the hints, opt-in subnet scan like LocalSend [55]; detect "Local Network denied" on iOS and explain it [30]; detect AP isolation (internet works, no peer answers) and offer R2/R3 | multicast blocked, AP isolation, iOS permission | all | connect with multicast blocked by the router; a clear screen for each failure |
| R2 | **Our own iroh-relay** (ACME TLS, rate limit, on a Vutric domain); n0 only as fallback; route badge "To'g'ridan-to'g'ri / Relay orqali" and the live speed | n0 dependency, honest UI | all | two phones on mobile data transfer through our relay; the badge switches to direct when hole punching succeeds |
| R3 | **Hotspot mode**: an Android phone opens LocalOnlyHotspot; its QR carries SSID + password + ticket; iPhone joins with NEHotspotConfiguration [35]; Windows, Linux and macOS join with their Wi-Fi API; Windows can host too (NetworkOperatorTetheringManager) [37] | no shared network | Android or Windows host, all join | 1 GB with no router, both directions; fallback text for MIUI/HarmonyOS phones without LOHS [34] |
| R4 | **Bluetooth LE for finding and handshake only**: rotating IDs (never contact hashes [12][13]), carries the ticket and hotspot credentials, wakes the other device; payloads ≤ 64 KB (text, link) may go over BLE itself | no network, no QR scan | all | two devices in one room pair with no camera and no network, then the bytes move over R3 |
| R5 | **USB cable mode**: Android via ADB reverse or USB tethering [79]; iPhone via usbmuxd to the app's TCP port (peertalk) [81] | no radio allowed, very large files | Android, iOS ⇄ desktop | 10 GB over a cable; App Review risk for the iOS part checked before shipping |
| R6 | **Wi-Fi Aware within one platform**: Android ⇄ Android (NAN datapath), iPhone ⇄ iPhone (iOS 26+, DeviceDiscoveryUI pairing, `com.apple.developer.wifi-aware`) [1][2] | near, no network, hotel Wi-Fi | Android 8+, iOS 26+ | 1 GB phone ⇄ phone with no router; iOS needs a Network.framework adapter (unverified [1]) |
| R7 | **Wi-Fi Direct**: Android ⇄ Android, Android ⇄ Windows [17][37] | near, no network | Android, Windows | 1 GB; speed shown as measured (20 Mbit/s to 55 MB/s in the field [25][99]) |
| R8 | **Encrypted cloud handoff** (opt-in): encrypted on the device, uploaded to S3-compatible storage; the key stays in the link fragment; expiry and size limit | receiver offline now | all | a file sent at 10:00 is fetched at 18:00; the server never sees plain bytes |
| R9 | **Animated QR** (libcimbar) [73] | air-gapped rooms | desktop encodes, phone decodes | a 10 MB file goes screen → camera (limit 33 MB) |
| R10 | **iPhone ⇄ Android Wi-Fi Aware** | near, cross-platform | iOS 26+, Android with v4 pairing | blocked today [7][9]; re-checked every quarter |

**Not built:**

- Bluetooth Classic: absent on iOS.
- NFC P2P: absent on iOS.
- LoRa: needs extra hardware.
- Ultrasound for bulk data: 8–16 B/s [74]. Ultrasound may later carry a pairing code only.

**"Avtomatik" picks the route** in this order:

1. same network;
2. internet direct;
3. our relay;
4. hotspot, Wi-Fi Aware or cable, offered to the user.

The route picker from ADR-0006 part 2 lets the user force one route.

### 2. Security rules for every new route

- Before authentication, only fixed-size, bounded frames are accepted. Nothing is parsed (protobuf, plist,
  images) before the peer's key is checked [14][15][16].
- Every frame type goes through the same encryption and authentication. This is the Samsung V5 lesson [14].
- The framing and offer parser are fuzzed in CI with a protocol-aware fuzzer [14].
- Every route uses the 6-digit check code from the TLS exporter (ADR-0005).
- The default is that only trusted devices can send. "Everyone can send to me" turns itself off after 10 minutes,
  as AirDrop does [22].

### 3. Honest numbers on screen

The UI shows the **route type** and the **measured speed**, never a promised one. The route types are:

- "To'g'ridan-to'g'ri · LAN" (direct, local network)
- "To'g'ridan-to'g'ri · internet" (direct, over the internet)
- "Relay orqali" (through the relay)
- "Hotspot"
- "Kabel" (cable)

Static text uses only sourced ranges (research section 4.4). Claims such as "faster than AirDrop", "always P2P"
or "100 % offline" are never shown.

### 4. Where files go: the user's storage settings

**Scope.**

- The phone and the computer both have these settings.
- Every value has a safe default.
- All of them fit on one screen.
- Each can be overridden **per device**, from the device detail screen.

| Setting | Default | Options |
|---|---|---|
| Save folder | desktop: `Downloads\Vutric`; iPhone: the Vutric folder in the Files app; Android: `Download/Vutric` | any folder the user picks |
| Split by type | off | photos / videos / documents / archives / audio / other, each to its own folder |
| Subfolder | none | by sending device, by date (`2026-11-18`), or by device + date |
| Photos and videos to the gallery (phone) | on, album "Vutric" | on/off, album name, "also keep a copy in the save folder" |
| Folder tree from the sender | kept | kept / flattened |
| Name conflict | keep both (`name (2).ext`) | keep both / replace / skip / ask |
| Accepting | ask every time | ask / auto-accept from trusted devices / auto-accept up to N MB |
| After receiving | notification | notification / open the file / open the folder / nothing |
| Free-space guard | refuse when less than 1 GB would remain | size in GB |
| Unfinished parts | kept 7 days for resume | 1–30 days, "delete now" |
| History | 90 days | keep / 7 / 30 / 90 days / off |

**Platform rules.**

- **Windows, Linux, macOS:** any writable path, chosen with the system folder dialog.
- **Android:**
  - The folder is picked with SAF `ACTION_OPEN_DOCUMENT_TREE`, and the permission is persisted.
  - The Rust core writes the `.vutric-part` file in the app's own storage. After verification, the app moves the
    file into the SAF tree, or into MediaStore for the gallery.
  - So a file still appears only once it is complete and verified.
- **iPhone:**
  - The default is the Vutric folder in the app container, which the Files app shows.
  - The user may pick any folder in Files, including iCloud Drive or a USB drive.
  - That folder is kept as a security-scoped bookmark, and finished files are moved into it the same way.
  - Saving to the gallery goes through PhotoKit.
- **A missing folder:** if the chosen folder disappears or loses its permission, files fall back to the default
  folder and the user is told once.

**How settings are stored.**

- Each device keeps one typed settings file, owned by `vutric-core`, so every host reads the same rules.
- Changing a setting never moves files that are already saved.
- The desktop settings window and the phone Settings screen show the same groups, in the same order.

### 5. Measurements still owed

From research section 4.5:

- iPhone ⇄ Windows and Android ⇄ Windows on real devices, each over LAN and over a hotspot;
- the share of direct versus relay connections, from opt-in anonymous telemetry;
- LocalOnlyHotspot speed on 2.4 GHz versus 5 GHz.

## Consequences

**Schedule.**

- Phase 1: R1, R2 and the storage settings. Phase 1 does not ship on n0 relays (ADR-0006).
- Phases 1–3: R3 to R5, in the platform order of ADR-0003.
- After Phase 3: R6 and R7.
- Opt-in extras: R8 and R9.
- Watched, not scheduled: R10.

**Settings.** The settings file needs a versioned schema with migrations from the first release.

**Free space on phones.** On Android and iOS, a user-picked folder adds a step: verify in app storage, then
move. Until a file is moved, it needs room in app storage, so a 20 GB file needs 20 GB there. The free-space guard
checks both places.

**Design brief.** The design brief for every screen this ADR adds is kept with the design references, outside
the repo.
