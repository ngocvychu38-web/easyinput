#!/usr/bin/env bash
set -euo pipefail
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CUBE_PROJECT="${EASYINPUT_CUBE_PROJECT:-$(dirname "$PROJECT_ROOT")/三阶魔方09241}"
if [[ ! -f "$CUBE_PROJECT/server.js" ]]; then
  echo "未找到三阶魔方项目：$CUBE_PROJECT。请设置 EASYINPUT_CUBE_PROJECT。" >&2
  exit 1
fi
cd "$CUBE_PROJECT"
export PORT=8787
exec node server.js
