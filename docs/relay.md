# The Vutric relay

When two devices cannot reach each other directly (different networks, a strict NAT, guest Wi-Fi that keeps
devices apart), their QUIC packets go through a relay. Vutric runs its own (`crates/vutric-relay`,
[ADR-0053](decisions/0053-own-relay.md)); n0's public relays are only a fallback.

## What the relay operator can see

Every packet between two devices is end-to-end encrypted by QUIC (TLS 1.3, keys known only to the two devices).
The relay forwards opaque packets addressed by endpoint key.

| The operator sees | The operator never sees |
|---|---|
| The endpoint key (a public key) of each connected device | File names, file content, previews, messages |
| The IP address and port each device connects from | Device names (they travel inside the encrypted session) |
| Which keys exchange packets, when, and how many bytes | Which person a key belongs to, unless they learn it elsewhere |
| Connection times and counts (for the limits below) | Anything that would let it decrypt or alter the traffic |

The relay cannot log packet contents (it cannot read them) and, in the default build, keeps no log of who talked
to whom: only counters (`metrics_bind`, Prometheus, on localhost).

Once both devices find a direct path (hole punching), the relay stops carrying their traffic. The app shows the
current route: direct, or via which relay.

The relay also answers `GET /generate_204` over plain HTTP (port 80 with TLS on). The app uses it to tell a
hotel or guest Wi-Fi login page from a dead network (ADR-0068). That request carries no key and no device name.
The operator sees an IP address asking, like any web request.

Files above 10 GiB are never sent through a relay. The app waits 10 s for a direct path, then offers a hotspot
or a cable (ADR-0068). Above 2 GiB, the app asks the user first.

## Limits

| Limit | Default | Why |
|---|---|---|
| Receive rate per client | 12 MiB/s, burst 32 MiB | a phone can still send at Wi-Fi speed; one client cannot fill the uplink |
| Connections per key | 8 | a device reconnecting plus a few app instances; one key cannot take the server |
| Connections in total | 20 000 | the server stays responsive under load |

## Running one

```sh
cargo build --release -p vutric-relay
sudo setcap cap_net_bind_service=+ep target/release/vutric-relay   # ports 80 and 443 without root
vutric-relay /etc/vutric-relay/vutric-relay.toml
```

Start from [`vutric-relay.example.toml`](../crates/vutric-relay/vutric-relay.example.toml). Open TCP 80, TCP 443 and
UDP 7842. Point the DNS name at the server before the first start, so ACME can issue the certificate.

A systemd unit:

```ini
[Unit]
Description=Vutric relay
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=/usr/local/bin/vutric-relay /etc/vutric-relay/vutric-relay.toml
DynamicUser=yes
StateDirectory=vutric-relay
AmbientCapabilities=CAP_NET_BIND_SERVICE
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

With `DynamicUser`, set `cache_dir = "/var/lib/vutric-relay/acme"` (inside `StateDirectory`).

### Deploy kit (ready for the domain)

Everything is in [`crates/vutric-relay/deploy/`](../crates/vutric-relay/deploy/). Once the owner picks the domain
and a server, it takes one command after the DNS record exists:

| File | What it does |
|---|---|
| `vutric-relay.toml.in` + `render-config.sh DOMAIN CONTACT [--staging] [--container]` | writes the production config (ACME, QUIC on UDP 7842, metrics on localhost only); checks the domain and the `mailto:` contact |
| `install.sh DOMAIN CONTACT [--staging] [--binary PATH]` | systemd: builds (or takes a binary), installs to `/usr/local/bin`, renders `/etc/vutric-relay/vutric-relay.toml`, installs and starts `vutric-relay.service`; warns if the name does not resolve yet |
| `vutric-relay.service` | the unit above plus the usual hardening (`CapabilityBoundingSet`, `SystemCallFilter=@system-service`, `MemoryDenyWriteExecute`, private `/tmp` and devices) |
| `Dockerfile` (+ `Dockerfile.dockerignore`) and `docker-compose.yml` | multi-stage build to `distroless/cc:nonroot`; ports 80→8080, 443→8443, UDP 7842; read-only root, all capabilities dropped, the ACME cache in the `relay-data` volume |

First run with `--staging` (Let's Encrypt staging, untrusted certificate) to check DNS and ports. Then re-run without
it. `curl -sI http://DOMAIN/generate_204` must answer `204`. `crates/vutric-relay/tests/deploy_config.rs` checks that
the rendered template parses for both layouts and that the unit and compose files agree with it. **Not run yet:**
the scripts on a real server, the Docker build, and the systemd hardening against iroh-relay's system calls.

## Using it from the app

```rust
let relays = vutric_net::Relays { urls: vec!["https://relay.example.org/".into()], n0_fallback: true };
let peer = vutric_net::Peer::bind_with(secret, name, platform, vutric_net::Reach::Anywhere, &relays).await?;
```

`n0_fallback: false` is the "nothing third-party" setting: only our relays, and no public key lookup (peers are
dialed by ticket or found nearby).
