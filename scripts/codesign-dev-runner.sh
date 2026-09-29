#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "EasyInput 调试签名 runner 缺少可执行文件参数" >&2
  exit 2
fi

DEV_BINARY="$1"
shift

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENTITLEMENTS="$PROJECT_ROOT/src-tauri/Entitlements.plist"
APP_IDENTIFIER="pro.easyinput.desktop.intel"
DESIGNATED_REQUIREMENT='=designated => identifier "pro.easyinput.desktop.intel"'

/usr/bin/codesign \
  --force \
  --sign - \
  --timestamp=none \
  --identifier "$APP_IDENTIFIER" \
  --requirements "$DESIGNATED_REQUIREMENT" \
  --entitlements "$ENTITLEMENTS" \
  "$DEV_BINARY"

echo "✓ 已使用稳定开发标识签名：$APP_IDENTIFIER"
exec "$DEV_BINARY" "$@"
