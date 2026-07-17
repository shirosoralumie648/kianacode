# Kiana 个人项目操作系统设计规格

日期：2026-07-09

详细参考扫描、逐仓库归因和 38 个 `reference/` 目录覆盖矩阵见：

- `docs/reference_audit/kiana_personal_project_os_reference_audit_2026-07-09.md`

## 1. Product Thesis

Kiana 不是“又一个 AI coding CLI”。Kiana 的主叙事是：

> Kiana 是面向真实项目推进的个人项目操作系统：把目标管理、项目推进、记忆、代码理解、并行执行、验证交付放进一个可恢复、可审计、可长期迭代的闭环。

Kiana 解决的不是单轮代码生成，而是长期项目的持续推进：

- 用户说“继续”，Kiana 能恢复目标、状态、约束、blocker、验证门槛，并判断下一步。
- 用户说“从零启动项目”，Kiana 能把 PRD/想法变成 WBS、Kanban、任务卡、验证策略和证据账本。
- 用户说“并行推进”，Kiana 能把目标拆成互不冲突的 WorkPacket，有限派发 worker，再由主 agent 集成。
- 用户说“严格审计”，Kiana 能基于 live evidence 查 fake、stub、TODO、硬编码、缺测试、安全和交付风险。
- 用户进入 `/eda`，Kiana 能按硬件项目流程做嘉立创/EasyEDA 资料审查、BOM 风险、DFM、打样和 bring-up 计划。

Kiana 不做：

- 不只靠聊天上下文工作。
- 不把 roadmap 当完成证据。
- 不把自动并行变成无限 worker。
- 不把插件、MCP、hooks、shell、网络访问默认视为可信。
- P0 不自动画 PCB、不自动下单、不自动 push/merge/deploy。

三种交付形态共享同一个 Core：

```text
Kiana Core
  -> Personal CLI          日常自用和 dogfood 入口
  -> Enterprise Offline    权限、审计、license、离线交付
  -> Cloud Workspace       remote session、worker pool、Web console
```

第一阶段优先 Personal CLI，但核心对象、事件、策略、证据、恢复模型必须为企业离线和云端 workspace 留接口。

## 2. Golden Path User Journeys

### 2.1 用户说“继续”

| 项 | 内容 |
| --- | --- |
| 输入 | `继续`、`接着做上次那个商业化缺口`、`恢复 workflow abc123` |
| Kiana 动作 | 读取 workflow/state/eventlog/progress；检查 git dirty state；刷新项目 fingerprint；比对 memory 是否 stale；选择下一个 Ready task |
| 产物 | `context_pack.md`、更新后的 `state.json`、`/project next` 输出、必要时 Block Report |
| 验证 | 能解释为什么选择这个 next task；不会覆盖用户未提交修改；如果 memory 与 live repo 冲突，以 live repo 为准 |

### 2.2 从零启动项目

| 项 | 内容 |
| --- | --- |
| 输入 | 一段 PRD、issue、想法，或 `我要做一个个人项目 OS` |
| Kiana 动作 | Capture goal/criteria/constraints/NOT_BUILDING；构建 ContextPack；生成 WBS；创建 Kanban；写 Task Card；定义验证策略 |
| 产物 | `.kiana/workflows/<id>/workflow.yaml`、`task_plan.md`、`state.json`、`workpackets/*.json` |
| 验证 | 每个 task 都有 owner/status/dependencies/acceptance/verification；`/project board` 可恢复显示 |

### 2.3 并行推进 3 个任务

| 项 | 内容 |
| --- | --- |
| 输入 | `/swarm dispatch --max-workers 3` 或 Project 中出现 3 个 Ready task |
| Kiana 动作 | 检查 task dependencies；计算 allowed/forbidden files；做 path lock；必要时创建 worktree；派发有限 worker；收集 ResultPacket |
| 产物 | 3 个 WorkPacket、3 个 ResultPacket、integration summary、冲突/无冲突判定 |
| 验证 | worker 文件边界不重叠；主 agent 集成后跑统一 verification profile；有冲突则进入 Review/Fix，不自动合并 |

### 2.4 严格审计商用化缺口

| 项 | 内容 |
| --- | --- |
| 输入 | `/audit strict`、`这个项目离商用还差什么` |
| Kiana 动作 | live 扫描代码、脚本、CI、测试、release、docs、security、plugin/MCP/policy；查 fake/stub/TODO/hardcoded；对照 acceptance gates |
| 产物 | `findings.md`、`consolidated-review.md`、风险分级、P0/P1/P2 blocker 列表、中文汇报 |
| 验证 | 每个 finding 有文件/命令/证据；不能验证的项标 Blocked/Unknown；不把文档声明当生产完成 |

### 2.5 EDA 项目审查

| 项 | 内容 |
| --- | --- |
| 输入 | `/eda review` + EasyEDA 工程、BOM、Gerber、坐标文件、需求约束 |
| Kiana 动作 | Capture 硬件需求；检查电源树、接口电平、保护、去耦、测试点；审查 BOM/DFM；生成 bring-up 计划 |
| 产物 | `eda_review.json`、`bom_risk.md`、`bringup-plan.md`、硬件 ReviewPacket |
| 验证 | 关键风险有来源；BOM 风险标注库存/封装/替代料/来源时间；P0 不自动画板或下单 |

## 3. Operating Modes and Router

### 3.1 用户入口

| 入口 | 用途 | 第一版行为 |
| --- | --- | --- |
| `/quick` | 快速问答、解释代码、跑单个命令、单文件轻改 | 不创建 WorkflowRun，记录最小 evidence |
| `/task` | 单任务闭环 | 读上下文、改代码、跑测试、给证据 |
| `/project` | 长期项目推进 | 读取目标、roadmap、blocker，创建 WBS/Kanban |
| `/swarm` | 多 agent 并行 | 派发互不冲突的 WorkPacket，主 agent 集成 |
| `/audit` | 严格审计 | 查 fake/stub/TODO/硬编码/缺测试/安全与交付风险 |
| `/report` | 中文汇报 | 基于 evidence 生成进度、问题、下一步 |
| `/eda` | 电路电子设计 | 嘉立创/EasyEDA 资料审查、BOM、打样、bring-up |

### 3.2 弹性运行档位

| 档位 | 典型输入 | 自动行为 | 退出标准 |
| --- | --- | --- | --- |
| L0 Answer | 解释、建议、单点查询 | 只读回答，不创建 artifact | 用户得到答案 |
| L1 Quick Action | 单命令、单文件轻改、小修复 | 直接执行，记录最小 evidence | diff 或命令结果明确 |
| L2 Task Loop | 跨文件 bugfix/feature/refactor | 创建 Task Card，执行验证闭环 | Task Done/Blocked 有证据 |
| L3 Project OS | 长期目标、PRD、商业化推进 | 创建 WorkflowRun、WBS、Kanban、ContextPack | 项目 board 可恢复推进 |
| L4 Bounded Swarm | 多个 Ready task 且边界清楚 | 拆 WorkPacket，有限 worker 并行，主 agent 集成 | 无冲突 diff，统一验证通过 |
| L5 High-Risk Governance | 发布、部署、权限、安全、资金、硬件打样 | Approval Gate + 审计 + rollback/bring-up plan | 用户批准且风险记录完整 |

### 3.3 Intent Router Decision Rules

Router 按显式命令、风险、任务复杂度、证据需求、并行安全性五类信号决策。

```text
route(user_input, repo_state, memory_state):
  if explicit slash command:
    requested_mode = command_mode
  else:
    requested_mode = classify_by_intent(user_input)

  risk = classify_risk(user_input, repo_state)
  complexity = estimate_complexity(user_input, repo_state)
  evidence_need = estimate_evidence_need(user_input)
  parallel_ready = find_ready_non_overlapping_tasks(repo_state)

  mode = max(requested_mode, complexity_floor(complexity), evidence_floor(evidence_need))

  if risk.requires_approval:
    mode = max(mode, L5)
  if hardware_keywords or eda_artifacts:
    mode = max(mode, EDA)
  if mode == L3 and parallel_ready.count >= 2 and user_allows_swarm:
    mode = L4
  if mode > L1 and no_writable_scope_needed:
    mode = downgrade_to_read_only(mode)

  return mode with reasons
```

优先级规则：

1. 显式命令优先，但不能绕过安全和权限策略。
2. L5 风险优先级最高：push、merge、deploy、密钥、权限、网络、删除、大规模重写、硬件打样必须经过 Approval Gate。
3. `/eda` 是领域路由，不是单独 runtime；它复用 WorkflowRun、Evidence Ledger 和 GateResult。
4. 自动升级必须写入原因：例如 “Task -> Project，因为影响 5 个模块且需要阶段验收”。
5. 自动降级必须保留证据：例如用户输入 `/project` 但实际只是只读解释，可降到 L0/L1 并说明。
6. 冲突时选择更保守档位：风险 > 显式命令 > 复杂度 > 并行机会 > 速度。

### 3.4 `/eda` 领域边界

`/eda` 第一版是硬件项目操作系统，不是自动 PCB 黑箱：

| 能力 | 第一版行为 | 非目标 |
| --- | --- | --- |
| 需求捕获 | 抽取电源、接口、尺寸、成本、环境、认证、打样约束 | 不自动承诺器件可采购 |
| 原理图审查 | 检查电源树、接口电平、保护、去耦、连接器、调试口、测试点 | 不替代工程师最终电气审核 |
| BOM 风险 | 标记封装、库存、替代料、生命周期、嘉立创贴片可得性风险 | 不自动下单 |
| PCB/工艺审查 | 检查层数、线宽线距、孔径、阻抗、DFM、Gerber/坐标/BOM 文件完整性 | P0 不自动布线 |
| Bring-up 计划 | 生成上电顺序、测量点、示波器/万用表步骤、失败回滚 | 不跳过人工安全确认 |

## 4. Core Object Model

Kiana Core 的边界按对象划分，而不是按 UI 或命令划分。

```text
User Intent
  -> Intent Router
  -> WorkflowRun
  -> Task / WorkPacket
  -> Tool / Worker / Plugin
  -> Result / Verification / Review
  -> Evidence Ledger
  -> Memory Update
```

| 对象 | 作用 | 第一版要求 |
| --- | --- | --- |
| Session | 长期对话与项目入口 | 与 Task/Turn 分离，支持 resume/fork/export |
| Turn | 一轮模型请求和工具循环 | 记录 model、cwd、permission、stop reason |
| WorkflowRun | 复杂任务控制平面 | 有 artifact directory、DAG、state、eventlog |
| Task Card | 一个可执行工作单元 | 有目标、范围、依赖、状态、验收标准 |
| ContextPack | 本轮可用上下文 | memory、evidence、stack、scripts、constraints、risks |
| WorkPacket | worker 执行输入 | scope、allowed files、tests、rollback、review focus |
| ResultPacket | 执行结果 | diff、changed files、commands、errors、scope deviations |
| VerificationPacket | 验证结果 | format/lint/type/build/test/security/acceptance |
| ReviewPacket | 审查结果 | reviewer、findings、severity、confidence、evidence |
| Evidence Ledger | 完成证明 | 所有 Done/Blocked 都必须有证据 |
| Memory | 长期项目经验 | 只吸收高置信度、可溯源结果 |

### 4.1 Data Contract Examples

`workflow.yaml` 最小结构：

```yaml
schema_version: kiana.workflow.v1
workflow_id: wf_20260709_001
created_at: "2026-07-09T10:00:00+08:00"
mode: project
goal: "把 Kiana 设计成个人项目操作系统"
status: active
entrypoint: "/project plan"
artifact_dir: ".kiana/workflows/wf_20260709_001"
dag_template: project_os_default
constraints:
  language: zh-CN
  not_building:
    - "P0 不做完整云端 worker pool"
    - "P0 不自动画 PCB"
approval_required:
  - push
  - merge
  - deploy
  - hardware_order
verification_profile: project_os_p0
```

`state.json` 最小结构：

```json
{
  "schema_version": "kiana.workflow_state.v1",
  "workflow_id": "wf_20260709_001",
  "status": "active",
  "current_node": "plan_confirmation",
  "project_fingerprint": {
    "git_head": "abc123",
    "dirty": true,
    "tracked_dirty_files": ["docs/spec.md"],
    "untracked_files": [".kiana/workflows/wf_20260709_001/task_plan.md"]
  },
  "memory_state": {
    "loaded_sources": ["project_memory", "previous_workflow"],
    "stale": false
  },
  "kanban": {
    "ready": ["task_001"],
    "doing": [],
    "review": [],
    "blocked": [],
    "done": []
  },
  "last_event_id": "evt_0009"
}
```

`task_card.json` 最小结构：

```json
{
  "schema_version": "kiana.task_card.v1",
  "task_id": "task_001",
  "workflow_id": "wf_20260709_001",
  "title": "实现 WorkflowRun artifact schema",
  "status": "ready",
  "objective": "创建可恢复的 workflow 运行目录和状态文件",
  "in_scope": ["kiana-tasks/src/workflow.rs", "kiana-tasks/tests/workflow.rs"],
  "out_of_scope": ["cloud worker pool", "web dashboard"],
  "allowed_paths": ["kiana-tasks/**", "docs/superpowers/**"],
  "forbidden_paths": [".env", "target/**"],
  "dependencies": [],
  "acceptance_criteria": [
    "能创建 workflow.yaml/state.json/eventlog.jsonl",
    "重复 resume 不创建重复 workflow"
  ],
  "verification_commands": [
    "cargo test -p kiana-tasks workflow_artifact"
  ],
  "risk_flags": ["writes_project_state"]
}
```

`workpacket.json` 最小结构：

```json
{
  "schema_version": "kiana.workpacket.v1",
  "workpacket_id": "wp_001",
  "task_id": "task_001",
  "worker_mode": "single",
  "goal": "实现 WorkflowRun artifact schema",
  "context_pack": ".kiana/workflows/wf_20260709_001/context_pack.md",
  "allowed_files": ["kiana-tasks/src/workflow.rs", "kiana-tasks/tests/workflow.rs"],
  "forbidden_files": [".env", "Cargo.lock"],
  "commands": ["cargo test -p kiana-tasks workflow_artifact"],
  "rollback": {
    "type": "checkpoint",
    "checkpoint_id": "cp_task_001_start"
  },
  "review_focus": ["state consistency", "resume idempotency"],
  "retry_policy": {
    "max_attempts": 2,
    "on_repeated_failure": "block"
  }
}
```

Evidence event 最小结构：

```json
{
  "schema_version": "kiana.evidence_event.v1",
  "event_id": "evt_0012",
  "workflow_id": "wf_20260709_001",
  "task_id": "task_001",
  "timestamp": "2026-07-09T10:20:00+08:00",
  "kind": "verification_result",
  "status": "pass",
  "summary": "cargo test -p kiana-tasks workflow_artifact passed",
  "evidence": {
    "command": "cargo test -p kiana-tasks workflow_artifact",
    "exit_code": 0,
    "stdout_tail": "test result: ok. 3 passed",
    "changed_files": ["kiana-tasks/src/workflow.rs", "kiana-tasks/tests/workflow.rs"]
  },
  "next_action": "review"
}
```

## 5. Persistent State and Recovery Model

复杂任务创建独立运行目录：

```text
.kiana/workflows/<workflow_id>/
  workflow.yaml
  state.json
  eventlog.jsonl
  context_pack.md
  findings.md
  task_plan.md
  plan-confirmation.md
  decision-log.md
  workpackets/
  resultpackets/
  verification/
  review/
  pr/
  learnings.md
```

### 5.1 恢复规则

| 场景 | Kiana 行为 | 禁止行为 |
| --- | --- | --- |
| crash | 从 `state.json.last_event_id` 回放 eventlog，定位 last stable node | 不凭聊天记忆继续写 |
| interrupt | 写入 `interrupted` event，保留 current node 和 partial result | 不把 partial result 当 Done |
| resume | 检查 git head/dirty files/project fingerprint，再选择 next node | 不覆盖用户中途修改 |
| fork | 新 workflow 继承 parent goal/context，但创建独立 state/eventlog | 不共享可变 state |
| stale memory | 标注 stale，要求 live evidence 刷新后才能用于决策 | 不让旧 memory 覆盖当前文件 |
| dirty git state | 分类为 user_dirty、agent_dirty、mixed_dirty；需要隔离或确认 | 不自动 reset/checkout |
| worker failure | 收集 ResultPacket，按 retry policy 重试或 Block | 不无限重试 |

### 5.2 一致性模型

- EventLog 是 append-only；`state.json` 是 eventlog 的 materialized view。
- Done/Blocked 状态必须能从 eventlog 重建。
- `workflow.yaml` 定义目标和策略，运行中只允许通过 decision event 变更。
- Memory 是辅助输入，不是 source of truth。
- Git live state、当前文件、测试结果、CI/release 输出优先级高于 memory 和旧报告。
- 并行 worker 只写自己的 allowed files；集成前必须做 touch-set audit。

## 6. Trust, Policy, and Approval Model

Kiana 的 trust 模型必须覆盖插件、MCP、hooks、worker、shell、文件编辑、网络访问。

### 6.1 统一 PolicyDecision

```json
{
  "schema_version": "kiana.policy_decision.v1",
  "subject": "worker:wp_001",
  "capability": "file_edit",
  "target": "kiana-tasks/src/workflow.rs",
  "decision": "allow",
  "reason": "target is in workpacket.allowed_files",
  "requires_approval": false,
  "audit_event_id": "evt_0010"
}
```

### 6.2 权限边界

| 能力面 | 默认策略 | 必须记录 |
| --- | --- | --- |
| Shell | 只允许 workspace 内、非破坏命令自动执行；危险命令 ask | command、cwd、exit code、sandbox/network |
| File Edit | allowed paths 内 exact edit；跨 scope ask | before/after diff、path validation、checkpoint |
| Network | 默认 ask 或按 project policy allowlist | target host、purpose、data sensitivity |
| Plugin | manifest 校验、receipt、hash/signature/policy | source、version、capabilities、enabled/disabled |
| MCP | required/optional、server provenance、tool visibility | server config、startup status、tool exposure |
| Hooks | fail-closed 用于安全 gate；普通 hook 可 fail-soft | lifecycle、exit status、payload summary |
| Worker | WorkPacket scope、tool allowlist、budget、path lock | worker id、allowed files、commands、result |
| EDA | 打样/下单/高压/安全相关操作 ask | risk note、approval、manual confirmation |

### 6.3 Approval Gate

需要用户明确批准的操作：

- push、merge、deploy、release、发布包、改 license。
- 删除/重写大量文件、清理未跟踪文件、reset/checkout。
- 访问私密外部服务、上传代码、调用不在 allowlist 的网络地址。
- 安装或启用未信任插件/MCP/hooks。
- 硬件打样、BOM 下单、涉及高压/电池/安全认证的建议执行。

## 7. Reference-Derived Capability Map

参考项目按能力域吸收，不按仓库流水账。完整 38 仓库覆盖矩阵在附录。

| 能力域 | 参考谁 | 借鉴什么 | 不借鉴什么 | Kiana 落点 |
| --- | --- | --- | --- | --- |
| Core Agent Runtime | `codex`、`claude-code-rev-main`、`claude-code-rust`、`cline`、`Roo-Code`、`OpenHands` | Session/Task/Turn、runtime event、tool approval、interrupt/resume、safe edit、checkpoint | 单文件巨型 runner、VS Code-only 模型、stdout 当协议 | `kiana-types`、`kiana-coordinator`、`kiana-tools` |
| Project OS | `get-shit-done`、`gsd-core`、`planning-with-files`、`gstack`、`Archon`、`OpenSpec`、`architect-loop` | WBS、DAG、artifact、plan/review/validate/ship、frozen checks、issue/workpacket split | 纯 Markdown 状态、不可恢复计划、自写自审通过 | `kiana-tasks`、`.kiana/workflows` |
| Control Flow | `12-factor-agents`、`architect-loop`、`superpowers` | own control flow、unified execution state、launch/pause/resume、small focused agents | 框架黑盒控制流、无限 agent loop | Intent Router、GateResult、WorkflowRun |
| Memory / Context | `MemPalace`、`claude-memory`、`claude-mem-candidate`、`memorix` | session ingest、source/confidence、Observation/Reasoning/Git memory、context pack | 低置信度记忆直接注入、不可审计空间隐喻 | `kiana-query`、context memory |
| Repo Intelligence | `GitNexus`、`graphify`、`aider`、`continue`、`Roo-Code` | repo map、impact、trace、detect_changes、graph confidence、diff safety | graph 替代 source evidence、fuzzy edit 默认写 | `kiana-query::repo_map`、artifact graph |
| Multi-Agent / Workflow | `autogen`、`MetaGPT`、`ruflo`、`superpowers`、`ECC`、`architect-loop` | bounded swarm、role/action、worker handoff、path lock、typed evidence | 自由 speaker、几十个角色名、无文件边界并发 | `kiana-coordinator`、WorkPacket |
| Review / Verification | `gstack`、`superpowers`、`everything-claude-code`、`Strix`、`OpenHands` | verification-before-completion、security review、QA、PoC/evidence、review synthesis | 把 review 文本当测试、无证据安全结论 | `kiana validate`、`kiana review` |
| Plugin / Skill Ecosystem | `ruflo`、`everything-claude-code`、`skills`、`awesome-agent-skills`、`pm-skills`、`ECC`、`ai-coding-guide` | commands/skills/agents/hooks/MCP 分层、manifest、安装 receipt、使用指南 | 无签名/无 policy marketplace | `kiana-skills`、plugin policy |
| UI / Product Experience | `pi`、`emdash`、`herdr`、`OpenHands`、`codex`、`ECC` | TUI state、tool cards、dashboard、HUD/status payload | UI 代替核心协议 | CLI/TUI、future app-server |
| EDA / Hardware OS | 用户指定方向 + Project OS/Review/Policy 参考 | 硬件需求捕获、BOM/DFM/bring-up gate | 自动画板、自动下单 | `kiana-eda` rules pack |

## 8. 38-Repo Coverage Matrix

本轮 live 扫描确认 `reference/` 下有 38 个目录。主文档只保留压缩矩阵；逐项归因见 reference audit。

| 目录 | 能力域 | Kiana 吸收点 |
| --- | --- | --- |
| `12-factor-agents` | Control Flow | structured tool calls、unified execution state、pause/resume、small focused agents |
| `Archon` | Project OS | workflow/DAG、PR/issue automation、validate/review/artifact |
| `ECC` | Harness OS | cross-harness skills/agents/hooks/MCP、operator status、policy/security guide |
| `GitNexus` | Repo Intelligence | graph index、MCP tools、impact/trace/detect_changes、staleness |
| `MemPalace` | Memory | local memory layering、semantic search、init/mine/search/status |
| `MetaGPT` | Multi-Agent | role/action/team abstraction，作为边界参考 |
| `OpenHands` | Runtime/UI | app-server、workspace、event projection、PR review boundary |
| `OpenSpec` | Spec/Artifact | proposal/design/spec/tasks/code/test/evidence DAG |
| `Roo-Code` | Edit/Checkpoint | safe edit、checkpoint、rules、mode |
| `ai-coding-guide` | Docs/DX | 中文教程结构、权限/安全/MCP/subagent/plugin/worktree 教学入口 |
| `aider` | Repo Edit | repo map、git safety、patch/diff workflow |
| `architect-loop` | Parallel Factory | orchestrator/strategist/builder、fresh context、frozen checks、run manifest |
| `autogen` | Multi-Agent | agent worker protocol、termination、handoff |
| `awesome-agent-skills` | Skill Ecosystem | skill catalog 分类和安装参考 |
| `claude-code-main (2)` | Runtime/Plugins | plugin、settings、hooks、bash sandbox |
| `claude-code-rev-main` | Runtime | query loop、tool/result pairing、structured IO |
| `claude-code-rust` | Runtime/Portability | Rust CLI/runtime、WASM/i18n 参考 |
| `claude-mem-candidate` | Memory | session memory candidate extraction |
| `claude-memory` | Memory | cross-session memory、worker、consolidation |
| `cline` | Runtime/Edit | ToolExecutor、apply patch、checkpoint、Kanban/worktree |
| `codex` | Runtime/Policy | exec policy、protocol、MCP、sandbox/trust |
| `continue` | Rules/Edit | rules injection、terminal security、IDE edit loop |
| `emdash` | Product UI | lightweight product shell/reference UX |
| `everything-claude-code` | Ecosystem | commands、skills、agents、rules、reviews、verification loop |
| `get-shit-done` | Project OS | capture/spec/plan/execute/validate/ship/review |
| `graphify` | Repo Graph | local graph、EXTRACTED/INFERRED/AMBIGUOUS confidence、graph.json/html/report |
| `gsd-core` | Capability Registry | capability packs、project execution primitives |
| `gstack` | Delivery Gates | plan/eng/design/QA/canary/ship/deploy/retro |
| `herdr` | Product UI | project/workflow product experience参考 |
| `langchain` | Agent Framework | tool/runtime abstractions，主要作为反面边界和 integration 参考 |
| `memorix` | Memory/Orchestration | Observation/Reasoning/Git memory、MCP、dashboard、locks、verification |
| `pi` | Product UI | agent product shell、status/experience参考 |
| `planning-with-files` | Persistent Planning | task_plan/findings/progress/state 文件模式 |
| `pm-skills` | PM Skills | PRD、项目管理、产品经理 skill 参考 |
| `ruflo` | Plugin/Workflow | plugin system、workflow、swarm、autopilot、cost、安全、browser、memory、witness |
| `skills` | Skill Ecosystem | skill packaging and reusable workflows |
| `strix` | Security | agentic pentest、validated findings、PoC、security report |
| `superpowers` | Skills/Process | brainstorming、plans、TDD、debugging、parallel agents、verification |

## 9. P0 / P1 / P2 Implementation Slices

P0 不再按“能力列表”开工，而按 4 个可交付 milestone 实现。

### P0-M1 WorkflowRun + Data Contracts

| 项 | 内容 |
| --- | --- |
| 命令 | `/project plan <goal>`、`/project status` |
| Schema | `workflow.yaml`、`state.json`、`eventlog.jsonl`、`task_card.json` |
| 测试 | 创建 workflow、resume 幂等、eventlog 回放生成 state |
| 验收输出 | `.kiana/workflows/<id>/` 可恢复，`/project status` 能解释 current node |

### P0-M2 Project Board + Task Card

| 项 | 内容 |
| --- | --- |
| 命令 | `/project board`、`/project next`、`/project split <task_id>` |
| Schema | Task Card、Kanban state、dependencies |
| 测试 | Ready task 选择、dependency blocking、scope/NOT_BUILDING 保留 |
| 验收输出 | 一个长期目标能拆成 WBS/Kanban，并选择下一步 |

### P0-M3 Evidence Ledger + Verification Gate

| 项 | 内容 |
| --- | --- |
| 命令 | `kiana validate`、`/audit strict`、`/report progress` |
| Schema | Evidence event、ResultPacket、VerificationPacket、ReviewPacket |
| 测试 | 命令 pass/fail 捕获、Blocked report、中文 report 从 evidence 生成 |
| 验收输出 | Done/Blocked 必须有证据，不能验证则不宣称完成 |

### P0-M4 Context Pack + Recovery

| 项 | 内容 |
| --- | --- |
| 命令 | `/context packet`、`/context search <query>`、`/project resume <workflow_id>` |
| Schema | ContextPack、memory source/confidence、project fingerprint |
| 测试 | stale memory 降权、dirty git 分类、crash/interrupt/resume/fork |
| 验收输出 | 用户说“继续”时能恢复目标、风险、blocker、验证命令和 next task |

### P1

- Bounded swarm：WorkPacket path lock、worker budget、integration gate。
- Repo map：GitNexus/graphify 风格 impact/trace/ranking。
- Memory ingest：Observation/Reasoning/Git memory proposal。
- Rules/skills 条件注入：mode/path/task/glob。
- `/eda review`：BOM/DFM/bring-up checklist engine。

### P2

- Dashboard / workflow board / timeline。
- Browser companion / web shell。
- Cost / usage attribution。
- Plugin marketplace trust UX。
- Enterprise Offline 和 Cloud Workspace。

## 10. Acceptance Gates

### 10.1 设计完成

- Product thesis 清楚：Kiana 是个人项目 OS，不是普通 coding CLI。
- Golden Path 覆盖继续、从零启动、并行、审计、EDA。
- Router 有升级/降级/冲突优先级。
- WorkflowRun、Task Card、WorkPacket、Evidence Event 有最小数据契约。
- Recovery 和 Trust/Policy 有明确行为。
- 38 个 reference 目录都有归因或明确边界。

### 10.2 功能完成

- 有代码或 schema。
- 有测试或 smoke。
- 有 CLI/app-server 输出或用户可见行为。
- 有 failure path。
- 有 evidence ledger 记录。
- 有文档说明和边界说明。

### 10.3 可长期自用

- “继续”类请求能恢复当前目标、blocker、验证命令和下一步。
- 常见项目改动可通过 `/task` 或 `/project` 闭环。
- 失败后能进入 Fix Loop，而不是丢状态。
- 生成报告时基于 live evidence。
- memory 不覆盖当前 repo 事实。

### 10.4 可商用 / 可企业交付

- 权限、policy、plugin、MCP、hooks 都有 trust/provenance。
- release proof 不依赖口头声明。
- package lifecycle、签名、license、support、security proof 可验证。
- 完成状态来自代码/schema/test/smoke/docs/evidence，不来自 roadmap 文本。

## 11. Appendix: Detailed Reference Audit

完整参考审计、38 仓库覆盖矩阵、P0/P1/P2 机制来源、WorkflowRun 节点到模块映射，见：

- `docs/reference_audit/kiana_personal_project_os_reference_audit_2026-07-09.md`

主文档保留产品级决策和第一版实现规格；附录保留逐仓库归因和参考证据边界。
