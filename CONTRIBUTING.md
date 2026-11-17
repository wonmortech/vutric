# Contributing to Vutric

Thank you for helping. Vutric is in Phase 0: the work is technical spikes that prove the hard parts before the
apps are built. Read [docs/roadmap.md](docs/roadmap.md) first to see what is in scope now.

## Ways to contribute

| You want to | Do this |
|---|---|
| Run the overlay spike on your GPU and share the numbers | Open an issue with the table it prints, your GPU, OS and monitor setup |
| Report a bug | Open an issue with steps to reproduce |
| Propose a design change | Open an issue first; architectural changes get an ADR in `docs/decisions/` |
| Report a vulnerability | Not a public issue: see [SECURITY.md](SECURITY.md) |

## Development

Requirements: Rust 1.96 via rustup (pinned in `rust-toolchain.toml`) and Git. On Windows the overlay needs a
DirectX 12 capable GPU.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Requirements for a change

- **Tests.** New behaviour and every bug fix come with a test. Protocol, framing, pairing and file-name code
  (`vutric-core`) and effect timelines (`vutric-overlay`) are pure and must stay fully testable without a GPU or
  network. Shaders are validated by `naga` in the test suite.
- **Performance claims are measured.** A change that affects the overlay includes the spike table before and
  after, on the same machine.
- **Never steal focus, never block input.** The overlay must stay click-through and non-activating.
- **Peer input is hostile.** Anything that arrives from the network is validated before use.
- **Our own design.** Do not copy the name, branding, UI or effects of other products.
- **One topic per pull request**, Conventional Commits style (`feat:`, `fix:`, `docs:`, `perf:`, `test:`).

## Developer Certificate of Origin (DCO)

Every commit must be signed off, which certifies that you wrote it or have the right to submit it under the
project licence ([developercertificate.org](https://developercertificate.org/)):

```sh
git commit -s -m "feat(overlay): add Kometa timeline"
```
