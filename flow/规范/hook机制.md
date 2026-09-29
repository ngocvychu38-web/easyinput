# Hook 机制（收工自检）

> Claude Code 与通过兼容门的 Codex 在每个用户回合收尾时自动续跑一次，先执行当前路径生效的 `AGENTS.md`／`CLAUDE.md` 中已有的业务专项收工检查，再检查两件事：① 文档是否要按 `文档维护SOP.md`／`DESIGN维护SOP.md` 更新；② 是否已在 Hook 所属项目根的 `flow/进展.md` 顶部留下交接记录。
> Hook 是防漂移兜底，不替代 Agent 主动遵守收工约定。

## 作用域

| 内容 | 作用域 | 位置 |
|---|---|---|
| Hook 定义与行为 | 项目级 | 项目根 `.hooks/`、`.claude/settings.json`、`.codex/hooks.json` |
| Codex Hook 能力开关 | 机器级，当前默认开启 | `~/.codex/config.toml` 的 `features.hooks` 或启动参数 `--enable hooks` |

一个项目边界只安装一套根级 Hook。单仓多子项目不在子项目重复安装。Codex 只有在项目被 trust 后才加载项目级 `.codex/`。

## 共享脚本

Claude Code 与 Codex 共用 `.hooks/stop-doccheck.sh`，调用时传入 `claude` 或 `codex`：

```bash
#!/usr/bin/env bash
# Stop hook —— Claude Code / Codex 每个用户回合收尾续跑一次自检。
# 用法: stop-doccheck.sh [claude|codex]
tool="${1:-claude}"
input="$(cat)"

continue_safe() {
  printf '%s\n' '{"continue":true}'
  exit 0
}

codex_supports_continuation() {
  local version rest major minor patch
  version="$(codex --version 2>/dev/null | awk '{print $NF}')" || return 1
  case "$version" in
    *.*.*) ;;
    *) return 1 ;;
  esac
  major="${version%%.*}"
  rest="${version#*.}"
  minor="${rest%%.*}"
  patch="${rest#*.}"
  case "${major}:${minor}:${patch}" in
    *[!0-9:]* | :* | *: | *::* ) return 1 ;;
  esac
  [ "$major" -gt 0 ] || { [ "$major" -eq 0 ] && [ "$minor" -ge 145 ]; }
}

case "$tool" in
  claude)
    docname="CLAUDE.md"
    scope_field="prompt_id"
    ;;
  codex)
    docname="AGENTS.md"
    scope_field="turn_id"
    codex_supports_continuation || continue_safe
    ;;
  *)
    continue_safe
    ;;
esac

command -v jq >/dev/null 2>&1 || continue_safe
sid="$(printf '%s' "$input" | jq -r '
  if type == "object" and (.session_id | type) == "string"
  then .session_id else empty end
' 2>/dev/null || true)"
scope="$(printf '%s' "$input" | jq -r --arg field "$scope_field" '
  if type == "object"
    and (.[$field] | type) == "string"
    and (.[$field] | length > 0)
  then .[$field] else empty end
' 2>/dev/null || true)"
active="$(printf '%s' "$input" | jq -r '
  if type == "object" and .stop_hook_active == true then "true" else "false" end
' 2>/dev/null || echo false)"
[ -n "$sid" ] && [ -n "$scope" ] || continue_safe

case "${sid}:${scope}" in
  *[!A-Za-z0-9._:-]*) continue_safe ;;
esac
case "${UID:-}" in
  '' | *[!0-9]*) continue_safe ;;
esac
key="${#sid}-${sid}-${#scope}-${scope}"
marker_root="/tmp/project-flow-stopcheck-${UID}"
mkdir -p "$marker_root" 2>/dev/null || continue_safe
marker="$marker_root/${tool}-${key}"

if [ "$active" = "true" ]; then
  mkdir "$marker" 2>/dev/null || true
  continue_safe
fi

if ! mkdir "$marker" 2>/dev/null; then
  [ -d "$marker" ] && continue_safe
  continue_safe
fi

reason="【收工自检】先读取当前路径生效的 ${docname}；若其中有业务专项收工检查，先执行该检查且不扩大本轮授权。① 文档:本轮若有 结构/方案、心智模型、方向、外部资料、设计 变更 → 提议更新对应层级的 ${docname} 或 DESIGN.md,列出修改点等确认。② 交接:在本 Hook 所属项目根的 flow/进展.md 最上面追加一条进展(做了什么/为什么/怎么理解/产出路径/问题→解决/下一步)并把这条贴在回复里给用户看,决策落 decisions.md、坑落 踩坑记录.md。都没有就回复「无需更新」。"
jq -n --arg r "$reason" '{decision:"block", reason:$r}'
```

关键语义：

- Claude Code 只接受 `prompt_id`，Codex 只接受 `turn_id`，不在两种字段之间回退；marker 键同时包含 `session_id`，不同工具、会话和用户回合互不污染。
- marker 表示该用户回合已经触发过，保持单调存在。首次通过原子 `mkdir` 成功的调用返回 `decision:block`；同一 scope 后续无论 `stop_hook_active` 是否正确设置，都只返回 `continue:true`。
- Codex 只有在本机可解析版本不低于 `0.145.0` 时启用自动续跑。旧版本、未知版本、非法或不适合作为文件名的 ID、缺少 `jq`、缺少本工具专属稳定 scope 或 marker 无法建立时安全放行；脚本不依赖外部 `tr` 或 `id` 命令。
- `reason` 用 `jq -n` 生成，避免手工转义中文和引号。
- 上层项目 Hook 被子目录复用时，续跑提示先读取当前路径实际生效的 Agent 入口并执行其中的专项收工检查；项目交接仍只写回 Hook 所属项目根的 `flow/`，不会在子目录另建第二套控制层。

## 两端薄配置

```jsonc
// .claude/settings.json
{ "hooks": { "Stop": [ { "matcher": "", "hooks": [
  { "type": "command", "command": "bash \"$CLAUDE_PROJECT_DIR/.hooks/stop-doccheck.sh\" claude" }
] } ] } }

// .codex/hooks.json
{ "hooks": { "Stop": [ { "hooks": [
  { "type": "command", "command": "root=\"$(git rev-parse --show-toplevel 2>/dev/null || pwd)\"; if [ ! -f \"$root/.hooks/stop-doccheck.sh\" ]; then d=\"$PWD\"; while [ \"$d\" != \"/\" ] && [ ! -f \"$d/.hooks/stop-doccheck.sh\" ]; do d=\"$(dirname \"$d\")\"; done; root=\"$d\"; fi; if [ ! -f \"$root/.hooks/stop-doccheck.sh\" ]; then printf '%s\\n' '{\"continue\":true,\"systemMessage\":\"project-flow stop hook script not found\"}'; exit 0; fi; bash \"$root/.hooks/stop-doccheck.sh\" codex", "timeout": 30, "statusMessage": "收工自检" }
] } ] } }
```

脚本承载行为，配置只负责定位并调用脚本。后续只改脚本不会改变 Codex 的 command trusted hash。

## Codex 兼容与批准

- Codex 当前 `features.hooks` 默认开启；若用户显式关闭，只提醒在 `~/.codex/config.toml` 开启或用 `--enable hooks`，不要擅自修改全局配置。
- 项目级 `.codex/hooks.json` 需项目 trust，首次安装或 command 改动后还需在 TUI 用 `/hooks` 审核批准。
- Codex CLI `0.144.1` 曾把 `decision:block` 产生的 continuation prompt 保存为裸 UUID message ID，继续或恢复任务时触发 `invalid_id_prefix`。
- Codex CLI `0.145.0` 已于 2026-07-27 通过真实 TUI 三段回归：首次自动续跑、同一会话继续、完全退出后恢复；3 个项目 scope 各产生 1 个唯一 `msg_...` Hook prompt ID，裸 UUID 与 `invalid_id_prefix` 均为 0。恢复时 rollout 可能另存无 ID 的历史重放副本，这类记录不冒充新的 Hook prompt ID。
- 上述证据只支持当前兼容门，不外推到更旧或无法识别的客户端；脚本会对它们安全放行。

## 回归测试

在 `project-flow-cy` Skill 源码目录运行确定性测试：

```bash
bash tests/test-stop-hook.sh
```

它覆盖版本门、Claude 专属 `prompt_id`、Codex 专属 `turn_id`、错误字段拒绝、不同 session/tool、active-first、重复 Stop、异常输入、缺 `jq`、无外部 `tr`/`id` 和并发首次触发。

真实 Codex TUI 回归会调用在线模型并创建一次性本地 session，默认跳过；明确需要时运行：

```bash
PROJECT_FLOW_RUN_CODEX_TUI_E2E=1 \
  bash tests/test-codex-stop-hook-e2e.sh
```

它验证：

1. 首次回复后自动续跑；
2. 同一 TUI 再发消息并再次续跑；
3. `/exit` 后按 session ID 新进程恢复并续跑；
4. 三个 scope 的 continuation 分别返回不同完成 token，不能由终端旧画面冒充后续成功；
5. rollout 中恰有 3 个唯一且实际存在的 Hook prompt ID，全部以 `msg_...` 开头；允许恢复过程留下无 ID 的历史重放副本，但禁止把它计作新 ID；
6. 裸 UUID message ID 与 `invalid_id_prefix` 均为 0。

E2E 使用真实用户认证与配置，不修改全局配置；用户级其他 Hook 可能产生独立提示，但不计入项目 Hook 事件。测试结束会删除测试 session、marker 和临时项目。

## 安装

由 `初始化SOP.md` 执行：复制脚本 → `chmod +x .hooks/stop-doccheck.sh` → 合并两端配置 → 检查 Codex 版本与 hooks 开关 → 提醒 trust 项目及 `/hooks` 批准。
