#!/usr/bin/env bash
set -euo pipefail
binary="${1:-src-tauri/target/x86_64-apple-darwin/release/easyinput}"
if [[ ! -f "$binary" ]]; then
  echo "未找到构建产物: $binary" >&2
  echo "请先运行 npm run tauri:build:intel" >&2
  exit 1
fi
file "$binary"
archs="$(lipo -archs "$binary")"
if [[ "$archs" != "x86_64" ]]; then
  echo "架构校验失败，预期 x86_64，实际: $archs" >&2
  exit 1
fi
echo "架构校验通过: 纯 x86_64 Mach-O，不需要 Rosetta。"
