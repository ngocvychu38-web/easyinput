#!/usr/bin/env bash
set -euo pipefail

# 签名已有二进制（如果存在）
BINARY="src-tauri/target/debug/easyinput"
ENTITLEMENTS="src-tauri/Entitlements.plist"

if [[ -f "$BINARY" ]]; then
  /usr/bin/codesign --force --sign - --timestamp=none --entitlements "$ENTITLEMENTS" "$BINARY" 2>/dev/null || true
  echo "✓ 已签名调试二进制"
fi

# 启动 tauri dev
npx tauri dev &
TAURI_PID=$!

# 后台监听二进制变化，自动签名
(
  LAST_HASH=""
  while kill -0 $TAURI_PID 2>/dev/null; do
    if [[ -f "$BINARY" ]]; then
      CURRENT_HASH=$(shasum "$BINARY" 2>/dev/null | cut -d' ' -f1)
      if [[ "$CURRENT_HASH" != "$LAST_HASH" ]] && [[ -n "$CURRENT_HASH" ]]; then
        /usr/bin/codesign --force --sign - --timestamp=none --entitlements "$ENTITLEMENTS" "$BINARY" 2>/dev/null || true
        LAST_HASH="$CURRENT_HASH"
      fi
    fi
    sleep 2
  done
) &

wait $TAURI_PID
