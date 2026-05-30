#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

ALL_CLASSIFIERS=(
  macos-aarch64
  macos-x86_64
  linux-aarch64
  linux-x86_64
  windows-x86_64
)

OUTPUT_DIR="target/blockwright-controller-bundle"
MODE="current"
ALLOW_MISSING=false

usage() {
  cat <<'USAGE'
用法：./scripts/build-controller-bundle.sh [选项]

选项：
  --current-platform       只构建当前系统的 controller（默认）
  --all-platforms          构建 macOS/Linux/Windows 常用平台 controller
  --allow-missing          多平台构建时允许缺少交叉编译工具，能构建多少打包多少
  --output DIR             输出 bundle 目录，默认 target/blockwright-controller-bundle

输出结构：
  <DIR>/macos-aarch64/blockwright-controller
  <DIR>/macos-x86_64/blockwright-controller
  <DIR>/linux-aarch64/blockwright-controller
  <DIR>/linux-x86_64/blockwright-controller
  <DIR>/windows-x86_64/blockwright-controller.exe

说明：
  真正全平台构建需要对应 Rust target 和 linker。当前机器没有交叉编译工具时，
  请在对应平台/CI 上分别构建后，把产物整理成上面的目录结构，再交给
  ./scripts/build-java-mod.sh --controller-bundle-dir <DIR> 打包成单个 universal jar。
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --current-platform)
      MODE="current"
      shift
      ;;
    --all-platforms)
      MODE="all"
      shift
      ;;
    --allow-missing)
      ALLOW_MISSING=true
      shift
      ;;
    --output)
      OUTPUT_DIR="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "未知参数：$1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

host_target() {
  rustc -vV | sed -nE 's/^host: //p'
}

classifier_for_target() {
  case "$1" in
    aarch64-apple-darwin) echo "macos-aarch64" ;;
    x86_64-apple-darwin) echo "macos-x86_64" ;;
    aarch64-unknown-linux-gnu) echo "linux-aarch64" ;;
    x86_64-unknown-linux-gnu) echo "linux-x86_64" ;;
    x86_64-pc-windows-gnu|x86_64-pc-windows-msvc) echo "windows-x86_64" ;;
    *)
      echo "不支持的 Rust target：$1" >&2
      return 1
      ;;
  esac
}

target_for_classifier() {
  case "$1" in
    macos-aarch64) echo "aarch64-apple-darwin" ;;
    macos-x86_64) echo "x86_64-apple-darwin" ;;
    linux-aarch64) echo "aarch64-unknown-linux-gnu" ;;
    linux-x86_64) echo "x86_64-unknown-linux-gnu" ;;
    windows-x86_64) echo "x86_64-pc-windows-gnu" ;;
    *)
      echo "不支持的平台标识：$1" >&2
      return 1
      ;;
  esac
}

binary_name_for_classifier() {
  case "$1" in
    windows-*) echo "blockwright-controller.exe" ;;
    *) echo "blockwright-controller" ;;
  esac
}

build_one() {
  local classifier="$1"
  local target binary_name source_binary target_dir
  target="$(target_for_classifier "$classifier")"
  binary_name="$(binary_name_for_classifier "$classifier")"

  echo "正在构建 controller：$classifier ($target)"
  if ! cargo build -p blockwright-controller --release --target "$target"; then
    if [[ "$ALLOW_MISSING" == true ]]; then
      echo "跳过 $classifier：当前环境缺少对应 Rust target 或 linker。" >&2
      return 0
    fi
    echo "构建 $classifier 失败。请安装对应 Rust target/linker，或在对应平台/CI 上构建后再打包。" >&2
    return 1
  fi

  source_binary="target/${target}/release/${binary_name}"
  if [[ ! -f "$source_binary" ]]; then
    echo "构建 $classifier 后未找到二进制文件：$source_binary" >&2
    return 1
  fi

  target_dir="${OUTPUT_DIR}/${classifier}"
  mkdir -p "$target_dir"
  install -m 0755 "$source_binary" "${target_dir}/${binary_name}"
}

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

if [[ "$MODE" == "current" ]]; then
  build_one "$(classifier_for_target "$(host_target)")"
else
  for classifier in "${ALL_CLASSIFIERS[@]}"; do
    build_one "$classifier"
  done
fi

echo
echo "controller bundle 已生成：$OUTPUT_DIR"
find "$OUTPUT_DIR" -maxdepth 2 -type f | sort
