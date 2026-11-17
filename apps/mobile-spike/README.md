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
**QR kodni skanerlab oʻlchash** against `vutric-net-spike listen` on the desktop. Discovery through `NWBrowser`
(no entitlement needed) comes with the real iOS client.

## Measure

Phone and desktop on the same Wi-Fi (for the offline run, unplug the router's internet cable first).

| Direction | Desktop | Phone |
|---|---|---|
| phone finds desktop | `vutric-net-spike listen` | **Kompyuterni topish va oʻlchash** |
| desktop finds phone | `vutric-net-spike send --discover` | **Kutish: boshqa qurilma ulansin** |
| multicast blocked | `vutric-net-spike listen`, copy the pairing link | paste it, **Havola orqali ulanib oʻlchash** |

Each run sends 3 warm-up and 100 measured probes one way, then the other way. Both screens show the same
six-digit check code. Pass: one-way p50 < 40 ms and p95 < 80 ms ([roadmap](../../docs/roadmap.md)).
