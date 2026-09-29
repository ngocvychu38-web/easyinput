# 采访问卷（Text Fallback）

主题：WaytoAGI第七期AI训练营：EasyInput Intel Mac客户端、实时语音与语音调用应用项目复盘
用户背景：用户是WaytoAGI第七期AI训练营学员。分享起因：Intel芯片Mac不受现有客户端支持，参考老师客户端界面和流程，并利用开源固件与公开接口，完成兼容客户端。希望用大白话讲业务架构、实现原理，重点讲实时语音通话、端到端实时语音取代ASR-LLM-TTS、打断能力，以及通过Function Calling和本地确定性路由打开微信、剪映、Codex等第三方应用。补充火山引擎Function Calling文档输入输出示例容易放反的踩坑。核心经验：人把握方向、边界和验收，具体编码排查交给AI。当前只需要先产出Markdown内容，后续再制作HTML Slide。

---

## 当前执行模式

当前环境不支持原生结构化采访 UI。你必须回退为**结构化文本采访单**，而不是一行填空或散乱追问。

# Text Fallback Mode -- 结构化文本采访单

## 强制执行纪律

1. **必须一字不漏复制**：你在发出采访问卷时，**必须且只能 100% 原样复制**下方【固定输出格式】代码块内的 Markdown 文本！
2. **严禁自作聪明的删减和总结**：绝不允许你基于后文的业务逻辑自己编造采访问卷！绝不允许把带有【A. xxx】【B. xxx】的选项列表吃掉退化成分组标题，必须每一行都原封不动吐给用户，否则用户将不知道该选什么。
3. **极致静默**：严禁附加寒暄、前言或“为什么要问”的解释。
4. 若用户直接回复“全部按默认，用 research”，主 agent 必须按默认值归纳并继续推进，不得因此卡死。

## 固定输出格式

```markdown
请按如下清单直接回复（填写序号或补全说明，不确定选“默认”）：

**A. 场景与目标**
- 场景：【A. 严肃内部汇报(对齐目标)】 【B. 重量级产品宣讲(抓眼球)】 【C. 招商/融资路演(秀实力)】 【D. 知识型培训课件(重逻辑)】 【E. 其他:___】
- 身份与受众：【A. 一线操盘手向高层汇报(要资源/讲成效)】 【B. 业务一号位向外部客户布道(画大饼/重利益)】 【C. 技术骨干与硬核同行切磋(扣细节)】 【D. 专业人士向泛大众科普(求易懂)】 【E. 其他:___】
- 目标：【A. 扭转认知/刷新观念】 【B. 促成拍板决策或掏钱】 【C. 吸引对方主动加入/传播】 【D. 纯信息铺陈同步】 【E. 其他:___】

**B. 内容与边界**
- 期望页数：【A. 5-10页(微型/短汇报)】 【B. 10-20页(标准)】 【C. 20-30页及以上(宽幅深度盘点)】 【D. 让AI凭内容自动决断】
- 密度：【A. 极简呼吸感(整套偏松，不代表每页都空)】 【B. 均衡图文(整套有起伏，主次分明)】 【C. 极高密度干货(整套窗口上移，但页与页仍要错落)】
- 资料：【A. Research(全网检索扩写并做发散)】 【B. 严格闭卷(绝不发散，仅限用户现有文字)】
- 核心主张与红线：【自由补充，例如：主张必须是我们最快，禁止提xx竞品名字】

**C. 视觉与资产策略**
- 风格：【A. 蓝灰数据向极简商务】 【B. 赛博/暗色流光科技极客】 【C. 活泼多色的动感/流行风】 【D. 让AI凭主题自决定】 【E. 其他:___】
- 语言：【A. 中文】 【B. 英文】 【C. 中英双语/专业术语多用英文】
- 配图策略：【A. 无脑 Decorate(装饰点缀)】 【B. Generate(让AI做插画/文生图)】 【C. Provide(有指定图库)】 【D. 仅留空占位槽】
- 品牌定制：【自由补充，例如：主色设为 #FF0000】

**D. 架构控制**
- 模型：【A. 默认(继承主代理)】 【B. 指定更强/更快的模型:___】
- 思考深度/推理等级：【A. 默认(中等)】 【B. 低(快速逻辑)】 【C. 高(烧脑深度思考)】

**E. 人工审计与断点**
- 是否参与中间审计：【A. 不参与，自动跑完全程】 【B. 只看关键节点(如大纲/风格/最终图审)】 【C. 细颗粒度断点(可在单页 planning/html/review 介入)】
- 重点介入节点：【A. 只看最终图审图】 【B. 看 HTML + 图审】 【C. planning / HTML / 图审都可介入】 【D. 其他:___】
- 审计材料范围：【A. 只看主 agent 摘要】 【B. 可直接看最终 PNG】 【C. 可直接点名 runtime 文件 / HTML / 某轮审查图】
```

## 归纳后的问答落点

主 agent 收到文本回答后，必须先按 canonical 字段归纳，再归一化写盘：

```text
presentation_scenario -> scenario
core_audience -> audience
visual_style -> style
brand_constraints -> brand
language_mode -> language
imagery_strategy -> imagery
material_strategy -> material_strategy
subagent_model_strategy -> subagent_model_strategy
subagent_thinking_effort -> subagent_thinking_effort
manual_audit_mode -> manual_audit_mode
manual_audit_scope -> manual_audit_scope
manual_audit_assets -> manual_audit_assets
```

若用户回复“全部按默认，用 research”，至少按以下默认落点写入：

```text
material_strategy: research
page_density: 适中
visual_style: 自动匹配
language_mode: 中文
imagery_strategy: decorate
subagent_model_strategy: 继承主代理
subagent_thinking_effort: 中等
manual_audit_mode: off
manual_audit_scope: final_review_only
manual_audit_assets: summary_only
```

---

## 共享采访核心

# 采访问卷共享核心

> 本文件是 Step 0 的共享采访内容合同，不直接作为运行时 prompt 发给主 agent。
> 运行时应按能力选择 `tpl-interview-structured-ui.md` 或 `tpl-interview-text-fallback.md`。

## 核心采访目标（执行指南）

作为系统首个守门节点，必须以最高效的轮次获取极高信噪比的输入。核心目标：不与用户寒暄，直接锁定能左右大纲结构、视觉风格和管线分支的关键维度参数。

## 必须覆盖的 4 组维度（另加 1 组人工审计扩展）

你向用户抛出的选项，必须精准涵盖基础 4 组维度域，并额外补上 1 组人工审计扩展维度。

### A. 业务场景与传达目标

左右内容深度与叙事基调。

- `presentation_scenario`（落盘归一化到 `scenario`）: 新人介绍 / 内部汇报 / 社区宣讲 / 招商合作 / 融资路演 / 大众科普等
- `core_audience`（落盘归一化到 `audience`）: “你是谁，要在台上向谁讲？” 如一线操盘手向高层要资源 / 业务一号位向客户布道 / 讲师向小白泛大众科普
- `target_action`: 建立认知 / 促成意向 / 愿意加入 / 纯信息同步

### B. 结构密度与生产管线

左右大纲页数、图文排布与数据源获取。

- `expected_pages`: 5-10 页 / 10-20 页 / 20-30 页宽幅 / 自由发挥
- `page_density`: 少而精 / 适中 / 容量极大（注意：这是整套 deck 的整体倾向，不是要求每一页完全同密）
- `material_strategy`: `research`（全网扩写）或 `local_only`（仅限当前提供资料）
- `must_include` / `must_avoid`: 可要求用户补充唯一核心主张与绝对禁区

### C. 视觉审美与资产策略

左右后续 Style / HTML 生成器的美学锁。

- `visual_style`（落盘归一化到 `style`）: 极简商务 / 科技极客 / 轻量活泼 / 自动匹配
- `language_mode`（落盘归一化到 `language`）: 中文 / 英文 / 中英混排
- `imagery_strategy`（落盘归一化到 `imagery`）: decorate / generate / provided / manual_slot
- `brand_constraints`（落盘归一化到 `brand`）: 品牌视觉禁忌、主色、字体偏好、Logo 使用边界

### D. 构建环境与工程卡口

- `success_criteria`: 用户评价标准
- `subagent_model_strategy`: 继承主代理 / 指定更强模型 / 指定更快模型
- `subagent_thinking_effort`: 低 / 中 / 高

### E. 人工审计与断点控制

- `manual_audit_mode`: `off`（不参与） / `milestone_only`（只看关键节点） / `fine_grained`（细颗粒度断点）
- `manual_audit_scope`: 想介入哪些节点，如 `outline` / `style` / `page_planning` / `page_html` / `page_review`
- `manual_audit_assets`: `summary_only`（只看主 agent 摘要） / `png_only`（看图） / `runtime_and_selected_assets`（允许直接点 runtime / html / 指定审查图）

## 字段归一化映射

采访阶段优先使用上面的 canonical 采集名；写盘时统一归一化到 validator 与下游 playbook 现在消费的锚点名。

| 采集字段 | 写入 `interview-qa.txt` / `requirements-interview.txt` 的锚点 |
|---|---|
| `presentation_scenario` | `scenario` |
| `core_audience` | `audience` |
| `target_action` | `target_action` |
| `expected_pages` | `expected_pages` |
| `page_density` | `page_density` |
| `visual_style` | `style` |
| `brand_constraints` | `brand` |
| `must_include` | `must_include` |
| `must_avoid` | `must_avoid` |
| `language_mode` | `language` |
| `imagery_strategy` | `imagery` |
| `material_strategy` | `material_strategy` |
| `subagent_model_strategy` | `subagent_model_strategy` |
| `subagent_thinking_effort` | `subagent_thinking_effort` |
| `manual_audit_mode` | `manual_audit_mode` |
| `manual_audit_scope` | `manual_audit_scope` |
| `manual_audit_assets` | `manual_audit_assets` |

## `interview-qa.txt` 写盘锚点（强制）

所有问卷结果必须映射到以下两份产物，作为后续子代理的真源输入。

1. `interview-qa.txt`
   保留用户原意。为通过 `contract_validator.py` 强校验，结尾必须附加 canonical 锚点段。基础 12 个锚点缺一不可，同时默认追加模型与人工审计锚点：
   `scenario`, `audience`, `target_action`, `expected_pages`, `page_density`, `style`, `brand`, `must_include`, `must_avoid`, `language`, `imagery`, `material_strategy`, `subagent_model_strategy`, `subagent_thinking_effort`, `manual_audit_mode`, `manual_audit_scope`, `manual_audit_assets`

2. `requirements-interview.txt`
   脱水的纯净参数组，同样必须包含上方基础 12 个锚点，以及模型 / 人工审计锚点，并带上丰富化后的明确取值，供 validator、Style、Outline、PageAgent 与后续人工返工节点直接消费。
   若主 agent 已完成归一化，可额外补充 `density_bias: relaxed/balanced/ultra_dense` 作为内部派生字段；未补充时，下游必须根据 `page_density` 自行推导，不得报错。

---

## 最终要求

- 直接给用户一个分组明确的 Markdown 采访单
- 不要退化成 `场景=；受众=；目标动作=...` 这种单行格式
- 允许用户写“默认”，但字段覆盖不能少
- 若用户只回“全部按默认，用 research”，仍必须按 shared core 的默认落点补全 `material_strategy: research` 等关键字段
- 收集完成后，主 agent 再写 `interview-qa.txt` 与 `requirements-interview.txt`
- 写 `interview-qa.txt` 时，必须追加 canonical 锚点段，显式写出 `target_action`、`must_avoid`、`material_strategy`、`subagent_model_strategy`、`subagent_thinking_effort`、`manual_audit_mode`、`manual_audit_scope`、`manual_audit_assets` 等关键字段，避免 validator 因用户回答过短而漏检
