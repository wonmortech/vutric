#!/bin/sh
# SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
# SPDX-License-Identifier: Apache-2.0
#
# Installs the relay on a systemd Linux server (backlog R2). Run as root from a checkout:
#   crates/vutric-relay/deploy/install.sh DOMAIN CONTACT [--staging] [--binary PATH]
# Without --binary it builds with cargo (needs a Rust toolchain). Re-running upgrades in place.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
[ "$(id -u)" -eq 0 ] || { echo "run as root" >&2; exit 1; }
[ "$#" -ge 2 ] || { sed -n '5,7p' "$0" >&2; exit 2; }
domain=$1
contact=$2
shift 2
staging=""
binary=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --staging) staging="--staging" ;;
    --binary) shift; binary=$1 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

# ACME issues the certificate on the first start only if DOMAIN already resolves here.
if command -v getent >/dev/null 2>&1; then
  if ! getent ahosts "$domain" >/dev/null 2>&1; then
    echo "warning: $domain does not resolve yet; ACME will fail until its DNS record exists" >&2
  fi
fi

if [ -z "$binary" ]; then
  (cd "$repo" && cargo build --release --locked -p vutric-relay)
  binary="${CARGO_TARGET_DIR:-$repo/target}/release/vutric-relay"
fi
install -m 0755 "$binary" /usr/local/bin/vutric-relay
install -d -m 0755 /etc/vutric-relay
# shellcheck disable=SC2086 # $staging is empty or one flag
"$here/render-config.sh" "$domain" "$contact" $staging --out /etc/vutric-relay/vutric-relay.toml
chmod 0644 /etc/vutric-relay/vutric-relay.toml
install -m 0644 "$here/vutric-relay.service" /etc/systemd/system/vutric-relay.service
systemctl daemon-reload
systemctl enable vutric-relay.service
systemctl restart vutric-relay.service
echo "started; firewall: open TCP 80, TCP 443, UDP 7842. Check: curl -sI http://$domain/generate_204"
