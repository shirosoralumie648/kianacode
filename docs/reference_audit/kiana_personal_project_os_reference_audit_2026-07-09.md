# Kiana 个人项目操作系统参考审计

日期：2026-07-09

本文是 2026-07-09 的详细参考审计附录。对应主设计文档已于 2026-08-22 删除；本文只保留参考来源、可借鉴机制、风险边界和 backlog 映射，不再作为执行规格。

## 1. 扫描范围

本轮 live 扫描确认 `reference/` 下有 38 个目录：

- `12-factor-agents`
- `Archon`
- `ECC`
- `GitNexus`
- `MemPalace`
- `MetaGPT`
- `OpenHands`
- `OpenSpec`
- `Roo-Code`
- `ai-coding-guide`
- `aider`
- `architect-loop`
- `autogen`
- `awesome-agent-skills`
- `claude-code-main (2)`
- `claude-code-rev-main`
- `claude-code-rust`
- `claude-mem-candidate`
- `claude-memory`
- `cline`
- `codex`
- `continue`
- `emdash`
- `everything-claude-code`
- `get-shit-done`
- `graphify`
- `gsd-core`
- `gstack`
- `herdr`
- `langchain`
- `memorix`
- `pi`
- `planning-with-files`
- `pm-skills`
- `ruflo`
- `skills`
- `strix`
- `superpowers`

过期事实修正：`reference/GitNexus` 当前已经存在，不能再写“GitNexus 缺失”。本轮已把它纳入 Repo Intelligence 归因。

## 2. 38-Repo Coverage Matrix

| Reference | 归属能力域 | 可借鉴机制 | 不借鉴 / 边界 | Kiana 落点 |
| --- | --- | --- | --- | --- |
| `12-factor-agents` | Control Flow | natural language -> tool calls、own prompts/context、unified execution state、launch/pause/resume、small focused agents | 不把 12 条原则当框架依赖 | Intent Router、WorkflowRun、GateResult |
| `Archon` | Project OS | command workflow、PR/issue automation、validate/review/artifact、repo config | 不复制其完整 GitHub 自动化作为 P0 | Project Orchestrator、artifact DAG |
| `ECC` | Harness OS | cross-harness skills/agents/hooks/MCP、operator status、AgentShield/security、adapter matrix | 不照搬庞大 catalog；P0 只吸收 policy/status/manifest | Trust Model、plugin policy、status payload |
| `GitNexus` | Repo Intelligence | analyze/setup、knowledge graph、MCP tools、context/impact/trace/detect_changes、staleness | graph 不能替代 live source/test evidence | `kiana-query::repo_map`、impact analysis |
| `MemPalace` | Memory | local memory layering、semantic search、init/mine/search/status | 不采用不可审计命名隐喻作为核心模型 | ContextPack、project memory |
| `MetaGPT` | Multi-Agent | role/action/team 抽象、软件公司式分工 | 不堆角色名，不允许无边界协作 | worker role taxonomy |
| `OpenHands` | Runtime/UI | app-server、workspace、event projection、PR/HUMAN boundary | 不先做完整 SaaS UI | future app-server、review artifacts |
| `OpenSpec` | Spec/Artifact | proposal/design/spec/tasks/code/test/evidence DAG | 当前部分内容偏占位，不能作为核心技术源 | artifact lifecycle |
| `Roo-Code` | Edit/Checkpoint | safe edit、checkpoint、rules、mode、diff preview | 不绑定 VS Code-only 事件模型 | Safe Edit、checkpoint |
| `ai-coding-guide` | Docs/DX | 中文教程结构、权限/安全/MCP/subagent/worktree 最佳实践 | 只作 DX/文档参考，不当实现证据 | 中文 docs、onboarding |
| `aider` | Repo Edit | repo map、git safety、patch/diff workflow、undo | 不自动 commit 用户未确认文件 | repo map、file edit |
| `architect-loop` | Parallel Factory | orchestrator/strategist/builder、fresh context、frozen checks、run manifest、typed evidence | 不采用“无 approval gate”作为 Kiana 默认；Kiana 高风险必须 approval | bounded swarm、frozen checks、job wrapper |
| `autogen` | Multi-Agent | agent worker protocol、handoff、termination | 不使用自由 speaker 或无界群聊 | worker protocol |
| `awesome-agent-skills` | Skill Ecosystem | skill 分类、技能市场参考 | catalog 不等于实现 | skill registry |
| `claude-code-main (2)` | Runtime/Plugins | plugin、settings、hooks、bash sandbox | 不把 bash sandbox 宣传成覆盖所有工具 | plugin/hook/exec policy |
| `claude-code-rev-main` | Runtime | query loop、tool_use/tool_result pairing、structured IO、context organization | 不复制巨型 runner | runtime events |
| `claude-code-rust` | Runtime/Portability | Rust CLI/runtime、WASM/i18n、可移植实现参考 | 不为 P0 引入 WASM/UI 复杂度 | Rust core patterns |
| `claude-mem-candidate` | Memory | session memory candidate extraction | 不把候选记忆直接注入 | memory proposal |
| `claude-memory` | Memory | cross-session memory、worker、consolidation | 不无来源写入长期记忆 | memory ingest/consolidation |
| `cline` | Runtime/Edit | ToolExecutor、ApplyPatch、checkpoint、Kanban/worktree | 不绑定 IDE-only UI | tool execution、worktree |
| `codex` | Runtime/Policy | protocol、exec policy、MCP、sandbox/trust、安全边界 | 不把某一 CLI 的交互当唯一入口 | policy engine、protocol |
| `continue` | Rules/Edit | rules injection、terminal security、IDE edit loop | 不做 IDE 绑定 | rules selector、terminal policy |
| `emdash` | Product UI | lightweight agent/product shell | 不让 UI 先于 core 协议 | product shell reference |
| `everything-claude-code` | Ecosystem | commands、skills、agents、rules、review、tests、verification loop | 生态大全只能作检索来源，不等于 Kiana 功能完成 | capability map、reference lookup |
| `get-shit-done` | Project OS | capture/spec/plan/execute/validate/ship/review、workstreams | 不把阶段命令当唯一状态机 | Project OS commands |
| `graphify` | Repo Graph | local graph、graph.json/html/report、EXTRACTED/INFERRED/AMBIGUOUS confidence | 不用图谱替代源码；推断边必须降权 | graph confidence、repo graph |
| `gsd-core` | Capability Registry | capability packs、execution primitives | 不把 capability 声明当完成 | capability registry |
| `gstack` | Delivery Gates | plan review、eng review、design review、QA、canary、ship、deploy、retro | 不让关卡拖慢小任务 | acceptance gates |
| `herdr` | Product UI | workflow/product experience | 不作为核心协议源 | future dashboard UX |
| `langchain` | Agent Framework | tool/runtime abstraction、integration ecosystem | 避免框架黑盒化控制流 | adapter/integration boundary |
| `memorix` | Memory/Orchestration | Observation/Reasoning/Git memory、MCP、dashboard、locks、verification、team | 不自动把所有会话写成高置信 memory | memory layers、locks |
| `pi` | Product UI | agent shell、状态体验 | 不复制产品形态 | CLI/TUI feel |
| `planning-with-files` | Persistent Planning | `task_plan.md`、`findings.md`、`progress.md` 三文件模式 | 不只靠 markdown checkbox | `.kiana/plans` |
| `pm-skills` | PM Skills | PRD、项目管理、scope/roadmap 技能 | 不把 PM 文档当实现 | planning templates |
| `ruflo` | Plugin/Workflow | plugin system、workflow、swarm、autopilot、cost、安全、browser、memory、witness | 不一次性照搬全部模块 | plugin/swarm/witness |
| `skills` | Skill Ecosystem | skill packaging and reusable workflows | catalog 需 eval 才能启用 | skill loader |
| `strix` | Security | agentic pentest、validated findings、PoC、security report、CI security gate | P0 不做攻击性自动化；只吸收 validated evidence 思想 | security review/audit |
| `superpowers` | Skills/Process | brainstorming、writing plans、TDD、debugging、parallel agents、verification-before-completion | 不把所有任务都重流程化 | workflow skills |

## 3. Reference-Derived Capability Map

| 能力域 | 参考谁 | 借鉴什么 | 不借鉴什么 | 第一版验收 |
| --- | --- | --- | --- | --- |
| Core Agent Runtime | `codex`、`claude-code-rev-main`、`claude-code-rust`、`cline`、`Roo-Code`、`OpenHands` | event、tool approval、safe edit、checkpoint、resume | 单文件巨型 runner、阻塞式 REPL、VS Code-only 模型 | event 可回放，tool result 成对，edit 可预览/回滚 |
| Project OS | `get-shit-done`、`gsd-core`、`planning-with-files`、`gstack`、`Archon`、`OpenSpec`、`architect-loop` | WBS、DAG、plan/review/verify/ship、frozen checks | 纯 Markdown 状态、不可恢复长任务、自写自审 | task cards 可执行、可验证、可恢复 |
| Control Flow | `12-factor-agents`、`architect-loop`、`superpowers` | own control flow、unified execution state、pause/resume、small agents | 框架黑盒控制流 | Router 决策有 reasons，WorkflowRun 可恢复 |
| Memory / Context | `MemPalace`、`claude-memory`、`claude-mem-candidate`、`memorix` | 分层注入、session ingest、Observation/Reasoning/Git memory、source/confidence | 低置信度记忆直接注入 | memory 有 source/confidence，live evidence 优先 |
| Repo Intelligence | `GitNexus`、`graphify`、`aider`、`continue`、`Roo-Code`、`Archon` | repo map、impact、trace、graph confidence、file set、diff | fuzzy edit 默认写入、图谱替代源码 | 修改前知道 scope，diff/checkpoint 可回滚 |
| Multi-Agent / Workflow | `superpowers`、`autogen`、`MetaGPT`、`ruflo`、`ECC`、`architect-loop` | bounded swarm、role/action、budget、termination、path lock | 无限 worker、自由 speaker、无文件边界 | worker 只拿 WorkPacket，主 agent 集成 |
| Review / Delivery | `gstack`、`superpowers`、`everything-claude-code`、`OpenHands`、`Strix` | verification gate、review synthesis、PR artifacts、validated findings | 生成 review 当验证、一键 push/merge | Done 有命令结果，blocking finding 处理完 |
| Plugin Ecosystem | `ruflo`、`everything-claude-code`、`superpowers`、`gsd-core`、`ECC`、`skills` | commands/skills/agents/hooks/MCP 分层、manifest、receipt | 无签名/无 policy marketplace | manifest 可校验，安装有 receipt |
| UI / Product | `pi`、`emdash`、`herdr`、`OpenHands`、`codex`、`ECC` | TUI state、tool cards、workflow board、HUD/status | UI 代替核心协议 | CLI/TUI 可显示 task/evidence/status |
| EDA / Hardware OS | 用户指定嘉立创/EasyEDA 方向、OpenSpec、gstack review gate | 硬件需求捕获、设计审查、BOM 风险、打样/bring-up gate | 自动画板、自动下单、无人工确认的电气安全判断 | `/eda review` 输出 review packet、risk list、bring-up plan |

## 4. P0 Backlog

| P0 能力 | 参考来源 | Kiana 模块 | 可落地机制 | 风险边界 |
| --- | --- | --- | --- | --- |
| WorkflowRun artifact schema | `planning-with-files`、`architect-loop`、`12-factor-agents` | `kiana-tasks::workflow` | `workflow.yaml/state.json/eventlog.jsonl`，eventlog -> state replay | state 是 materialized view，不手写漂移 |
| Task Card / Kanban | `get-shit-done`、`gstack`、`Archon` | `Project Orchestrator` | WBS -> Task Card -> Ready/Doing/Review/Blocked/Done | Kanban 是执行视图，不替代 DAG |
| Evidence Ledger | `superpowers`、`gstack`、`architect-loop` | Evidence Ledger | Done/Blocked 都记录 command/diff/result/risk | 不能把口头声明当完成 |
| ContextPack + Recovery | `memorix`、`claude-memory`、`planning-with-files` | `kiana-query`、context pack | live evidence + memory source/confidence + fingerprint | memory 不能覆盖当前文件 |
| Intent Router | `12-factor-agents`、`codex`、`superpowers` | `kiana-entrypoints` | L0-L5 决策、升级/降级、reasons | 显式命令不能绕过 L5 policy |
| Trust / Policy | `codex`、`ECC`、`claude-code-main (2)` | policy engine | shell/file/network/plugin/MCP/hook/worker 统一 PolicyDecision | 高风险必须 approval |
| `/audit strict` | `gstack`、`Strix`、`everything-claude-code` | `kiana review` | fake/stub/TODO/hardcoded/test/security/release scan | P0 不做攻击性自动化 |
| `/report progress` | `gstack`、Kiana 历史 teacher status | `kiana report` | 从 evidence 生成中文背景/进展/问题/下一步 | 不基于 stale memory 生成当前状态 |

## 5. P1 Backlog

| P1 能力 | 参考来源 | Kiana 模块 | 可落地机制 | 风险边界 |
| --- | --- | --- | --- | --- |
| Bounded swarm / worker runtime | `superpowers`、`autogen`、`MetaGPT`、`architect-loop`、`ECC` | `kiana-coordinator` | typed worker、budget、termination、tool allowlist、path lock | 不过早做分布式 worker |
| Repo map / impact analysis | `GitNexus`、`graphify`、`aider`、`Archon` | `kiana-query::repo_map` | path/symbol/imports/tests/history/graph confidence ranking | graph 不能替代 source evidence |
| Memory ingest | `memorix`、`claude-memory`、`MemPalace` | memory consolidation | Observation/Reasoning/Git memory proposal、stale detection | 不自动写低置信 memory |
| Rules / skills 条件注入 | `continue`、`Roo-Code`、`ECC`、`everything-claude-code` | `kiana-skills` | alwaysApply、glob、regex、mode restrictions | 规则冲突必须可审计 |
| MCP provenance / visibility | `codex`、`ECC`、`everything-claude-code` | `kiana-services::mcp` | server provenance、visibility、required/optional、startup status | MCP 工具不能默认全暴露 |
| `/eda review` | 用户方向、`gstack`、`OpenSpec` | `kiana-eda` | BOM/DFM/Gerber/CPL/bring-up checklist | 不自动画板、不自动下单 |

## 6. P2 Backlog

| P2 能力 | 参考来源 | Kiana 模块 | 可落地机制 | 风险边界 |
| --- | --- | --- | --- | --- |
| Dashboard / graph / timeline | `memorix`、`OpenHands`、`GitNexus`、`graphify` | app-server context panel | workflow board、memory graph、repo graph timeline | 复用 Kiana app contract |
| Browser companion / web shell | `superpowers`、`OpenHands` | app-server、future Web UI | 设计审查、浏览器测试、远程 workspace | 先稳 RPC/event contract |
| Cost / usage attribution | `ruflo`、`ECC`、`cline` | usage ledger | provider、token、估算成本、worker budget | 成本必须标注估算 |
| Ship / canary / deploy | `gstack`、`get-shit-done` | release profile | ship、canary、deploy、retro、release channels | 不拖慢个人版早期迭代 |
| Hardware project dashboard | `/eda` workflow、OpenHands workspace view | future app-server UI | 原理图/BOM/Gerber/bring-up evidence timeline | 只显示 evidence，不替代专业 EDA |

## 7. WorkflowRun 节点到实现模块映射

| 流程节点 | 文件/模块落点 | 最小可测行为 |
| --- | --- | --- |
| Runtime Init | `kiana-tasks::workflow`、`.kiana/workflows/<id>/` | 创建 workflow_id、state、eventlog、DAG 文件 |
| Capture | `kiana-tasks::capture`、`kiana-commands::project` | 从自然语言生成 goal、criteria、constraints、NOT_BUILDING |
| Context Intake | `kiana-query`、`kiana-commands::context` | 生成 ContextPack，包含 live repo evidence 和 memory source/confidence |
| Research | `kiana-query::search`、`kiana-query::repo_map` | findings.md 记录搜索证据、模式、未知项 |
| Design Candidate | `kiana-tasks::design` | 记录 affected modules、risk、test surface、scope limits |
| Plan | `kiana-tasks::plan` | 生成 task_plan.md 和 DAG dependencies |
| Plan Confirmation | `kiana validate --plan` | 验证文件、命令、验收标准可执行 |
| WorkPacket | `kiana-tasks::workpacket` | 输出 allowed/forbidden files、commands、review focus |
| Router | `kiana-coordinator`、`kiana-skills` | 根据 mode/path/task 选择 rules、skills、worker mode |
| Execute | `kiana-tools`、worker runtime | 产出 ResultPacket，记录 diff、commands、scope deviations |
| Quality / Verify | `kiana validate`、`kiana checks` | 产出 VerificationPacket，失败进入 Fix Loop |
| Review / Triage | `kiana review` | 产出 consolidated-review.md，blocking finding 必须处理 |
| Ship / Learn | `kiana report`、memory proposal | 交付摘要、progress、learnings、memory update proposal |

## 8. 归因规则

- 参考结论必须保留来源归因。
- `reference/` live 事实优先于旧文档记忆；本轮确认 GitNexus 已存在。
- 近似目录不能替代目标仓库；同名目录存在时必须按实际内容归因。
- “可借鉴”不等于“已实现”。
- feature matrix 只有在代码、schema、测试、smoke、文档证据同时存在时，才能标记完成。
- 主设计文档只能写产品级决策和实现规格；实现细节必须映射到 backlog、schema、测试或 smoke。
- 每个外部参考最多贡献可落地机制，不贡献口号；无法落地的观察只能留在审计备注，不进入 P0。
