#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

fabric_version() {
  sed -nE 's/^version = "([0-9]+\.[0-9]+\.[0-9]+)"$/\1/p' plugins/fabric/build.gradle.kts | head -n 1
}

bump_fabric_version() {
  local current major minor patch next
  current="$(fabric_version)"
  if [[ ! "$current" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "无法解析 Fabric 模组版本号：$current" >&2
    exit 1
  fi

  IFS=. read -r major minor patch <<< "$current"
  next="${major}.${minor}.$((patch + 1))"
  LC_ALL=C CURRENT_VERSION="$current" NEXT_VERSION="$next" perl -0pi -e \
    's/^version = "\Q$ENV{CURRENT_VERSION}\E"/version = "$ENV{NEXT_VERSION}"/m' \
    plugins/fabric/build.gradle.kts
  echo "$next"
}

if [[ -d /opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home ]]; then
  export JAVA_HOME="/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home"
fi

FABRIC_VERSION="$(bump_fabric_version)"
echo "Blockwright Fabric 模组版本已递增到：$FABRIC_VERSION"

(
  cd plugins/fabric
  gradle clean remapJar --no-daemon
)

echo
echo "HMCL/Fabric 模组已生成："
echo "plugins/fabric/build/libs/blockwright-fabric-${FABRIC_VERSION}.jar"
echo
echo "把这个 jar 放到 HMCL 当前 1.21.8 实例的 mods/ 目录，并确认同目录已有 Fabric API。"
