# ADR-0053: Our own relay, n0 only as a fallback

Status: accepted (2026-11-18) · Stream: W3 · Backlog: R2 · Builds on: ADR-0005, ADR-0007

## Context

Phase 0.2 relays through n0's public iroh relays (route R0). That is fine for a spike, but Phase 1 must not depend
on a third party's free service for its internet path: no SLA, no say over limits, and users should be able to
read what the relay can see.

## Decision

1. **`crates/vutric-relay`**: the upstream `iroh-relay` 1.4.1 server (the same wire protocol as the client inside
   iroh, so nothing to keep in sync) wrapped with:
   - a TOML config with checked, safe defaults (`deny_unknown_fields`; ACME needs a `mailto:` contact; QUIC
     needs TLS);
   - Let's Encrypt through ACME with a certificate cache, or manual PEM files;
   - a per-client receive rate limit (12 MiB/s, burst 32 MiB);
   - connection caps through upstream's `AccessControl` hook: 8 per endpoint key, 20 000 in total. Upstream's
     own `accept_conn_limit` is documented as not implemented, so we enforce it ourselves;
   - QUIC address discovery on UDP 7842.
2. **Client side** (`vutric_net::Relays`, `Peer::bind_with`, additive): our relay URLs, plus n0's relays and
   lookup when `n0_fallback` is on. iroh chooses the home relay by measured latency among all of them, so n0 is
   used only where it is closer or ours is down. `n0_fallback: false` is fully self-hosted.
3. **Route badge and live speed**: `Session::route()` (direct or relay, round trip), `Session::relay_url()`
   (which relay), `Session::traffic()` with `Speed` (bytes per second over a 2 s window).
4. **Transparency**: [docs/relay.md](../relay.md) lists what the operator sees and never sees, and how to run
   one (the Syncthing lesson: publish it before anyone asks).

## Why not

- **Our own relay protocol:** iroh's client already speaks this one, with key-proven handshakes.
- **A strict "ours first, n0 only when ours fails" order:** iroh has no relay priority; adding one means forking
  the endpoint. Choosing by latency gives the same result once our relay is deployed where the users are.
- **Rate limits by IP:** carrier-grade NAT puts many phones behind one address; the key is the right unit.

## Measured

`crates/vutric-relay/tests/relay_path.rs`, loopback, Windows 10: two peers with no address hints and no n0 meet
through a local relay in 33 ms, then iroh upgrades to a direct path (3.9 ms round trip). With a cap of 1 per key,
a second instance of the same key is refused and the open count stays at 1.

## Left open

- The Vutric domain and server for the production relay (owner question, 2026-11-18). Until then
  `Relays::default()` stays n0 only, and backlog R2 stays `[~]`.
- Throughput through a remote relay and the direct/relay share (M2).
