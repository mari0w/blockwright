#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ $# -ne 1 ]]; then
  echo "用法：./scripts/install-hmcl-mod.sh <HMCL 当前游戏目录>"
  echo
  echo "例子："
  echo "./scripts/install-hmcl-mod.sh ~/.minecraft"
  echo "./scripts/install-hmcl-mod.sh ~/HMCL/.minecraft/versions/1.21.8-fabric"
  exit 1
fi

GAME_DIR="${1/#\~/$HOME}"
MODS_DIR="$GAME_DIR/mods"
JAR_PATH="plugins/fabric/build/libs/blockwright-fabric-0.1.0.jar"

echo "正在重新编译 Blockwright Fabric 模组..."
./scripts/build-hmcl-mod.sh

if [[ ! -f "$JAR_PATH" ]]; then
  echo "构建失败：没有找到生成的 jar：$JAR_PATH" >&2
  exit 1
fi

mkdir -p "$MODS_DIR"
find "$MODS_DIR" -maxdepth 1 -type f -name 'blockwright-fabric-*.jar' -delete
install -m 0644 "$JAR_PATH" "$MODS_DIR/"

echo "已安装 Blockwright Fabric 模组到："
echo "$MODS_DIR/blockwright-fabric-0.1.0.jar"

if ! find "$MODS_DIR" -maxdepth 1 -type f -iname 'fabric-api*.jar' | grep -q .; then
  echo
  echo "注意：没有在 mods/ 目录里发现 Fabric API。"
  echo "请在 HMCL 里安装 Fabric API，或把 Fabric API 1.21.8 的 jar 放进："
  echo "$MODS_DIR"
fi
