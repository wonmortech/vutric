# SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
# SPDX-License-Identifier: Apache-2.0
#
# Vutric relay image (backlog R2). Build from the repository root:
#   docker build -f crates/vutric-relay/deploy/Dockerfile -t vutric-relay .
# The config is rendered with --container (binds 8080/8443/7842) and mounted at /etc/vutric-relay.

FROM rust:1.96-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p vutric-relay \
 && install -m 0755 target/release/vutric-relay /vutric-relay \
 && mkdir /data-init

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /vutric-relay /usr/local/bin/vutric-relay
# A fresh named volume copies this directory, so the ACME cache is writable by nonroot (uid 65532).
COPY --from=build --chown=65532:65532 /data-init /data
VOLUME ["/data"]
EXPOSE 8080/tcp 8443/tcp 7842/udp
USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/vutric-relay", "/etc/vutric-relay/vutric-relay.toml"]
