# ADR-0006: Any network, the user's choice of route, any file

- Status: accepted.
  - Parts 1 and 3 are implemented in spike 0.2 (see `docs/protocol.md`). Part 3 measured 203 MB, 2 files, in
    4.4 s, 46.6 MB/s, on loopback with a debug build.
  - Part 3 still waits for: a desktop drag-and-drop UI, folder pick on the phone, and the stack effect.
  - Part 2 is next.
- Date: 2026-11-18
- Supersedes in part: ADR-0005 decision 2 ("relay is a later opt-in mode").

## Context

The owner tested spike 0.2 on an iPhone on 2026-11-18 and widened the brief again:

- It must work in every case, also when the two devices are **not on one network**.
- The user **chooses how** to send and receive:
  - internet, with a list of devices;
  - with no internet, Bluetooth and the like;
  - every way must be strong.
- Not one photo at a time but **many from the gallery**: video, folders, documents, any format, both ways between computer and phone.

## Decision

### 1. Reach: local first, internet when needed (implemented)

| Route | When it is used | Speed | Privacy |
|---|---|---|---|
| Same network, direct | both on one Wi-Fi or LAN | full link speed | never leaves the network |
| Internet, direct | different networks, NAT hole punching succeeds (most home and mobile networks) | the slower uplink of the two | end-to-end QUIC + TLS, no server sees content |
| Internet, relay | hole punching fails (strict or carrier NAT) | relay-limited | still end-to-end encrypted; the relay forwards ciphertext only |

- `Reach::Anywhere` is the default: iroh's n0 relays and pkarr lookup. The pairing link carries the relay URL (`r=`), so a device on another network is reached by its key.
- `Reach::Lan` (`--lan-only`, app setting) is ADR-0005's offline mode.
- **The device name is never published.** Lookup records carry only the key and the relay URL: the `Nameless` wrapper strips `user_data`. Pkarr publishes no IP addresses (relay-only filter).
- The UI shows the route of the open session ("to'g'ridan-to'g'ri" / "internet orqali (shifrlangan)"). iroh upgrades relay to direct by itself when it can.
- Later: our own relay at a Vutric domain, so no third-party server is in the path.

### 2. The user chooses the route (next)

The Settings screen and the send sheet get a route picker:

| Choice | Meaning |
|---|---|
| Avtomatik (default) | local first, then internet, as in 1 |
| Faqat shu tarmoq | `Reach::Lan`, nothing touches the internet |
| Internet orqali | to a remembered device anywhere; the device list shows which are online |
| Tarmoqsiz (Hotspot) | ADR-0005 decision 4: one device opens a local hotspot, the other joins from the QR |
| Bluetooth | discovery and pairing only, see below |

**Bluetooth, honestly.** BLE carries well under 1 Mbit/s in practice. A 12 MP photo would take tens of seconds, and a video would take minutes to hours. iOS also gives apps no Bluetooth Classic data channel. So Bluetooth is used for what it is good at:

- finding a nearby device with no network at all;
- waking it;
- exchanging the pairing ticket and hotspot credentials.

The bytes then move over the Wi-Fi link Bluetooth set up. That is the same split AirDrop and Quick Share use. Payloads under about 64 KB (text, a link, a contact) may go over BLE directly.

**Remembered devices.** After the first QR, both sides keep each other's key in the trust list. The device list then works without a QR:

- devices on the network come from mDNS or Bonjour;
- devices elsewhere are reached by key through lookup and the relay.

### 3. Any file, many at once (next)

- **Selection.**
  - Phone: multi-select from the gallery (photos and videos), the Files app or the document picker (any type), and the share sheet.
  - Desktop: drag files or folders onto the Send Zone, or use "Send with Vutric".
- **Wire.** One `Offer` message lists the entries: relative path, size, MIME type, a BLAKE3 hash, and a thumbnail for media. The receiver accepts the whole offer or part of it.
- **Data.** Each file streams on its own QUIC stream in 1 MiB chunks, up to 4 at once, with no whole-file buffering on either side. Progress is per file and in total.
- **Resume and verify.** After a drop the receiver asks for missing ranges only. Each file is hashed when it completes and is written under a `.part` name until then. A folder keeps its tree; names go through `vutric-core::filename` sanitising.
- **The effect.** The Halqa throw animates the first item. The rest show as a stack with a counter.

## Consequences

- The privacy line on screen A1 changes from "Hech narsa internetga chiqmaydi" ("nothing goes to the internet") to "Fayllar shifrlangan, faqat siz va qabul qiluvchi ko'radi" ("files are encrypted; only you and the receiver see them"). The Lan-only setting brings the old promise back.
- Using n0's public relays is a dependency until our own relay exists. Spike 0.2 accepts that; Phase 1 does not ship without our relay.
- Part 3 replaces the single-photo `FlingStart` path. The photo throw becomes the one-file case of an offer.

## Sources

- [iroh: holepunching and relays](https://www.iroh.computer/docs/concepts/relay)
- [Apple Core Bluetooth: BLE only for third-party apps](https://developer.apple.com/documentation/corebluetooth)
- [Android: Wi-Fi Direct](https://developer.android.com/develop/connectivity/wifi/wifi-direct)
