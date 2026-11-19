# vutric-compat-quickshare

Receive from Android's Quick Share share sheet over the LAN (ADR-0010, ADR-0056). **Experimental.**

| | |
|---|---|
| Protocol source | `grishka/NearDrop` `PROTOCOL.md` at `f75f5fb` (2025-08-06, Unlicense); Google's UKEY2 description; `.proto` field numbers (Chromium, BSD-3) |
| Read again | 2026-11-18 |
| Tested against the real app | not yet (backlog M5) |
| Code copied | none |

Runs inside the `vutric-compat` adapter process (crate `vutric-compat-localsend`), never in the daemon.
