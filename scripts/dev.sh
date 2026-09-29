#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLED_BINARY="$PROJECT_ROOT/src-tauri/target/debug/bundle/macos/EasyInput.app/Contents/MacOS/easyinput"
DEV_RUNNER="$PROJECT_ROOT/scripts/codesign-dev-runner.sh"

if pgrep -f "$BUNDLED_BINARY" >/dev/null 2>&1; then
  echo "检测到旧的 EasyInput 打包应用仍在运行。请先对该应用按 Command+Q 完全退出，再重新执行 npm run tauri:dev。" >&2
  exit 1
fi

export CARGO_TARGET_X86_64_APPLE_DARWIN_RUNNER="$DEV_RUNNER"
LIVE_PID=""
 CUBE_PID=""
cleanup_services() {
  if [[ -n "$LIVE_PID" ]]; then kill "$LIVE_PID" 2>/dev/null || true; fi
  if [[ -n "$CUBE_PID" ]]; then kill "$CUBE_PID" 2>/dev/null || true; fi
}
trap cleanup_services EXIT INT TERM
if ! curl --fail --silent --max-time 2 http://localhost:3002/api/health | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>{try{const h=JSON.parse(s);process.exit(h.service==="live-color-studio"&&h.easyinput===true?0:1)}catch{process.exit(1)}})'; then
  # Start Node directly so the PID belongs to the server and cleanup cannot orphan it.
  LIVE_PROJECT="${EASYINPUT_LIVE_PROJECT:-$(dirname "$PROJECT_ROOT")/直播软件}"
  if [[ -f "$LIVE_PROJECT/server/index.js" ]]; then
    (cd "$LIVE_PROJECT" && PORT=3002 EASYINPUT_EMBED=1 exec node server/index.js --dev) &
    LIVE_PID=$!
  fi
fi
CUBE_PROJECT="${EASYINPUT_CUBE_PROJECT:-$(dirname "$PROJECT_ROOT")/三阶魔方09241}"
if ! curl --fail --silent --max-time 2 http://localhost:8787/health | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>{try{const h=JSON.parse(s);process.exit(h.ok===true&&h.app==="cube-lab"?0:1)}catch{process.exit(1)}})'; then
  if [[ -f "$CUBE_PROJECT/server.js" ]]; then
    (cd "$CUBE_PROJECT" && PORT=8787 exec node server.js) &
    CUBE_PID=$!
  else
    echo "未找到三阶魔方服务：$CUBE_PROJECT" >&2
  fi
fi
npx tauri dev
