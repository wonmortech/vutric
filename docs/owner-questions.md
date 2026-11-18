# Questions for the owner

Append only, one line each, prefixed with the stream tag (W1, W2, W3). A question never blocks the work: the stream
chooses the safest default, writes that default here, and continues.

| Stream | Date | Question | Default taken meanwhile |
|---|---|---|---|
| W1 | 2026-11-18 | Manual fallback when the QR cannot be scanned: typed IP alone cannot connect (the endpoint id is needed). Short code or address-only hello? Asked W3 in #9. | Paste the pairing link by hand (scanner page, reads the clipboard). |
| W1 | 2026-11-18 | Android sharing shortcuts for paired devices ("Ofis kompyuteri" in the Share sheet) need the trust list, which W3 has not built yet. | Vutric itself is the Share target; per-device shortcuts follow the trust store. |
| W3 | 2026-11-18 | Which domain and server should host the production relay (R2)? It needs TCP 80/443, UDP 7842, a DNS name and an ACME contact mailbox. | `Relays::default()` stays n0 only; the relay crate, config and docs are ready to deploy. |
| W3 | 2026-11-18 | CI `Format` fails on `crates/vutric-mobile/src/frb_generated.rs` (W1, generated, import order) since bf85c1c; every PR on this base is red there. W1 to run `cargo fmt -p vutric-mobile` after codegen, or mark the file `#![rustfmt::skip]`? | W3 does not touch W1 files; its own crates pass `cargo fmt --check`. |
