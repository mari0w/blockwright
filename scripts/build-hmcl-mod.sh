#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ -d /opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home ]]; then
  export JAVA_HOME="/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home"
fi

(
  cd plugins/fabric
  gradle clean remapJar --no-daemon
)

echo
echo "HMCL/Fabric 模组已生成："
echo "plugins/fabric/build/libs/blockwright-fabric-0.1.2.jar"
echo
echo "把这个 jar 放到 HMCL 当前 1.21.8 实例的 mods/ 目录，并确认同目录已有 Fabric API。"
