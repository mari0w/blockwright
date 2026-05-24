#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ ! -f .env && -f .env.example ]]; then
  cp .env.example .env
  echo "Created local .env from .env.example. Put real secrets only in .env and do not commit them."
fi

export SERVER_NAME="${SERVER_NAME:-local}"
export RUST_LOG="${RUST_LOG:-info}"

if [[ "${PORT:-}" =~ ^[0-9]+$ ]]; then
  http_port="${PORT}"
else
  http_port="8765"
fi

if [[ -n "${HTTPS_PORT:-}" ]]; then
  https_port="${HTTPS_PORT}"
else
  https_port="$((http_port + 1))"
fi

https_flag="$(printf '%s' "${HTTPS_ENABLED:-}" | tr '[:upper:]' '[:lower:]')"

echo "Starting Blockwright Web..."
echo "Default HTTP address: http://127.0.0.1:${http_port}/web"
if [[ "$https_flag" == "false" || "$https_flag" == "0" || "$https_flag" == "no" || "$https_flag" == "off" ]]; then
  echo "HTTPS is disabled by environment variable."
else
  echo "Default mobile voice HTTPS address: https://127.0.0.1:${https_port}/web"
fi
echo "Temporary port example: PORT=18765 ./scripts/run-web.sh"
echo "Temporary HTTP polling debug example: RUST_LOG=info,tower_http=debug ./scripts/run-web.sh"

exec cargo run -p blockwright-controller -- serve
