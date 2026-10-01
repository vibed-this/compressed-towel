#!/bin/sh
# macOS .app 打包脚本：在 target/release 下产出可双击的 <AppName>.app。
#
# 用法：sh tools/macos/pack-app.sh
# 环境变量（可选）：APP_NAME（默认 CompressedTowel）、BUNDLE_ID（默认 com.example.compressedtowel）。
# 产物：target/release/<AppName>.app（target/ 不入库）。
# 签名：本地 ad-hoc（codesign -s -），本机可跑；分发到别的机器需各自过 Gatekeeper（右键打开）。
set -eu

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP_NAME="${APP_NAME:-CompressedTowel}"
BUNDLE_ID="${BUNDLE_ID:-com.example.compressedtowel}"
VERSION="$(sed -n 's/^version *= *"\(.*\)"$/\1/p' "$ROOT/Cargo.toml" | head -n 1)"

cargo build --release

APP="$ROOT/target/release/$APP_NAME.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp "$ROOT/target/release/compressed-towel" "$APP/Contents/MacOS/$APP_NAME"
cp "$ROOT/launcher.template.toml" "$APP/Contents/Resources/launcher.toml"
cp -r "$ROOT/templates/scripts" "$APP/Contents/Resources/scripts"

sed -e "s/@APP_NAME@/$APP_NAME/g" \
    -e "s/@BUNDLE_ID@/$BUNDLE_ID/g" \
    -e "s/@VERSION@/$VERSION/g" \
    "$ROOT/tools/macos/Info.plist.template" > "$APP/Contents/Info.plist"

codesign --force --deep -s - "$APP"
codesign -v "$APP"
echo "packed: $APP"
