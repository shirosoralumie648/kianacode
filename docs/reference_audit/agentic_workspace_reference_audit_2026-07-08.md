# Agentic Workspace Reference Audit - 2026-07-08

## 结论

这批参考项目不是一个层面的东西。对 Kiana 最合理的吸收方式是把它们拆成五层：

1. **Runner / harness core**：Claude Code / Codex 这类基座，决定交互、工具调用、权限、上下文、CLI/TUI/stream-json 的共同事件协议。
2. **Task workspace**：`planning-with-files`、`gsd-core`、`OpenSpec`，把长任务从聊天记录移到文件系统，形成可恢复、可审查、可继续的任务状态。
3. **Skill / plugin ecosystem**：`ruflo`、`everything-claude-code`、`superpowers`、`gstack`，提供命令、技能、规则、hooks、agent profiles、review gates 的组织方式。
4. **Memory / repo intelligence**：`claude-memory`、`MemPalace`、Archon，以及待确认 URL 的 GitNexus，负责跨会话记忆、代码图谱、影响分析和 MCP 化查询。
5. **Evidence / delivery gates**：`gstack`、`gsd-core`、Archon，把 plan review、eng review、QA、validate、ship、blast radius 变成明确关卡和可保存证据。

Kiana 不应该把这些项目堆成一个“大而全 agent swarm”。当前更稳的方向是：**本地优先的 agentic workspace**，先把 runner 事件、任务工件、插件/技能加载、记忆抽取、代码影响分析、验证证据做成稳定协议，再把 swarm/autopilot/browser/cost/vector memory 作为插件化能力逐步加入。

## 克隆状态

| Reference | Local path | Remote | Commit | Files | Status |
|---|---|---|---:|---:|---|
| Claude Code source / reverse refs | `reference/claude-code-rev-main`, `reference/claude-code-main (2)`, `reference/claude-code-rust` | existing local refs | existing | existing | 已在原 reference 集合中 |
| ruflo | `reference/ruflo` | `https://github.com/ruvnet/ruflo.git` | `12ec7e7` | 5104 | cloned |
| everything-claude-code | `reference/everything-claude-code` | `https://github.com/WorldFlowAI/everything-claude-code.git` | `432485b` | 81 | cloned |
| Archon | `reference/Archon` | `https://github.com/Schr0d/Archon.git` | `55ef3bc` | 164 | cloned |
| gstack | `reference/gstack` | `https://github.com/garrytan/gstack.git` | `11de390` | 1169 | cloned |
| planning-with-files | `reference/planning-with-files` | `https://github.com/OthmanAdi/planning-with-files.git` | `565d4ff` | 422 | cloned |
| get-shit-done | `reference/get-shit-done` | `https://github.com/gsd-build/get-shit-done.git` | `bdcaab2` | 1854 | cloned, repo now points to active `gsd-core` |
| gsd-core | `reference/gsd-core` | `https://github.com/open-gsd/gsd-core.git` | `e162c31` | 2419 | cloned as active successor |
| superpowers | `reference/superpowers` | `https://github.com/obra/superpowers.git` | `d884ae0` | 172 | cloned |
| OpenSpec | `reference/OpenSpec` | `https://github.com/Fission-AI/OpenSpec.git` | `8886e3a` | 945 | cloned |
| MemPalace | `reference/MemPalace` | `https://github.com/adshaa/mempalacejs.git` | `000524b` | 65 | cloned |
| claude-memory | `reference/claude-memory` | `https://github.com/Zey413/claude-memory.git` | `1f1c13c` | 46 | cloned |
| claude-memory fork candidate | `reference/claude-mem-candidate` | `https://github.com/aaronlab/claude-memory.git` | `1f1c13c` | 46 | discovery artifact, not primary |
| GitNexus | not cloned | not found | n/a | n/a | 需要用户提供 URL |

`GitNexus` 没有找到和描述完全匹配的公开仓库：`analyze/status/clean/wiki/list`、`.gitnexus/`、MCP resources、`CLAUDE.md`/`AGENTS.md` 生成这些线索没有形成可信命中。不要用无关项目冒充。

## 核心参考仓库

### Claude Code source / reverse refs

**它怎么做**

Claude Code 这组参考主要覆盖实际 harness 基座：用户输入路由、工具调用生命周期、权限确认、Bash/PowerShell 风险处理、MCP 连接、settings/policy、plugins/skills/hooks、TUI/CLI 体验和上下文压缩。现有本地参考已经有 `claude-code-rev-main`、`claude-code-main (2)`、`claude-code-rust` 三类：逆向行为参考、公开 plugin/skill 示例、Rust 化能力草图。

**Kiana 借鉴重点**

- `kiana-entrypoints` 里把 CLI、TUI、stream-json、bridge 都压到同一个 runner/event contract。
- `kiana-tools` 里统一 tool validation、permission request、tool lifecycle event、shell-aware risk classification。
- `kiana-skills` 里实现 plugin/skill/hook 的加载、信任边界、加载日志和错误报告。
- `kiana-types` 里固定 RuntimeEvent、ToolResult、StopReason、PermissionRequest 等 schema。

**不要照搬**

不要复制逆向源码或 Claude 产品特定 shims。Kiana 要借行为边界和协议，不借品牌提示词、私有 UI 结构和云耦合。

### ruflo

**它怎么做**

`ruflo` 是 agent meta-harness：一个 `npx ruflo init` 安装完整 loop，包括 agents、commands、skills、MCP server、hooks、daemon。它把能力拆成大量 plugin：swarm、autopilot、loop-workers、workflows、federation、AgentDB、RAG memory、knowledge graph、testgen、browser、security audit、cost tracker、goals、observability 等。

**Kiana 借鉴重点**

- 插件目录和 capability 分组：core/orchestration、memory、intelligence、quality、security、observability。
- MCP server + hooks + daemon 的 full install path，和 lite plugin path 的差异。
- cost/budget、安全扫描、browser 测试、memory、workflow 都应是可启停模块，不应直接混进 runner。
- 可以作为 Kiana `plugin manifest v1` 和 `kiana plugin doctor` 的参考。

**风险**

能力面过宽，容易让 Kiana 从 coding-agent core 膨胀成 swarm 平台。现阶段只借插件边界、capability catalog、health/doctor，不把 swarm/autopilot 作为主路径。

### everything-claude-code

**它怎么做**

这是 Claude Code 生态素材库：`agents/`、`commands/`、`skills/`、`rules/`、`hooks/`、`mcp-configs/`、`contexts/`、`scripts/` 和测试。它强调 cross-platform Node hooks、package manager detection、memory persistence、strategic compact、verification loop、continuous learning。

**Kiana 借鉴重点**

- 生态资产组织方式：command 是入口，skill 是流程，agent 是可委派角色，rule 是长期约束，hook 是生命周期自动化。
- hooks 脚本跨平台化，特别是 Windows/macOS/Linux 一套实现。
- `/verify`、`/checkpoint`、`/learn`、`/plan`、`/tdd` 这类命令可以映射成 Kiana built-in commands 或官方 skills。

**风险**

这是素材库，不是 runtime。规则和 agents 不能默认全局启用，否则会污染用户项目行为。

### Archon

**它怎么做**

Archon 的核心问题是“改这个文件会影响什么”。它用真实 parser 构建 dependency graph，支持 Java/Spring DI、JS/TS、Python，输出 dependency graph、hotspots、cycles、domain grouping、blast radius 和 machine-readable agent JSON。Claude skill 提供 `/archon diff`、`/archon analyze`、`/archon setup`、`/archon upgrade`。

**Kiana 借鉴重点**

- `kiana-query` 增加 code impact analyzer：改前给 P0/P1/P2 影响范围，改后给新增依赖边、跨域违规、blind spots。
- `kiana review` 或 `kiana task validate` 把 blast radius 作为 evidence。
- 输出 token-efficient JSON，给 runner/agent 直接消费，不只做人类报告。

**风险**

不要把静态依赖分析包装成“完整理解代码”。必须显式报告 blind spots：反射、动态 import、事件驱动、运行时注册。

### gstack

**它怎么做**

`gstack` 是高强度工程交付方法包，核心是把一个人变成虚拟团队：CEO/product review、eng review、design review、code review、QA、security、ship、deploy、retro。它的 `SKILL.md` 是 router，先做 update/session/repo/telemetry/learnings/routing 状态，然后把请求导向具体 skill。

**Kiana 借鉴重点**

- “关卡式审查”：plan review、eng review、design review、QA、ship 作为明确 gate。
- 每个 gate 都应该产出 artifact/evidence，而不是聊天里的口头判断。
- `kiana task ship` 应该检查 tests、diff、review findings、QA evidence、release notes。
- `kiana review --mode plan|eng|design|qa|security|ship` 可以作为官方命令组。

**风险**

gstack 的人格化和强话术不适合作为 Kiana 默认体验。Kiana 应该保留中性、工程化、可配置的 review profile。

## 控制流 / 任务规划 / 执行协议

### planning-with-files

**它怎么做**

它把长任务状态落到三个文件：`task_plan.md`、`findings.md`、`progress.md`。新版还加入 per-session plan isolation、attestation、append-only JSONL ledger、completion gate、Windows 路径修复、Codex hooks、`PLANNING_DISABLED=1` 防止短任务被计划系统劫持。

**Kiana 借鉴重点**

- `kiana-tasks` 建议采用类似结构，但放在 `.kiana/tasks/<task_id>/`：
  - `task_plan.md`：目标、阶段、验收标准、owner、依赖。
  - `findings.md`：探索事实、风险、外部约束。
  - `progress.md`：人类可读进度。
  - `ledger.jsonl`：机器可读事件和 gate 结果。
- plan attestation/hash 可以防止模型执行过期或被篡改计划。
- gated mode 必须 opt-in，短任务默认不阻塞。

**风险**

计划 hook 太强会劫持 one-shot review、CI、只读研究、nested orchestrator。Kiana 必须有明确的 disable/readonly/headless 行为。

### get-shit-done / gsd-core

**它怎么做**

用户点名的 `get-shit-done` 已迁移到 active `gsd-core`。`gsd-core` 的核心循环是 Discuss -> Plan -> Execute -> Verify -> Ship。它把重研究、计划和执行放到 fresh-context subagents，主会话保持轻量；用 `STATE.md`、`CONTEXT.md` 等工件跨 session 保存状态；`verify` 阶段先诊断和修复，再允许 `ship`。

**Kiana 借鉴重点**

- `kiana task` 命令组可以对齐：capture/discuss、spec、plan、execute、validate/verify、ship、review、milestone、workstreams。
- fresh-context executor 是 Kiana 后续 subagent 的合理形态：每个执行器只拿计划片段、上下文包和验收命令。
- `verify-work` 与 `ship` 要成为硬门槛，不能把“写完代码”当完成。
- `capabilities/` 目录说明它已经把 AI runtime 适配做成 capability catalog，Kiana 也应这样管理 host/provider/tool 能力。

**风险**

不要把完整 GSD 阶段流程强制套到所有任务。Kiana 应支持三种 task profile：quick、standard、gated。

### superpowers

**它怎么做**

Superpowers 是技能链方法论：brainstorming -> writing-plans -> executing-plans/subagent-driven-development -> TDD -> review -> verification-before-completion -> finishing branch。它强调“先确认设计，再写计划，再执行”，并把 TDD、系统化 debugging、验证前置作为技能触发。

**Kiana 借鉴重点**

- Kiana 的官方 skills 可以按生命周期触发，而不是只做 slash command。
- `brainstorming` 和 `writing-plans` 可以作为高风险/大功能默认流程。
- `verification-before-completion` 应变成 runner 的完成声明 gate：没有命令输出证据，不允许标记 done。

**风险**

对于小修小补，强制完整技能链会过重。需要 task size classifier 或用户显式进入 plan mode。

### OpenSpec

**它怎么做**

OpenSpec 是 spec-driven development 框架，主要命令是 `/opsx:explore`、`/opsx:propose`、`/opsx:apply`、`/opsx:archive`。它把每个 change 放到 `openspec/changes/<change>/`，里面有 proposal、specs、design、tasks，并支持跨仓库/team 的 stores。

**Kiana 借鉴重点**

- 大功能用 `spec mode`：需求、场景、设计、任务清单先落盘。
- `stores` 思路适合 Kiana 未来做多仓库/团队共享规格库。
- brownfield 友好，不要求用户重做项目结构。

**风险**

OpenSpec 是规范入口，不是执行 runtime。Kiana 不应把所有 coding loop 都变成 spec ceremony。

## 记忆 / 知识图谱 / 代码理解

### MemPalace

**它怎么做**

MemPalace JS 是 local-first、zero-LLM memory system + MCP server。它用 Wings/Rooms/Drawers/Tunnels 的空间隐喻组织记忆，底层有 LanceDB、SQLite、filesystem、worker_threads、AAAK 压缩和 MCP tools。它提供 `setup`、`init`、`mine`、MCP server、hooks 和 agent-to-system 双向保存。

**Kiana 借鉴重点**

- 记忆分层很有价值：
  - L0 identity/static context。
  - L1 essential story/project milestones。
  - L2 on-demand topic rooms。
  - L3 vector deep search。
- Kiana 可以先做 `kiana memory init/mine/search/status`，再把 vector backend 做成可选。
- MCP memory server 可以作为插件，而不是 runner 内置依赖。

**风险**

一开始引入 LanceDB/embedding/AAAK 会增加安装成本。Kiana P0 应先做 session/event/artifact memory + FTS，再扩向 vector。

### claude-memory

**它怎么做**

`claude-memory` 从 Claude Code session JSONL 抽取结构化记忆，落 SQLite + FTS5，并可选 sentence-transformer embedding。它的规则抽取包括 decisions、file patterns、TODOs、error/fix pairs、preferences；支持 SessionEnd hook、watch daemon、MCP server、web dashboard、session diff/replay、CLAUDE.md generation。

**Kiana 借鉴重点**

- Kiana 自己已经有 session/runtime event，应该直接从 `RuntimeEvent` 和 session JSONL 抽取：
  - decision
  - issue
  - solution
  - preference
  - modified file pattern
  - TODO/follow-up
  - verification evidence
- P0 用 SQLite FTS/BM25 足够，embedding 是 P1/P2。
- 生成项目上下文文件时，不应只生成 `CLAUDE.md`，可以生成 `AGENTS.md`、`.kiana/context.md` 或 Kiana 自己的 context bundle。

**风险**

规则抽取会误判，必须带 confidence、source session、source line/event id，并允许用户删除/禁用某类记忆。

### GitNexus

**它应该代表的能力**

按用户描述，GitNexus 应该是仓库级代码图谱和 MCP 资源系统：`analyze/status/clean/wiki/list` CLI、`.gitnexus/` 索引、调用关系、影响分析、调试/重构辅助、生成 `CLAUDE.md`/`AGENTS.md` 上下文文件。

**Kiana 借鉴重点**

- 如果拿到 URL，重点审计：
  - 索引格式和增量更新策略。
  - MCP resource schema。
  - call graph / symbol graph / file graph 的查询 API。
  - wiki/context file 生成策略。
  - clean/status 的可恢复性和安全边界。

**当前状态**

未找到可信公开仓库。需要用户补 URL 后再补审计。

## Kiana 模块映射

| Kiana module | 借鉴对象 | 应落地的能力 |
|---|---|---|
| `kiana-entrypoints` | Claude Code, Codex, gsd-core | CLI/TUI/stream-json/bridge 共享 runner，quick/standard/gated task profile |
| `kiana-types` | Claude Code, gsd-core, planning-with-files | RuntimeEvent、TaskEvent、GateResult、Evidence、StopReason、PermissionRequest schema |
| `kiana-tools` | Claude Code, ruflo, everything-claude-code | tool lifecycle、permission、shell risk、browser/security/cost 插件化 |
| `kiana-tasks` | planning-with-files, gsd-core, OpenSpec | `.kiana/tasks/<id>/` task workspace、plan/findings/progress/ledger、attestation、resume |
| `kiana-skills` | ruflo, everything-claude-code, superpowers, gstack | skill/plugin manifest、commands、rules、hooks、agent profiles、capability catalog |
| `kiana-query` | Archon, GitNexus, MemPalace | repo map、dependency graph、impact analysis、memory search、context bundle |
| `kiana-commands` | gsd-core, gstack, OpenSpec | `task`, `review`, `validate`, `ship`, `memory`, `impact`, `plugin doctor` |
| `kiana-bridge` / `kiana-remote` | Claude Code app bridge, gstack QA, ruflo UI | event replay、task dashboard、evidence viewer、review/QA artifacts |

## 建议优先级

### P0 - 先做稳的骨架

1. **Task workspace v1**
   - `.kiana/tasks/<task_id>/task_plan.md`
   - `.kiana/tasks/<task_id>/findings.md`
   - `.kiana/tasks/<task_id>/progress.md`
   - `.kiana/tasks/<task_id>/ledger.jsonl`
   - quick/standard/gated 三种 profile。

2. **Evidence gate v1**
   - `kiana task validate`
   - `kiana task ship`
   - `GateResult` 统一记录测试、lint、build、review、diff、manual blocker。

3. **Plugin/skill manifest v1**
   - 明确 commands、skills、rules、hooks、agents、capabilities 的区别。
   - 加 `kiana plugin doctor`，检查权限、路径、hook 顺序和依赖。

4. **Session memory extractor v1**
   - 从 Kiana RuntimeEvent 抽取 decisions/issues/solutions/preferences/TODOs。
   - SQLite FTS first，embedding later。

5. **Impact analysis v1**
   - 先做语言无关 file graph + imports，再按 Rust/TS/Python 扩展。
   - 改前/改后都能生成 blast radius evidence。

### P1 - 做成可用工作流

1. `kiana task capture/spec/plan/execute/validate/ship/review/resume`。
2. `kiana review --mode plan|eng|design|qa|security|ship`。
3. `kiana memory mine/search/status/generate-context`。
4. `kiana impact analyze/diff --format json`。
5. 官方 skills：brainstorming、writing-plan、verification-before-completion、systematic-debugging。

### P2 - 扩展能力

1. Browser QA plugin。
2. Cost/budget tracker。
3. Vector memory backend。
4. OpenSpec-compatible spec store。
5. Fresh-context subagent executor。
6. Security audit and policy plugin。

### P3 - 慎重进入

1. Swarm/autopilot。
2. Federation。
3. Plugin marketplace。
4. Full DAG/PR automation。
5. Web dashboard as primary UX。

## 不建议直接照搬

| Source | 不建议照搬 | 原因 | Kiana 替代 |
|---|---|---|---|
| Claude Code reverse refs | 私有源码、品牌提示词、云产品 shims | 法务和产品耦合风险 | 借行为协议和测试场景 |
| ruflo | 全量 swarm/autopilot/federation 默认启用 | 主线复杂度爆炸 | 插件化 capability，默认关闭 |
| everything-claude-code | 全套 rules/agents 默认注入 | 会污染项目风格 | 用户/项目选择性启用 |
| gstack | 人设化 CEO/经理话术 | 与 Kiana 工程风格不一致 | 中性 review profiles |
| planning-with-files | Stop hook 强阻塞所有未完成计划 | one-shot/CI/read-only 会被劫持 | gated mode opt-in |
| gsd-core | 所有任务都走五阶段 | 小任务过重 | quick/standard/gated profiles |
| OpenSpec | spec ceremony 覆盖所有功能 | 会拖慢普通 coding loop | 大功能 opt-in spec mode |
| MemPalace | P0 引入向量库和 embedding | 安装/性能/隐私成本高 | 先 SQLite FTS + event memory |
| Archon | 把静态图谱当完整真相 | 动态调用和运行时注册 blind spots | 报告 blind spots 和 confidence |

## 最小可落地设计

Kiana 下一步可以先做一个很窄但完整的闭环：

```text
user request
  -> kiana task capture
  -> task workspace created under .kiana/tasks/<id>/
  -> kiana task plan writes task_plan.md + findings.md
  -> runner executes steps and appends ledger.jsonl
  -> tools emit RuntimeEvent + Evidence
  -> memory extractor indexes decisions/issues/solutions
  -> impact analyzer records changed-file blast radius
  -> kiana task validate runs gates
  -> kiana task ship produces final evidence report
```

这个闭环对应了用户最关心的“Runner 交互、工具调用、上下文组织、TUI/CLI 体验”，同时也给后续插件、workflow、swarm、memory、witness、browser、cost、安全、PR 自动化留出稳定插口。
