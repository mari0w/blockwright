#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
export SERVER_NAME="${SERVER_NAME:-local}"
export RUST_LOG="${RUST_LOG:-info}"
cargo run -p blockwright-controller
