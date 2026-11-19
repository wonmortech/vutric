# third_party

| Crate | Version | License | Why vendored |
|---|---|---|---|
| [netdev](https://github.com/shellrow/netdev) | 0.46.3 | MIT (`netdev/LICENSE`) | Builds for iOS: defines `in6_ifreq` locally, as in the open upstream PR [shellrow/netdev#192](https://github.com/shellrow/netdev/pull/192). Nothing else changed. Remove it and the `[patch.crates-io]` entry in `Cargo.toml` once a fixed release is out. |
| [netwatch](https://github.com/n0-computer/net-tools) | 0.19.3 | MIT OR Apache-2.0 (`netwatch/LICENSE-*`) | Builds for iOS: Darwin routing constants defined locally, the three source files of the merged upstream fix [n0-computer/net-tools#267](https://github.com/n0-computer/net-tools/pull/267) (not in a release yet; iroh 1.4.1 needs ^0.19.3). |
