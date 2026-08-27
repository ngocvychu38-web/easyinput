#!/usr/bin/env bash
set -euo pipefail

app="${1:-src-tauri/target/debug/bundle/macos/EasyInput.app}"
entitlements="${2:-src-tauri/Entitlements.plist}"
binary="src-tauri/target/debug/easyinput"
bundle_id="pro.easyinput.desktop.intel"
stable_requirement="=designated => identifier \"${bundle_id}\""

# 1) 对裸调试二进制做 adhoc 签名 + entitlements
#    tauri dev 直接运行此二进制，未签名会导致 macOS TCC 拒绝麦克风/蓝牙等权限
if [[ -f "$binary" ]]; then
  /usr/bin/codesign --force --sign - --timestamp=none \
    --requirements "$stable_requirement" \
    --entitlements "$entitlements" "$binary"
  echo "调试二进制签名完成: $binary"
fi

# 2) 对 .app bundle 做完整签名（tauri:build 后使用）
if [[ -d "$app" ]]; then
  /usr/bin/codesign --force --deep --sign - --timestamp=none \
    --requirements "$stable_requirement" \
    --entitlements "$entitlements" "$app"
  /usr/bin/codesign --verify --deep --strict --verbose=2 "$app"
  /usr/bin/codesign -d --requirements - "$app" 2>&1
  echo "本机签名完成: $app"
fi
