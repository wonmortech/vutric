# ADR-0077: Opt-in anonymous route telemetry

Status: accepted (2026-11-19) · Stream: W3 · Backlog: M2 · Builds on: ADR-0007 §5 (route list), ADR-0053 (relay)

## Context

Two numbers decide the relay budget and whether the hole punching works: the share of sessions that end up
direct, and the share that need the relay. Lab runs do not give them; real networks (carrier NAT, hotel Wi-Fi,
office firewalls) do. Backlog M2 allows measuring this only opt-in and anonymous. The Syncthing lesson (ADR-0053)
applies: say exactly what is sent.

## Decision

1. **Off by default.**
   - `Settings.telemetry` (`TelemetrySettings`) has `enabled: false` and an empty `endpoint`. A client that was
     switched on still sends nothing until a collector is set; the owner question is in `docs/streams/w3-notes.md`.
   - Only `https` collectors are accepted.
   - The switch is "Anonim statistika yuborish", and the screen shows the exact JSON of the next report.
2. **What is counted.** For each session that ends, the badge it had last (`Tally::record`):
   - direct LAN, direct internet, hotspot, cable or relay;
   - `failed` when it never got a path;
   - `upgraded` when it started on the relay and moved to a direct path.
3. **What is sent, and nothing else:** `{"v":1,"day":<UTC day number>,"platform":"Android","tally":{…}}`.
   - It contains no device or install id, no peer, no IP field, no file names or sizes, and no app version. The
     test `a_report_has_no_identifiers` pins the keys.
   - Counts are rounded down to buckets `0, 1, 2, 3, 6, 11, 21, 51, 101`, so a report cannot single out one
     session.
   - Only a finished UTC day is sent (`Day::roll`), never today's, and at most one report per day. The send waits
     a random `jitter` of up to 6 h, so the request time says nothing about the session times.
4. **What the collector still sees.** It sees the client's IP address on the request, which HTTPS cannot hide.
   - The collector design (when the owner picks one) must keep only daily sums per platform and never log IPs.
     `docs/relay.md` will publish that, as it does for the relay.
   - Sending through the relay to hide the IP is possible later. It is not built, because the relay operator is
     the same party.
5. **Client.** `vutric_net::telemetry::send` makes one JSON `POST` with the shared rustls client (`cloud::client`).
   Its user agent is `vutric`, without a version. It returns `Ok(false)` with no traffic while telemetry is off.

## Measured (written as code, not run in this round)

| Test | What it checks |
|---|---|
| `telemetry::tests` (core, 5) | off by default, https only, no collector = not ready; counting by last badge and `upgraded`; bucket table; only finished non-empty days, coarse; the report's keys are exactly `day, platform, tally, v` |
| `crates/vutric-net/tests/telemetry_off.rs` | off, no endpoint, or plain http: `send` returns `false` and nothing connects to the listener; `jitter` bounds |

**Not verified:** a real collector, which does not exist yet.

## Consequences

- W1/W2 add the switch and the daily roll and send (`docs/streams/w3-notes.md`). The collector is a small service
  on the relay host once the owner decides (R2).
