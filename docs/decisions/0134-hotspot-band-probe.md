# ADR-0134: M3 hotspot band probe on Android

- Status: accepted, implemented as a debug-only probe (stream W6). Compiles and its JSON passes the unit tests;
  no real-phone run yet, so no numbers yet.
- Date: 2026-11-18
- Builds on: [ADR-0007](0007-every-route-and-storage-settings.md) §5, [ADR-0058](0058-hotspot-in-the-pairing-link.md),
  [ADR-0012](0012-android-modules-phase-1.md) decision 6.
- Backlog: M3 (the measurement row is W3's; W6 builds the Android tool that produces it).

## Context

M3 asks how much a LocalOnlyHotspot gains on 5 GHz over 2.4 GHz. The plan assumed the band could be requested on
Android 13. Checked against SDK 36's `api-versions.xml`: `WifiManager.startLocalOnlyHotspotWithConfiguration`,
`SoftApConfiguration.Builder` and `SoftApConfiguration.getChannels` are public **since API 36** (Android 16); on
13-15 they are system APIs an app cannot call.

## Decision

| # | Decision | Why | Where |
|---|---|---|---|
| 1 | Band request (`setChannels` with `BAND_2GHZ` or `BAND_5GHZ`, channel 0 = auto) only on **Android 16+**. Older phones run the default band and the JSON says so (`band_requested: false`, `note`) | No hidden-API reflection; the result must be honest about what was asked | `HotspotBandProbe.run` |
| 2 | One JSON file per run (`schema` 1): device, SDK, 5/6 GHz support, STA+AP concurrency, start time, SSID, security, the channels the system chose (16+), the phone's IPv4 addresses, and every upload/download with bytes, nanoseconds and Mbit/s | Real-device runs can be collected later and compared without re-reading logs | `M3Report.toJson` |
| 3 | Throughput needs a client on the hotspot. The probe opens a **sink** port (counts what a client uploads) and a **source** port (sends 256 MiB of zeros); from a PC: `curl -T big.bin telnet://HOST:SINK` and `curl telnet://HOST:SOURCE -o NUL` | Only stock tools on the PC; the phone measures the bytes itself | `HotspotBandProbe.measure` |
| 4 | **Debug builds only**: `M3ProbeActivity` is registered in `src/debug/AndroidManifest.xml` and started by adb: `adb shell am start -n io.wonmor.vutric_spike/.w6.M3ProbeActivity --es band 5 --ei seconds 120`. Output in `/sdcard/Android/data/io.wonmor.vutric_spike/files/m3/` (`adb pull`) and logcat tag `VutricM3` | No measurement screen in the release app; no W1 screen work | `M3ProbeActivity.kt` |

## Verified

- `M3ReportTest` (3): band names, every JSON field present and escaped, a failed run has no speed.

## Device checklist (owner)

- [~] 1. Android 16 phone with 5 GHz: run `band 2.4` and `band 5`, a PC joins each hotspot and runs both curl commands; pull the two JSON files.
- [~] 2. Android 13-15 phone: `band default`; the PC reads the band from its own Wi-Fi status (`netsh wlan show interfaces`) and the value is added to W3's M3 row by hand.
- [~] 3. W3 records the numbers in its M3 row (ADR-0007 §5).
