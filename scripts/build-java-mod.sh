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

usage() {
  cat <<'USAGE'
用法：./scripts/build-java-mod.sh [选项]

选项：
  --current-platform       只打包当前平台 controller（默认）
  --all-platforms          尝试构建并打包 macOS/Linux/Windows controller
  --allow-missing          多平台构建时允许缺少交叉编译工具
  --controller-bundle-dir DIR
                           使用已有 controller bundle 目录，不重新构建 controller
  -h, --help               显示帮助

说明：
  universal jar 的 controller bundle 结构必须是：
  <DIR>/<platform>/blockwright-controller(.exe)
USAGE
}

BUILD_MODE="current"
ALLOW_MISSING_FLAG=()
CONTROLLER_BUNDLE_DIR="target/blockwright-controller-bundle"
SKIP_CONTROLLER_BUILD=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --current-platform)
      BUILD_MODE="current"
      shift
      ;;
    --all-platforms)
      BUILD_MODE="all"
      shift
      ;;
    --allow-missing)
      ALLOW_MISSING_FLAG=(--allow-missing)
      shift
      ;;
    --controller-bundle-dir)
      CONTROLLER_BUNDLE_DIR="$2"
      SKIP_CONTROLLER_BUILD=true
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

if [[ -d /opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home ]]; then
  export JAVA_HOME="/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home"
fi

FABRIC_VERSION="$(bump_fabric_version)"
echo "Blockwright Fabric 模组版本已递增到：$FABRIC_VERSION"

if [[ "$SKIP_CONTROLLER_BUILD" == false ]]; then
  if [[ "$BUILD_MODE" == "all" ]]; then
    ./scripts/build-controller-bundle.sh --all-platforms "${ALLOW_MISSING_FLAG[@]}" --output "$CONTROLLER_BUNDLE_DIR"
  else
    ./scripts/build-controller-bundle.sh --current-platform --output "$CONTROLLER_BUNDLE_DIR"
  fi
fi

if [[ ! -d "$CONTROLLER_BUNDLE_DIR" ]] || ! find "$CONTROLLER_BUNDLE_DIR" -mindepth 2 -maxdepth 2 -type f | grep -q .; then
  echo "controller bundle 为空或不存在：$CONTROLLER_BUNDLE_DIR" >&2
  exit 1
fi

CONTROLLER_BUNDLE_ABS="$(cd "$CONTROLLER_BUNDLE_DIR" && pwd)"

(
  cd plugins/fabric
  gradle clean remapJar --no-daemon \
    -PblockwrightControllerBundleDir="${CONTROLLER_BUNDLE_ABS}"
)

echo
echo "Java 版/Fabric 一体化模组已生成："
echo "plugins/fabric/build/libs/blockwright-fabric-${FABRIC_VERSION}.jar"
echo
echo "已打包 controller 平台："
find "$CONTROLLER_BUNDLE_DIR" -mindepth 1 -maxdepth 1 -type d -exec basename {} \; | sort
echo
echo "把这个 jar 放到 Java 版当前 1.21.8 实例的 mods/ 目录；游戏启动后会自动选择当前平台 controller 并启动 Web 服务。"
echo "仍需确认同目录已有 Fabric API。"
