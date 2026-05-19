#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

detect_running_game_dir() {
  ps -axo command= \
    | awk '
      /--gameDir/ {
        for (i = 1; i <= NF; i++) {
          if ($i == "--gameDir" && (i + 1) <= NF) {
            print $(i + 1)
            exit
          }
          if ($i ~ /^--gameDir=/) {
            sub(/^--gameDir=/, "", $i)
            print $i
            exit
          }
        }
      }
    '
}

detect_game_dir() {
  local running_dir
  running_dir="$(detect_running_game_dir || true)"
  if [[ -n "$running_dir" ]]; then
    echo "$running_dir"
    return
  fi

  if [[ -d /Applications/.minecraft ]]; then
    echo "/Applications/.minecraft"
    return
  fi

  if [[ -d "$HOME/.minecraft" ]]; then
    echo "$HOME/.minecraft"
    return
  fi

  echo "$HOME/.minecraft"
}

if [[ $# -gt 1 ]]; then
  echo "用法：./scripts/install-hmcl-mod.sh [HMCL 当前游戏目录|auto]" >&2
  echo
  echo "例子："
  echo "./scripts/install-hmcl-mod.sh"
  echo "./scripts/install-hmcl-mod.sh auto"
  echo "./scripts/install-hmcl-mod.sh ~/.minecraft"
  echo "./scripts/install-hmcl-mod.sh /Applications/.minecraft"
  exit 1
fi

GAME_DIR="${1:-auto}"
if [[ "$GAME_DIR" == "auto" || "$GAME_DIR" == "" ]]; then
  GAME_DIR="$(detect_game_dir)"
  echo "已自动识别 HMCL 当前游戏目录：$GAME_DIR"
else
  GAME_DIR="${GAME_DIR/#\~/$HOME}"
fi
MODS_DIR="$GAME_DIR/mods"
JAR_PATH="plugins/fabric/build/libs/blockwright-fabric-0.1.3.jar"

echo "正在重新编译 Blockwright Fabric 模组..."
./scripts/build-hmcl-mod.sh

if [[ ! -f "$JAR_PATH" ]]; then
  echo "构建失败：没有找到生成的 jar：$JAR_PATH" >&2
  exit 1
fi

mkdir -p "$MODS_DIR"
echo "正在清理旧版 Blockwright Fabric 模组：$MODS_DIR/blockwright-fabric-*.jar"
find "$MODS_DIR" -maxdepth 1 -type f -name 'blockwright-fabric-*.jar' -delete
install -m 0644 "$JAR_PATH" "$MODS_DIR/"

echo "已安装 Blockwright Fabric 模组到："
echo "$MODS_DIR/blockwright-fabric-0.1.3.jar"

if ! find "$MODS_DIR" -maxdepth 1 -type f -iname 'fabric-api*.jar' | grep -q .; then
  echo
  echo "注意：没有在 mods/ 目录里发现 Fabric API。"
  echo "请在 HMCL 里安装 Fabric API，或把 Fabric API 1.21.8 的 jar 放进："
  echo "$MODS_DIR"
fi
