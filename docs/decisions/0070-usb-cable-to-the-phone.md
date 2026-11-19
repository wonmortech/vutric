# ADR-0070: USB cable to the phone (tethering, ADB reverse; iPhone usbmuxd researched only)

Status: accepted (2026-11-18) · Stream: W3 · Backlog: R5 (R5a was ADR-0068) · Builds on: ADR-0052, ADR-0060,
ADR-0068

## Context

A cable is the route that works when nothing else does: hotel Wi-Fi with client isolation, no Wi-Fi at all,
a corporate laptop whose Wi-Fi is locked down. It is also the fastest and most private link. R5a covered two
computers on one cable. R5 is the phone: backlog says "Android ADB reverse / tethering / AOA; iPhone usbmuxd".

## Decision

### 1. USB tethering: the main path, no developer settings needed

Every Android phone can share its connection over USB, and an iPhone's Personal Hotspot works over USB too. The
phone then acts as a router on the cable. The computer gets a DHCP address, and the phone is the gateway.

- `vutric_net::usb::tether_kind(names, driver, mac)` recognises the link. It is pure and tested with fake
  interfaces:

  | Platform | Android | iPhone |
  |---|---|---|
  | Windows | "Remote NDIS based Internet Sharing Device", "UsbNcm Host Device" | "Apple Mobile Device Ethernet" |
  | Linux | `usb0`/`rndis0`, or driver `rndis_host`/`cdc_ncm`/`cdc_ether`/`cdc_eem` with a **locally administered MAC** | driver `ipheth` |

  A USB Ethernet adapter can use the same `cdc_*` drivers. It has a vendor (universal) MAC, though, and a phone's
  gadget never does, so the MAC test keeps an office dongle from being called "phone" (test
  `phone_links_by_name_driver_and_mac`). Linux reads the driver from `/sys/class/net/<if>/device/driver`.
- `lan::Medium::Usb`, a new variant **ranked first**, so the phone that scans the QR over the cable finds the
  cable address in the 8 slots. The badge for it is "Kabel". Nothing outside W3 matches `Medium` exhaustively:
  W1 only compares with `Virtual`, checked by search on 2026-11-18.
- `usb::usb_links()` / `links_from(&[IfaceInfo])` give `UsbLink { iface, name, kind, local, phone }`. `phone` is
  the gateway, plus `192.168.42.129` on the classic Android subnet. Android 11 and later pick a random subnet,
  and the gateway comes from DHCP.
- `usb::find_phone(beacon, link, port, wait)` sends a unicast beacon query to the phone's address (ADR-0052,
  ADR-0059), so **the computer finds the phone by itself** once the cable is in. The answer is unauthenticated, as
  always: pair with the six digits unless the key is already trusted.
- `usb::watch()` polls every 2 s (one OS call) and sends `UsbEvent::Plugged` / `Unplugged`. `usb::changes` is
  the pure diff behind it.

### 2. ADB reverse: for phones with USB debugging

`adb reverse tcp:P tcp:P` makes the phone's `127.0.0.1:P` reach the computer's. It needs no tethering, and
tethering is sometimes blocked by the carrier or by MDM. But **adb carries TCP only, and iroh speaks QUIC over UDP.**

- The computer runs our relay (`vutric-relay`, the same `iroh-relay`) on `127.0.0.1:P`, and both ends meet
  through it. Relayed packets are still end-to-end encrypted QUIC: the relay sees only keys and ciphertext.
- `Session::is_relayed()` is false and `Session::badge()` is `Cable` when the relay is on loopback. So
  `Session::bulk` and the Rb guard in `transfer::send` do not limit big files on this path. This is a change in
  what `is_relayed` reports, not in its signature.
- `usb::adb` (desktop only):
  - `Adb::find()` looks in `PATH`, `ANDROID_HOME`/`ANDROID_SDK_ROOT`, and the Android Studio default folders.
    Vutric never ships `adb`.
  - `devices()`, `reverse(serial, port)`, `remove_reverse`.
  - `open_link(serial, link)`: `am start -a android.intent.action.VIEW -d '<link>'`. The app then opens the
    pairing link and the user scans nothing. It accepts only `vutric://` links with no quote and no space,
    because the phone's shell sees the command.
- The output parser `vutric_core::adb::parse_devices` is pure, bounded (64 KiB, 32 devices, tokens of at most
  64 printable bytes) and fuzzed (`adb_devices`).
- `vutric-net-spike usb` (tethering) and `usb --adb` (relay, reverse, open the link, then `receive`) are the
  reference flows for W2.

### 3. Not built

- **AOA (Android Open Accessory)** would need libusb on the computer, a driver swap on Windows (WinUSB through
  Zadig), and an accessory mode the user must accept. Tethering and ADB cover the same phones with no driver
  install. Dropped from R5.
- **iPhone over usbmuxd (the peertalk approach), researched only.** On macOS and Windows, iTunes or the Apple
  Devices app runs `usbmuxd`, which tunnels TCP to a port the app listens on. peertalk is MIT, and libusbmuxd is
  LGPL (we would talk the documented plist protocol on the socket, without linking it). Open question: App Review
  has accepted peertalk-based apps (screen-mirroring tools), but a general file-transfer app that depends on the
  desktop running Apple's daemon is untested. Guideline 2.5.2 forbids code downloads, not this, but the risk is a
  rejection late in phase 3. An iPhone with Personal Hotspot over USB already works through part 1, with no new
  App Review surface. Owner question added.

## Measured

| Check | Result |
|---|---|
| `usb::tests` (fake RNDIS, NCM, `ipheth`, `cdc_ncm` with local and vendor MAC, Wi-Fi, WSL; Android 11 random subnet; link without DHCP; plug/unplug diff; the phone behind a fake cable answers its beacon on loopback) | 4 pass |
| `vutric-core` `adb::tests` (real `adb devices -l` listing with daemon lines, unauthorized, offline, Wi-Fi debugging, sideload; hostile listings bounded) and `usb::adb::tests::refuses_what_could_escape_the_phone_shell` | pass |
| `crates/vutric-net/tests/adb_fake.rs`: a fake `adb` (batch file on Windows, shell script on Linux) logs the exact `devices -l`, `-s SERIAL reverse tcp:P tcp:P`, `shell am start … -d '<link>'`, `reverse --remove` | pass |
| `crates/vutric-relay/tests/usb_loopback.rs`: phone and desktop meet through a relay on `127.0.0.1` with no LAN address; not relayed, `bulk(20 GiB)` = `Go`, badge `Cable` while on the relay | pass (4.2 s) |
| `lan::tests::badges_follow_the_link`: `Medium::Usb` → `Cable` | pass |
| `vutric-net-spike usb` on this Windows machine with no phone: "no phone on a USB cable"; `usb --adb` without adb: a clear error | by hand |
| fuzz `adb_devices` | stable smoke pass; libFuzzer in CI |

**Not verified:** a real Android phone with USB tethering or adb, a real iPhone over USB, Linux `cdc_ncm` sysfs on
real hardware, and the speed over the cable. Device steps are in W1 #21.

## What W1 does (issue)

- Register `vutric://` as a deep link, so `adb … am start -d` opens the pairing link in the app.
- Answer the beacon on the USB tethering interface: it already binds `0.0.0.0`, but the app must be visible.
- Over adb reverse, the relay in the link is `http://127.0.0.1:P/`. The phone endpoint must dial it with
  `Reach::Anywhere`. Show "Kabel".
- A hint on the "no connection" screen: "USB kabel bilan ulang va USB modemni yoqing".

## Consequences

- W2's daemon runs `usb::watch()` and, on `Plugged`, `find_phone` and a toast "Telefon kabel orqali topildi". ADB
  mode is an advanced setting, off by default.
- `Medium` has a new first variant. Code that sorts by `Medium` puts USB links first.
