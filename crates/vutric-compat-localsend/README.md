# vutric-compat-localsend

Send to and receive from the [LocalSend](https://localsend.org) app (ADR-0010, ADR-0054).

| | |
|---|---|
| Protocol | LocalSend **v2.2**, <https://github.com/localsend/protocol> `README.md` at `62bd340` (2026-08-07) |
| Read again | 2026-11-18 |
| Tested against the real app | not yet (backlog M5) |
| Source used | the protocol document only; no LocalSend code was copied |

The `vutric-compat-localsend` binary is the adapter process; the daemon starts it with `host::Adapter` and never
parses LocalSend bytes itself.
