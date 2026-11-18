# Questions for the owner

Append only, one line each, prefixed with the stream tag (W1, W2, W3). A question never blocks the work: the stream
chooses the safest default, writes that default here, and continues.

| Stream | Date | Question | Default taken meanwhile |
|---|---|---|---|
| W1 | 2026-11-18 | Manual fallback when the QR cannot be scanned: typed IP alone cannot connect (the endpoint id is needed). Short code or address-only hello? Asked W3 in #9. | Paste the pairing link by hand (scanner page, reads the clipboard). |
| W1 | 2026-11-18 | Android sharing shortcuts for paired devices ("Ofis kompyuteri" in the Share sheet) need the trust list, which W3 has not built yet. | Vutric itself is the Share target; per-device shortcuts follow the trust store. |
| W3 | 2026-11-18 | Which domain and server should host the production relay (R2)? It needs TCP 80/443, UDP 7842, a DNS name and an ACME contact mailbox. | `Relays::default()` stays n0 only; the relay crate, config and docs are ready to deploy. |
| W3 | 2026-11-18 | CI `Format` fails on `crates/vutric-mobile/src/frb_generated.rs` (W1, generated, import order) since bf85c1c; every PR on this base is red there. W1 to run `cargo fmt -p vutric-mobile` after codegen, or mark the file `#![rustfmt::skip]`? | W3 does not touch W1 files; its own crates pass `cargo fmt --check`. |
| W3 | 2026-11-18 | C5 Warpinator has no written protocol spec; the only description is its GPL-3 source (Linux Mint). ADR-0010 requires clean-room code. Can someone who will not write the adapter read that source and write a plain protocol note (gRPC services, group-code certificate exchange, mDNS records), or should C5 be dropped? | C5 not started; C6 croc and C7 magic-wormhole (documented protocols) go first |

## Browser page: HTTPS warning or plain HTTP? (W3, 2026-11-18, ADR-0055)

The browser page (C2) uses HTTPS with the device's self-signed certificate, as ADR-0010 says. Phones show a
"not private" warning once; the person taps through it. Plain HTTP would have no warning but would send the
files and the link token over the LAN unencrypted. Current choice: HTTPS only. Should plain HTTP be an opt-in
setting? Work continues with HTTPS only until you answer.

(W1) 2026-11-18 · A stranger that connects while the phone shows its own QR (pairing window) is trusted as `ticket` without the six-digit check, because the listener cannot know whether the dialer runs it (W3 #13). Accept that for the spike, or refuse such peers until #13? Default meanwhile: trusted as `ticket`.
(W1) 2026-11-18 · Companion notifications: once W3 #12 lands, mirror every app by default (with the allowlist to narrow it) or start with an empty list the user fills? Default meanwhile: every app, and nothing is sent yet.
(W1) 2026-11-18 · LocalSend and KDE Connect on the phone: discovery only until a phone host for the compat adapters exists (W3 #11). Is "discovery only" acceptable for the next APK, or should the rows stay hidden until sending works? Default meanwhile: shown, marked "sending opens later".
(W3) 2026-11-18 · Answer to W1's pairing-window question (#13): ADR-0064 adds a one-time secret to the QR, so a QR dialer proves it read the screen; a peer that proves nothing is never trusted (a guest whose offers are asked about). Trusting such peers anyway would need your decision. Default: not trusted.
