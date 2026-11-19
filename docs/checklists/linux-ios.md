# Phase 4: Linux daemon × iOS client, real-device cross-check (W4)

Roadmap Phase 4: "cross-check of the Phase 2 Linux daemon against the iOS client; nothing new on the protocol side
by design". So this is a test list, not a feature list: every row is something Linux and iOS each do already,
checked together. `[~]` = both sides built, never run together; nothing is `[x]` until it ran on the named devices.

Devices: **iP** iPhone (iOS 18 and 26, a TestFlight build of W1's iOS app), **U46** Ubuntu 24.04 GNOME 46,
**K6** Plasma 6, **F50** Fedora GNOME 50 (the machines of [`linux-desktop.md`](linux-desktop.md)). Network: home Wi-Fi
with the router's WAN unplugged (ADR-0005), then the iPhone's Personal Hotspot.

## Pair and find

| # | Check | U46 | K6 | F50 | What to look at |
|---|---|---|---|---|---|
| 1 | Scan the Linux pairing QR with the iPhone; the six-digit code matches on both screens | [~] | [~] | [~] | iOS Local Network prompt appears once, in Uzbek |
| 2 | iPhone finds the Linux desktop by mDNS (`_vutric._udp`) with `avahi-daemon` running on 5353 | [~] | [~] | [~] | Avahi and our responder share the port; `avahi-browse -a` still works |
| 3 | Linux restarted: the iPhone reconnects without a new QR | [~] | [~] | [~] | identity kept in `$XDG_DATA_HOME/vutric` |
| 4 | Multicast blocked (AP isolation on): connect by the link's IP:port | [~] | [~] | [~] | backlog N3 on Linux |
| 5 | Linux firewall: Fedora's default zone, then `firewall-cmd --set-default-zone=public` + `--add-service=vutric` | – | – | [~] | ephemeral ports need the trusted-zone choice (ADR-0093 §4) |

## Fling and receive

| # | Check | U46 | K6 | F50 | What to look at |
|---|---|---|---|---|---|
| 6 | Fling a photo: Halqa effect on the Linux screen, the card lands, file in `Downloads/Vutric` (localised Downloads name) | [~] | [~] | [~] | GNOME row 5b of linux-desktop.md must pass first |
| 7 | A HEIC photo with "HEIC→JPEG" on and off: the file lands; the notification picture shows (JPEG) or is absent (HEIC without a loader) | [~] | [~] | [~] | `heif-gdk-pixbuf` missing by default on Ubuntu |
| 8 | Live Photo, a 4K video, 200 photos at once from the iOS Share Extension | [~] | [~] | [~] | progress card, one notification at the end |
| 9 | 1 GB file over 5 GHz Wi-Fi and over Personal Hotspot: MB/s, iPhone battery, Linux CPU | [~] | [~] | [~] | numbers into the report |
| 10 | iPhone locked mid-transfer, then unlocked: the transfer resumes or fails with a clear message | [~] | [~] | [~] | iOS background limits |
| 11 | Offer card "Qabul qilasizmi?" accepted with one click while typing in another Linux app | [~] | [~] | [~] | focus never moves (cards ADR-0093 §1) |

## Send to the iPhone

| # | Check | U46 | K6 | F50 | What to look at |
|---|---|---|---|---|---|
| 12 | Nautilus / Dolphin "Vutric bilan yuborish" to the iPhone | [~] | [~] | [~] | needs `vutric-daemon send` (W2, #29) |
| 13 | The file shows in the iOS app and in Photos when it is an image | [~] | [~] | [~] | PhotoKit permission text |

## Other apps on the iPhone (compat, C1)

| # | Check | U46 | K6 | F50 | What to look at |
|---|---|---|---|---|---|
| 14 | LocalSend for iOS sends to the Linux Vutric (compat adapter) and receives from it | [~] | [~] | [~] | the adapter's `/usr/libexec/vutric/vutric-compat` runs with `no_new_privs` |
| 15 | The browser page (C2) from Safari on the iPhone: upload to Linux, download from Linux | [~] | [~] | [~] | no app on the phone |

## Notes

- Quick Share does not exist on iOS; its Linux checks are in [`linux-desktop.md`](linux-desktop.md) rows 30-36 with
  an Android phone.
- Anything that fails here and also fails Windows × iOS is a W1/W3 bug; anything that fails only on Linux is W4's.
