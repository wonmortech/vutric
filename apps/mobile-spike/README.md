# Mobile spike (Phase 0.2): Android and iPhone

A minimal Flutter app around `crates/vutric-mobile`, the same Rust peer the desktop runs
(`crates/vutric-net`, [ADR-0005](../../docs/decisions/0005-any-to-any-offline-first.md)). It measures a 30 KB
preview in both directions over the local network only; nothing goes to the internet.

## Build

Toolchain: Flutter 3.50 stable, JDK 17, Android SDK 36, NDK 28.2.13676358, `cargo-ndk` 4.1,
`flutter_rust_bridge_codegen` 2.13.0, Rust targets `aarch64-linux-android` and `x86_64-linux-android`.

```powershell
# from the repo root: the Rust library, into jniLibs (not committed)
cargo ndk -t arm64-v8a -t x86_64 --platform 24 -o apps/mobile-spike/android/app/src/main/jniLibs build -p vutric-mobile --release

# after changing crates/vutric-mobile/src/api.rs
cd apps/mobile-spike
flutter_rust_bridge_codegen generate

flutter build apk --release --target-platform android-arm64,android-x64
adb install -r build/app/outputs/flutter-apk/app-release.apk
```

## iPhone without the App Store

The [iOS spike workflow](../../.github/workflows/ios-spike.yml) builds an **unsigned** `vutric-spike-unsigned.ipa`
on a macOS runner (artifact `vutric-spike-ios`). Sign and install it from Windows or macOS with
[Sideloadly](https://sideloadly.io) and your own Apple ID (a free ID works; the app then runs for 7 days and must
be reinstalled). On the iPhone: Settings → Privacy & Security → Developer Mode on, then trust your Apple ID under
Settings → General → VPN & Device Management, and allow **Local Network** on first launch.

mDNS needs Apple's multicast entitlement, which a free Apple ID cannot get, so on iPhone pair with
**QR kodni skanerlab oʻlchash** against `vutric-net-spike listen` on the desktop. The Share Extension and App
Intents browse `_vutric._udp` with `NWBrowser` (no entitlement needed) to dial a desktop on the same Wi-Fi directly.

### Share Extension, widget, Live Activity, Shortcuts (ADR-0015)

The same artifact also holds `vutric-spike-adhoc.ipa`: the same app with the App Group
(`group.io.wonmor.vutricSpike`) and hotspot entitlements in its signatures. The Share Extension, the widget and App
Intents read the pairing from that App Group, so without it they say "open Vutric once".

1. In Sideloadly, choose `vutric-spike-adhoc.ipa`. Under **Advanced options** keep the app's own bundle id
   (`io.wonmor.vutricSpike`) and leave "Remove app extensions" **off**; if it offers to sign app extensions or keep
   entitlements, turn that on.
2. Install. Sideloadly registers three App IDs (`io.wonmor.vutricSpike`, `.share`, `.widgets`); a free Apple ID has
   a small weekly App ID allowance, so reinstalling with other ids can fail for a few days.
3. If the install fails or the extensions are missing, install `vutric-spike-unsigned.ipa` instead and write down
   what Sideloadly said about App Groups and Hotspot Configuration (owner question in `docs/owner-questions.md`).
4. Open Vutric once and pair a computer; then go through the device checklist in
   [ADR-0015](../../docs/decisions/0015-ios-native-modules.md).

On a Mac the extensions build from Xcode after `tool/ios_bindings.sh` (it builds the Rust library for iPhone and
writes the UniFFI Swift bindings into `ios/Generated/`, which is not committed); set your own team and App Group
in Signing & Capabilities for all three targets, and `VUTRIC_APP_GROUP` in the project's build settings to match.

## Measure

Phone and desktop on the same Wi-Fi (for the offline run, unplug the router's internet cable first).

| Direction | Desktop | Phone |
|---|---|---|
| phone finds desktop | `vutric-net-spike listen` | **Kompyuterni topish va oʻlchash** |
| desktop finds phone | `vutric-net-spike send --discover` | **Kutish: boshqa qurilma ulansin** |
| multicast blocked | `vutric-net-spike listen`, copy the pairing link | paste it, **Havola orqali ulanib oʻlchash** |

Each run sends 3 warm-up and 100 measured probes one way, then the other way. Both screens show the same
six-digit check code. Pass: one-way p50 < 40 ms and p95 < 80 ms ([roadmap](../../docs/roadmap.md)).

## Throw a photo (Phase 0 end to end)

On the desktop, from a terminal on the desktop session (the overlay needs a window):

```sh
cargo run --release -p vutric-overlay-spike -- --listen --name "Studio PC"
```

On the phone: **Rasmni kompyuterga otish** → scan the QR code (or **Tarmoqdan topish** on Android) → pick a photo
→ flick it upwards. Faster than 700 px/s counts as a throw; slower drags spring back. The photo (a JPEG preview,
longest side 512 px) flies onto the desktop through the Halqa effect.

The phone shows *release → first frame on the computer*, measured there and back (so an upper bound); the
desktop prints *arrived → first frame* for the same throw. Pass: under 100 ms ([roadmap](../../docs/roadmap.md)).
Without a phone, `vutric-net-spike fling --ticket LINK --photo PATH` throws a photo from another desktop.

## Catch a photo (desktop → phone, phone → phone)

**Rasm qabul qilish** on the phone, then either:

- desktop → phone: run `vutric-net-spike fling --photo PATH --count 3` (it prints a QR code), and on the phone
  tap **Kompyuter QR kodini skanerlash**. Works on iPhone too, since nothing has to find the phone over mDNS;
- phone → phone: tap **Kutish: QR kodimni skanerlashsin**; on the other phone, **Rasmni kompyuterga otish** →
  scan this QR code → throw.

The photo flies in from the top edge where the thrower let go, with its tilt. The phone shows how long the photo
took to reach the screen; the thrower gets its round trip as usual.
