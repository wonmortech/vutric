# Vutric

**Flick a photo off your phone. Watch it land on your desktop.**

Vutric sends photos and files from a phone to a computer with a gesture: you throw the item off the edge of the
phone screen, a portal opens at the edge of the desktop, and the item flies out of it, real-time and GPU-rendered,
following the speed and angle of your throw. Local network only, end-to-end encrypted, no account, no cloud.

> **Status: Phase 0 (technical spikes).** Nothing to install yet. The Windows overlay spike passes its exit
> criteria (60 FPS at 2560×1440 on an integrated GPU, never takes focus); see
> [the report](docs/reports/phase-0.1-windows-overlay.md).

## Platforms, in order

| Phase | Desktop | Phone |
|---|---|---|
| 1 | Windows 10/11 | Android |
| 2 | Linux (Ubuntu: GNOME, KDE) | Android |
| 3 | Windows | iOS |
| 4 | Linux | iOS |
| 5 | macOS | Android, iOS |

Why this order: [ADR-0003](docs/decisions/0003-platform-order.md). Full plan: [docs/roadmap.md](docs/roadmap.md).

## How it works

- **Rust core** ([`vutric-core`](crates/vutric-core)): protocol messages, framing, QR pairing tickets, safe file
  names. Pure, no I/O, shared with the phone app.
- **Transport**: QUIC over [iroh](https://iroh.computer) on the local network; devices find each other with mDNS
  and trust each other only after a QR pairing.
- **Preview first**: the effect starts from a small preview (≤ 96 KiB) while the full file streams and is checked
  with BLAKE3. Target: under 100 ms from finger release to the first frame on the desktop.
- **Overlay** ([`vutric-overlay`](crates/vutric-overlay)): effect timelines as pure functions of time plus one
  scissored WGSL pass on [wgpu](https://wgpu.rs). Platform hosts only provide a transparent, click-through window
  that never takes focus ([`vutric-host-win`](crates/vutric-host-win): DirectComposition).

More: [architecture](docs/architecture.md) · [protocol](docs/protocol.md) · [effects](docs/effects.md) ·
[decisions](docs/decisions/) · [design system](https://vutric.wonmor.io) (source in [`site/`](site), deployed by
[`deploy.yml`](.github/workflows/deploy.yml)).

## Try the overlay spike (Windows)

Requires Rust 1.96 ([rustup](https://rustup.rs) picks it up from `rust-toolchain.toml`).

```powershell
cargo build -p vutric-overlay-spike --release
.\target\release\vutric-overlay-spike.exe --runs 5          # plays the Halqa effect and prints frame timings
.\target\release\vutric-overlay-spike.exe --image photo.jpg --zone left
```

Run it from your desktop session, not over SSH or as a service. Keep working while it runs: it must never steal
your keyboard focus.

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

See [CONTRIBUTING.md](CONTRIBUTING.md). Security reports: [SECURITY.md](SECURITY.md).

## Licence

[Apache-2.0](LICENSE). © 2026 Wonmor and Vutric contributors.
