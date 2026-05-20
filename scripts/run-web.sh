#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ ! -f .env && -f .env.example ]]; then
  cp .env.example .env
  echo "已从 .env.example 创建本地 .env。真实密钥只写 .env，不要提交。"
fi

export SERVER_NAME="${SERVER_NAME:-local}"
export RUST_LOG="${RUST_LOG:-info,tower_http=debug}"

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

echo "启动 Blockwright Web..."
echo "HTTP 默认地址：http://127.0.0.1:${http_port}/web"
if [[ "$https_flag" == "false" || "$https_flag" == "0" || "$https_flag" == "no" || "$https_flag" == "off" ]]; then
  echo "HTTPS 已按环境变量关闭。"
else
  echo "手机语音 HTTPS 默认地址：https://127.0.0.1:${https_port}/web"
fi
echo "临时换端口示例：PORT=18765 ./scripts/run-web.sh"

exec cargo run -p blockwright-controller -- serve
