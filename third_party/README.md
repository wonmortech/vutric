# third_party

| Crate | Version | License | Why vendored |
|---|---|---|---|
| [netdev](https://github.com/shellrow/netdev) | 0.46.3 | MIT (`netdev/LICENSE`) | Builds for iOS: defines `in6_ifreq` locally, as in the open upstream PR [shellrow/netdev#192](https://github.com/shellrow/netdev/pull/192). Nothing else changed. Remove it and the `[patch.crates-io]` entry in `Cargo.toml` once a fixed release is out. |
