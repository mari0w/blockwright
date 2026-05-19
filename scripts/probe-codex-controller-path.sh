#!/usr/bin/env bash
set -u

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CODEX_HOME_DIR="${ROOT_DIR}/data/codex_home"
SCHEMA_PATH="${ROOT_DIR}/apps/controller/schemas/plan.schema.json"
PROMPT='你是 Blockwright 的 Minecraft AI 助手。只返回一个 JSON 对象，字段为 reply、summary、blueprint、site_plan、actions。玩家说：给我一组红色的砖。请返回 give_item 动作，item=minecraft:red_concrete，count=64。'

print_env() {
  echo "== codex version =="
  codex --version
  echo
  echo "== proxy env =="
  env | grep -Ei '^(https?_proxy|all_proxy|no_proxy)=' | sort || true
  echo
  echo "== codex homes =="
  echo "host_home=${HOME}/.codex"
  echo "controller_home=${CODEX_HOME_DIR}"
  if [[ -L "${CODEX_HOME_DIR}/auth.json" ]]; then
    echo "controller_auth_link=$(readlink "${CODEX_HOME_DIR}/auth.json")"
  elif [[ -f "${CODEX_HOME_DIR}/auth.json" ]]; then
    echo "controller_auth_file=${CODEX_HOME_DIR}/auth.json"
  else
    echo "controller_auth_missing=1"
  fi
  echo
}

run_case() {
  local name="$1"
  shift
  local output_file
  output_file="$(mktemp -t blockwright-codex-probe.XXXXXX)"
  echo "== ${name} =="
  echo "command: CODEX_HOME=${CODEX_HOME_DIR} codex exec $* --output-last-message ${output_file} -"
  printf '%s\n' "${PROMPT}" | CODEX_HOME="${CODEX_HOME_DIR}" codex exec "$@" --output-last-message "${output_file}" -
  local status=$?
  echo "exit_status=${status}"
  if [[ -s "${output_file}" ]]; then
    echo "last_message:"
    sed -n '1,40p' "${output_file}"
  else
    echo "last_message_empty=1"
  fi
  rm -f "${output_file}"
  echo
}

print_env
run_case "plain_exec_same_home" --ignore-user-config -m gpt-5.5 -c model_reasoning_effort=medium --ephemeral
run_case "json_exec_same_home" --ignore-user-config -m gpt-5.5 -c model_reasoning_effort=medium --ephemeral --json
run_case "json_schema_exec_same_home" --ignore-user-config -m gpt-5.5 -c model_reasoning_effort=medium --ephemeral --json --output-schema "${SCHEMA_PATH}"
