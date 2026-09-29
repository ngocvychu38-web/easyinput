#!/usr/bin/env bash
set -euo pipefail
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIVE_PROJECT="${EASYINPUT_LIVE_PROJECT:-$(dirname "$PROJECT_ROOT")/直播软件}"
if [[ ! -f "$LIVE_PROJECT/server/index.js" ]]; then
  echo "未找到直播项目：$LIVE_PROJECT。请设置 EASYINPUT_LIVE_PROJECT 为直播软件目录。" >&2
  exit 1
fi
cd "$LIVE_PROJECT"
export PORT=3002 EASYINPUT_EMBED=1
exec npm run dev
