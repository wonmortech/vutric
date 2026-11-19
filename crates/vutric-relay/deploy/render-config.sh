#!/bin/sh
# SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
# SPDX-License-Identifier: Apache-2.0
#
# Renders vutric-relay.toml from vutric-relay.toml.in (backlog R2, docs/relay.md "Deploy kit").
#   ./render-config.sh DOMAIN CONTACT [--staging] [--container] [--out FILE]
# DOMAIN   the relay's DNS name, already pointing at this server (ACME checks it on the first start)
# CONTACT  mailto: address Let's Encrypt writes to about expiring certificates
# --staging    use Let's Encrypt staging (untrusted certificates; for a first dry run)
# --container  bind 8080/8443 inside a container (docker-compose maps 80/443 to them)
set -eu

here=$(cd "$(dirname "$0")" && pwd)
[ "$#" -ge 2 ] || { sed -n '5,11p' "$0" >&2; exit 2; }
domain=$1
contact=$2
shift 2
staging=false
http_bind="[::]:80"
https_bind="[::]:443"
state_dir=/var/lib/vutric-relay
out="$here/vutric-relay.toml"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --staging) staging=true ;;
    --container) http_bind="[::]:8080"; https_bind="[::]:8443"; state_dir=/data ;;
    --out) shift; out=$1 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

case "$domain" in
  *[!a-z0-9.-]* | "" | .* | *. | *..*) echo "DOMAIN must be a lower-case DNS name: $domain" >&2; exit 2 ;;
  *.*) ;;
  *) echo "DOMAIN needs a dot: $domain" >&2; exit 2 ;;
esac
case "$contact" in
  mailto:?*@?*.?*) ;;
  *) echo "CONTACT must look like mailto:ops@example.org" >&2; exit 2 ;;
esac
if printf '%s' "$contact" | grep -q '["\\ |]'; then
  echo "CONTACT must not contain quotes, backslashes, spaces or |" >&2
  exit 2
fi

sed -e "s|@DOMAIN@|$domain|g" \
    -e "s|@CONTACT@|$contact|g" \
    -e "s|@STAGING@|$staging|g" \
    -e "s|@HTTP_BIND@|$http_bind|g" \
    -e "s|@HTTPS_BIND@|$https_bind|g" \
    -e "s|@STATE_DIR@|$state_dir|g" \
    "$here/vutric-relay.toml.in" > "$out.tmp"
if grep -q '@[A-Z_]*@' "$out.tmp"; then
  echo "a placeholder was left in $out.tmp" >&2
  exit 1
fi
mv "$out.tmp" "$out"
echo "wrote $out (domain $domain, staging $staging)"
