# Kiana 个人项目操作系统完整设计规格

日期：2026-07-10

> **状态说明：本文件降级为架构、流程和参考资料汇编，不再作为产品功能设计的主规格。新的产品功能与实现逻辑主规格见 `docs/kiana-product-functional-and-implementation-design.md`。**

> 文档定位：Kiana 软件逻辑与功能设计的单文件完整交付版。本文不包含具体代码实现，而是定义产品逻辑、对象模型、状态机、路由、项目管理、并行执行、记忆、权限、验证、EDA 和商用交付协议。

> 维护策略：本文件用于连续阅读、评审和需求冻结；`docs/kiana_project_os/` 中的分卷继续作为模块化维护源。任何产品级变更应同步到主设计和对应分卷，再重新生成本文件。

## 阅读导航

1. 第一部分：产品级主设计与总体架构。
2. 第二部分：36 个软件逻辑与功能设计分卷。
3. 第三部分：38 个参考仓库覆盖矩阵与机制归因。

## 设计边界

- 本文描述软件行为、数据对象、状态迁移、命令语义、权限边界、并发约束、失败恢复和验收证据。
- 本文不直接规定 Rust/TypeScript 的具体类、函数、数据库建表语句或代码实现。
- P0 重点是 Project Orchestrator、Project Board、Context/Recovery、Evidence Ledger、Verification Gate 和有界并行。
- `/eda` 与软件项目共享 WorkflowRun、WorkPacket、Evidence、Gate、Approval 和 Report，不建立孤立的第二套系统。
- 所有 Done、Blocked、Ready、Approval 和 Release Readiness 状态都必须有可追踪依据。

## 分卷索引

- `docs/kiana_project_os/01-system-overview-and-product-logic.md`：Volume 01: 系统总览与产品逻辑
- `docs/kiana_project_os/02-workflow-runtime-and-state-machine.md`：Volume 02: Workflow Runtime 与状态机
- `docs/kiana_project_os/03-intent-router-and-command-semantics.md`：Volume 03: Intent Router 与命令语义
- `docs/kiana_project_os/04-project-orchestrator-wbs-kanban.md`：Volume 04: Project Orchestrator、WBS 与 Kanban
- `docs/kiana_project_os/05-evidence-verification-review-ledger.md`：Volume 05: Evidence、Verification、Review 与 Ledger
- `docs/kiana_project_os/06-context-memory-repo-intelligence.md`：Volume 06: Context、Memory 与 Repo Intelligence
- `docs/kiana_project_os/07-bounded-swarm-worker-model.md`：Volume 07: Bounded Swarm 与 Worker 模型
- `docs/kiana_project_os/08-trust-policy-plugin-mcp-hooks.md`：Volume 08: Trust、Policy、Plugin、MCP 与 Hooks
- `docs/kiana_project_os/09-eda-domain-workflow.md`：Volume 09: EDA 领域工作流
- `docs/kiana_project_os/10-p0-p1-p2-delivery-slices.md`：Volume 10: P0/P1/P2 交付切片
- `docs/kiana_project_os/11-data-contracts-and-event-taxonomy.md`：Volume 11: 数据契约与事件分类
- `docs/kiana_project_os/12-cli-tui-product-surface.md`：Volume 12: CLI/TUI 产品面
- `docs/kiana_project_os/13-plugin-skill-ecosystem-design.md`：Volume 13: Plugin 与 Skill 生态设计
- `docs/kiana_project_os/14-mcp-tooling-and-provenance.md`：Volume 14: MCP、工具与 Provenance
- `docs/kiana_project_os/15-security-audit-and-commercial-readiness.md`：Volume 15: 安全审计与商用化准备
- `docs/kiana_project_os/16-reporting-learning-and-memory-governance.md`：Volume 16: Reporting、Learning 与 Memory Governance
- `docs/kiana_project_os/17-enterprise-offline-and-cloud-workspace.md`：Volume 17: Enterprise Offline 与 Cloud Workspace
- `docs/kiana_project_os/18-reference-to-feature-playbook.md`：Volume 18: 参考仓库到功能落地剧本
- `docs/kiana_project_os/19-testing-evaluation-and-smoke-strategy.md`：Volume 19: 测试、评估与 Smoke 策略
- `docs/kiana_project_os/20-roadmap-governance-and-scope-control.md`：Volume 20: Roadmap Governance 与 Scope Control
- `docs/kiana_project_os/21-scheduler-dependency-and-priority-model.md`：Volume 21: Scheduler、依赖与优先级模型
- `docs/kiana_project_os/22-error-failure-and-blocker-taxonomy.md`：Volume 22: 错误、失败与 Blocker 分类
- `docs/kiana_project_os/23-artifact-storage-retention-and-redaction.md`：Volume 23: Artifact 存储、保留与脱敏
- `docs/kiana_project_os/24-command-catalog-and-output-contracts.md`：Volume 24: 命令目录与输出契约
- `docs/kiana_project_os/25-gate-engine-and-quality-profiles.md`：Volume 25: Gate 引擎与 Quality Profile
- `docs/kiana_project_os/26-eda-rules-deep-checklists.md`：Volume 26: EDA 深度规则清单
- `docs/kiana_project_os/27-worker-integration-and-conflict-resolution.md`：Volume 27: Worker 集成与冲突解决
- `docs/kiana_project_os/28-memory-formation-and-stale-invalidation.md`：Volume 28: Memory 形成与失效
- `docs/kiana_project_os/29-report-template-library.md`：Volume 29: 中文报告模板库
- `docs/kiana_project_os/30-implementation-governance-without-code.md`：Volume 30: 非代码实施治理
- `docs/kiana_project_os/31-project-board-data-model-and-queries.md`：Volume 31: Project Board 数据模型与查询语义
- `docs/kiana_project_os/32-workpacket-schema-deep-dive.md`：Volume 32: WorkPacket Schema 深潜
- `docs/kiana_project_os/33-approval-and-human-decision-protocol.md`：Volume 33: Approval 与人工决策协议
- `docs/kiana_project_os/34-domain-rules-and-conditional-injection.md`：Volume 34: Domain Rules 与条件注入
- `docs/kiana_project_os/35-dashboard-projection-model.md`：Volume 35: Dashboard 投影模型
- `docs/kiana_project_os/36-release-proof-and-commercial-blocker-ledger.md`：Volume 36: Release Proof 与商用阻塞账本

# 第一部分：产品级主设计

---

<!-- source: docs/superpowers/specs/2026-07-09-kiana-personal-project-os-design.md -->

## Kiana 个人项目操作系统设计规格

日期：2026-07-09

详细参考扫描、逐仓库归因和 38 个 `reference/` 目录覆盖矩阵见：

- `docs/reference_audit/kiana_personal_project_os_reference_audit_2026-07-09.md`

### 1. Product Thesis

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

### 2. Golden Path User Journeys

#### 2.1 用户说“继续”

| 项 | 内容 |
| --- | --- |
| 输入 | `继续`、`接着做上次那个商业化缺口`、`恢复 workflow abc123` |
| Kiana 动作 | 读取 workflow/state/eventlog/progress；检查 git dirty state；刷新项目 fingerprint；比对 memory 是否 stale；选择下一个 Ready task |
| 产物 | `context_pack.md`、更新后的 `state.json`、`/project next` 输出、必要时 Block Report |
| 验证 | 能解释为什么选择这个 next task；不会覆盖用户未提交修改；如果 memory 与 live repo 冲突，以 live repo 为准 |

#### 2.2 从零启动项目

| 项 | 内容 |
| --- | --- |
| 输入 | 一段 PRD、issue、想法，或 `我要做一个个人项目 OS` |
| Kiana 动作 | Capture goal/criteria/constraints/NOT_BUILDING；构建 ContextPack；生成 WBS；创建 Kanban；写 Task Card；定义验证策略 |
| 产物 | `.kiana/workflows/<id>/workflow.yaml`、`task_plan.md`、`state.json`、`workpackets/*.json` |
| 验证 | 每个 task 都有 owner/status/dependencies/acceptance/verification；`/project board` 可恢复显示 |

#### 2.3 并行推进 3 个任务

| 项 | 内容 |
| --- | --- |
| 输入 | `/swarm dispatch --max-workers 3` 或 Project 中出现 3 个 Ready task |
| Kiana 动作 | 检查 task dependencies；计算 allowed/forbidden files；做 path lock；必要时创建 worktree；派发有限 worker；收集 ResultPacket |
| 产物 | 3 个 WorkPacket、3 个 ResultPacket、integration summary、冲突/无冲突判定 |
| 验证 | worker 文件边界不重叠；主 agent 集成后跑统一 verification profile；有冲突则进入 Review/Fix，不自动合并 |

#### 2.4 严格审计商用化缺口

| 项 | 内容 |
| --- | --- |
| 输入 | `/audit strict`、`这个项目离商用还差什么` |
| Kiana 动作 | live 扫描代码、脚本、CI、测试、release、docs、security、plugin/MCP/policy；查 fake/stub/TODO/hardcoded；对照 acceptance gates |
| 产物 | `findings.md`、`consolidated-review.md`、风险分级、P0/P1/P2 blocker 列表、中文汇报 |
| 验证 | 每个 finding 有文件/命令/证据；不能验证的项标 Blocked/Unknown；不把文档声明当生产完成 |

#### 2.5 EDA 项目审查

| 项 | 内容 |
| --- | --- |
| 输入 | `/eda review` + EasyEDA 工程、BOM、Gerber、坐标文件、需求约束 |
| Kiana 动作 | Capture 硬件需求；检查电源树、接口电平、保护、去耦、测试点；审查 BOM/DFM；生成 bring-up 计划 |
| 产物 | `eda_review.json`、`bom_risk.md`、`bringup-plan.md`、硬件 ReviewPacket |
| 验证 | 关键风险有来源；BOM 风险标注库存/封装/替代料/来源时间；P0 不自动画板或下单 |

### 3. Operating Modes and Router

#### 3.1 用户入口

| 入口 | 用途 | 第一版行为 |
| --- | --- | --- |
| `/quick` | 快速问答、解释代码、跑单个命令、单文件轻改 | 不创建 WorkflowRun，记录最小 evidence |
| `/task` | 单任务闭环 | 读上下文、改代码、跑测试、给证据 |
| `/project` | 长期项目推进 | 读取目标、roadmap、blocker，创建 WBS/Kanban |
| `/swarm` | 多 agent 并行 | 派发互不冲突的 WorkPacket，主 agent 集成 |
| `/audit` | 严格审计 | 查 fake/stub/TODO/硬编码/缺测试/安全与交付风险 |
| `/report` | 中文汇报 | 基于 evidence 生成进度、问题、下一步 |
| `/eda` | 电路电子设计 | 嘉立创/EasyEDA 资料审查、BOM、打样、bring-up |

#### 3.2 弹性运行档位

| 档位 | 典型输入 | 自动行为 | 退出标准 |
| --- | --- | --- | --- |
| L0 Answer | 解释、建议、单点查询 | 只读回答，不创建 artifact | 用户得到答案 |
| L1 Quick Action | 单命令、单文件轻改、小修复 | 直接执行，记录最小 evidence | diff 或命令结果明确 |
| L2 Task Loop | 跨文件 bugfix/feature/refactor | 创建 Task Card，执行验证闭环 | Task Done/Blocked 有证据 |
| L3 Project OS | 长期目标、PRD、商业化推进 | 创建 WorkflowRun、WBS、Kanban、ContextPack | 项目 board 可恢复推进 |
| L4 Bounded Swarm | 多个 Ready task 且边界清楚 | 拆 WorkPacket，有限 worker 并行，主 agent 集成 | 无冲突 diff，统一验证通过 |
| L5 High-Risk Governance | 发布、部署、权限、安全、资金、硬件打样 | Approval Gate + 审计 + rollback/bring-up plan | 用户批准且风险记录完整 |

#### 3.3 Intent Router Decision Rules

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

#### 3.4 `/eda` 领域边界

`/eda` 第一版是硬件项目操作系统，不是自动 PCB 黑箱：

| 能力 | 第一版行为 | 非目标 |
| --- | --- | --- |
| 需求捕获 | 抽取电源、接口、尺寸、成本、环境、认证、打样约束 | 不自动承诺器件可采购 |
| 原理图审查 | 检查电源树、接口电平、保护、去耦、连接器、调试口、测试点 | 不替代工程师最终电气审核 |
| BOM 风险 | 标记封装、库存、替代料、生命周期、嘉立创贴片可得性风险 | 不自动下单 |
| PCB/工艺审查 | 检查层数、线宽线距、孔径、阻抗、DFM、Gerber/坐标/BOM 文件完整性 | P0 不自动布线 |
| Bring-up 计划 | 生成上电顺序、测量点、示波器/万用表步骤、失败回滚 | 不跳过人工安全确认 |

### 4. Core Object Model

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

#### 4.1 Data Contract Examples

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

### 5. Persistent State and Recovery Model

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

#### 5.1 恢复规则

| 场景 | Kiana 行为 | 禁止行为 |
| --- | --- | --- |
| crash | 从 `state.json.last_event_id` 回放 eventlog，定位 last stable node | 不凭聊天记忆继续写 |
| interrupt | 写入 `interrupted` event，保留 current node 和 partial result | 不把 partial result 当 Done |
| resume | 检查 git head/dirty files/project fingerprint，再选择 next node | 不覆盖用户中途修改 |
| fork | 新 workflow 继承 parent goal/context，但创建独立 state/eventlog | 不共享可变 state |
| stale memory | 标注 stale，要求 live evidence 刷新后才能用于决策 | 不让旧 memory 覆盖当前文件 |
| dirty git state | 分类为 user_dirty、agent_dirty、mixed_dirty；需要隔离或确认 | 不自动 reset/checkout |
| worker failure | 收集 ResultPacket，按 retry policy 重试或 Block | 不无限重试 |

#### 5.2 一致性模型

- EventLog 是 append-only；`state.json` 是 eventlog 的 materialized view。
- Done/Blocked 状态必须能从 eventlog 重建。
- `workflow.yaml` 定义目标和策略，运行中只允许通过 decision event 变更。
- Memory 是辅助输入，不是 source of truth。
- Git live state、当前文件、测试结果、CI/release 输出优先级高于 memory 和旧报告。
- 并行 worker 只写自己的 allowed files；集成前必须做 touch-set audit。

### 6. Trust, Policy, and Approval Model

Kiana 的 trust 模型必须覆盖插件、MCP、hooks、worker、shell、文件编辑、网络访问。

#### 6.1 统一 PolicyDecision

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

#### 6.2 权限边界

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

#### 6.3 Approval Gate

需要用户明确批准的操作：

- push、merge、deploy、release、发布包、改 license。
- 删除/重写大量文件、清理未跟踪文件、reset/checkout。
- 访问私密外部服务、上传代码、调用不在 allowlist 的网络地址。
- 安装或启用未信任插件/MCP/hooks。
- 硬件打样、BOM 下单、涉及高压/电池/安全认证的建议执行。

### 7. Reference-Derived Capability Map

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

### 8. 38-Repo Coverage Matrix

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

### 9. P0 / P1 / P2 Implementation Slices

P0 不再按“能力列表”开工，而按 4 个可交付 milestone 实现。

#### P0-M1 WorkflowRun + Data Contracts

| 项 | 内容 |
| --- | --- |
| 命令 | `/project plan <goal>`、`/project status` |
| Schema | `workflow.yaml`、`state.json`、`eventlog.jsonl`、`task_card.json` |
| 测试 | 创建 workflow、resume 幂等、eventlog 回放生成 state |
| 验收输出 | `.kiana/workflows/<id>/` 可恢复，`/project status` 能解释 current node |

#### P0-M2 Project Board + Task Card

| 项 | 内容 |
| --- | --- |
| 命令 | `/project board`、`/project next`、`/project split <task_id>` |
| Schema | Task Card、Kanban state、dependencies |
| 测试 | Ready task 选择、dependency blocking、scope/NOT_BUILDING 保留 |
| 验收输出 | 一个长期目标能拆成 WBS/Kanban，并选择下一步 |

#### P0-M3 Evidence Ledger + Verification Gate

| 项 | 内容 |
| --- | --- |
| 命令 | `kiana validate`、`/audit strict`、`/report progress` |
| Schema | Evidence event、ResultPacket、VerificationPacket、ReviewPacket |
| 测试 | 命令 pass/fail 捕获、Blocked report、中文 report 从 evidence 生成 |
| 验收输出 | Done/Blocked 必须有证据，不能验证则不宣称完成 |

#### P0-M4 Context Pack + Recovery

| 项 | 内容 |
| --- | --- |
| 命令 | `/context packet`、`/context search <query>`、`/project resume <workflow_id>` |
| Schema | ContextPack、memory source/confidence、project fingerprint |
| 测试 | stale memory 降权、dirty git 分类、crash/interrupt/resume/fork |
| 验收输出 | 用户说“继续”时能恢复目标、风险、blocker、验证命令和 next task |

#### P1

- Bounded swarm：WorkPacket path lock、worker budget、integration gate。
- Repo map：GitNexus/graphify 风格 impact/trace/ranking。
- Memory ingest：Observation/Reasoning/Git memory proposal。
- Rules/skills 条件注入：mode/path/task/glob。
- `/eda review`：BOM/DFM/bring-up checklist engine。

#### P2

- Dashboard / workflow board / timeline。
- Browser companion / web shell。
- Cost / usage attribution。
- Plugin marketplace trust UX。
- Enterprise Offline 和 Cloud Workspace。

### 10. Acceptance Gates

#### 10.1 设计完成

- Product thesis 清楚：Kiana 是个人项目 OS，不是普通 coding CLI。
- Golden Path 覆盖继续、从零启动、并行、审计、EDA。
- Router 有升级/降级/冲突优先级。
- WorkflowRun、Task Card、WorkPacket、Evidence Event 有最小数据契约。
- Recovery 和 Trust/Policy 有明确行为。
- 38 个 reference 目录都有归因或明确边界。

#### 10.2 功能完成

- 有代码或 schema。
- 有测试或 smoke。
- 有 CLI/app-server 输出或用户可见行为。
- 有 failure path。
- 有 evidence ledger 记录。
- 有文档说明和边界说明。

#### 10.3 可长期自用

- “继续”类请求能恢复当前目标、blocker、验证命令和下一步。
- 常见项目改动可通过 `/task` 或 `/project` 闭环。
- 失败后能进入 Fix Loop，而不是丢状态。
- 生成报告时基于 live evidence。
- memory 不覆盖当前 repo 事实。

#### 10.4 可商用 / 可企业交付

- 权限、policy、plugin、MCP、hooks 都有 trust/provenance。
- release proof 不依赖口头声明。
- package lifecycle、签名、license、support、security proof 可验证。
- 完成状态来自代码/schema/test/smoke/docs/evidence，不来自 roadmap 文本。

### 11. Appendix: Detailed Reference Audit

完整参考审计、38 仓库覆盖矩阵、P0/P1/P2 机制来源、WorkflowRun 节点到模块映射，见：

- `docs/reference_audit/kiana_personal_project_os_reference_audit_2026-07-09.md`

主文档保留产品级决策和第一版实现规格；附录保留逐仓库归因和参考证据边界。

# 第二部分：深度软件逻辑与功能设计

---

<!-- source: docs/kiana_project_os/01-system-overview-and-product-logic.md -->

## Volume 01: 系统总览与产品逻辑

### 1. 定位

Kiana 的目标不是成为一个更会聊天的 CLI，也不是把现有 coding agent 换皮。它要成为个人项目操作系统，把“项目为什么做、现在做到哪、下一步做什么、怎么验证完成、长期记忆如何继承”这些问题变成软件对象和运行协议。

Kiana 的核心闭环：

```text
Goal
  -> Capture
  -> Context
  -> Plan
  -> Work
  -> Verify
  -> Review
  -> Report
  -> Learn
  -> Next Goal / Next Task
```

这个闭环必须可恢复、可审计、可拆分、可并行、可长期演进。

### 2. 产品层级

Kiana 包含三个产品层级：

| 层级 | 目标用户 | 核心价值 | 不做什么 |
| --- | --- | --- | --- |
| Personal CLI | 单人开发者、研究者、硬件项目操作者 | 本地项目推进、记忆、验证、汇报 | 不依赖云端才能工作 |
| Enterprise Offline | 企业内网/离线团队 | 权限、审计、插件治理、离线交付 | 不默认上传代码 |
| Cloud Workspace | 远程项目空间和 worker pool | 并发执行、团队视图、浏览器控制台 | P0 不优先 |

三个层级共享：

- Runtime event model。
- WorkflowRun。
- Task Card。
- Evidence Ledger。
- PolicyDecision。
- ContextPack。
- VerificationPacket。

差异只在 UI、部署方式、worker 执行位置和权限策略默认值。

### 3. 用户模型

#### 3.1 个人持续推进型用户

典型行为：

- 经常说“继续”。
- 同时维护多个项目。
- 需要保留长期目标和当前 blocker。
- 需要让 agent 不丢上下文。
- 需要中文汇报。

Kiana 必须提供：

- 项目级 memory。
- `.kiana/workflows/` 持久状态。
- `/project next`。
- `/report progress`。
- live evidence 检查。

#### 3.2 工程交付型用户

典型行为：

- 希望从需求到实现再到验证有闭环。
- 关心测试、构建、发布、回滚。
- 不接受“看起来完成”。

Kiana 必须提供：

- verification gate。
- review gate。
- release proof。
- blocker report。
- check profile。

#### 3.3 并行推进型用户

典型行为：

- 希望多个任务同时做。
- 担心并发冲突。
- 希望主 agent 管理集成。

Kiana 必须提供：

- WorkPacket。
- path lock。
- worker budget。
- integration gate。
- touch-set audit。

#### 3.4 EDA/硬件项目用户

典型行为：

- 有原理图、BOM、Gerber、CPL、嘉立创打样约束。
- 希望 AI 做审查和计划，而不是盲目画板。
- 需要 bring-up 步骤和风险清单。

Kiana 必须提供：

- `/eda review`。
- BOM risk。
- DFM checklist。
- bring-up plan。
- hardware approval gate。

### 4. 产品模式

#### 4.1 `/quick`

用途：

- 快速解释。
- 单命令。
- 单文件轻改。
- 低风险查询。

逻辑：

1. 不创建 WorkflowRun。
2. 可以创建轻量 evidence。
3. 如果发现跨文件修改或验证需求，升级到 `/task`。
4. 如果用户只问概念，保持 L0。

输出：

- 简短回答。
- 命令结果。
- 小 diff。
- 必要时升级说明。

#### 4.2 `/task`

用途：

- 单个 bugfix。
- 单个 feature slice。
- 单个 refactor。
- 单个测试补齐。

逻辑：

1. 构建 task-level ContextPack。
2. 定义 scope。
3. 执行修改。
4. 运行验证命令。
5. 生成 ResultPacket 和 VerificationPacket。
6. 根据结果 Done/Blocked/Fix Loop。

#### 4.3 `/project`

用途：

- 长期项目。
- 多阶段目标。
- 商业化推进。
- 论文/产品/硬件项目。

逻辑：

1. 创建 WorkflowRun。
2. Capture 目标和约束。
3. 建 WBS。
4. 生成 Kanban。
5. 选择 Ready task。
6. 每次继续都从状态恢复。

#### 4.4 `/swarm`

用途：

- 有多个互不冲突的 Ready task。
- 需要高并发推进。

逻辑：

1. 检查 task dependency。
2. 计算 allowed files。
3. 检查 path overlap。
4. 创建 WorkPacket。
5. 派发有限 worker。
6. 收集 ResultPacket。
7. 主 agent 集成。
8. 统一验证。

#### 4.5 `/audit`

用途：

- 严格审计。
- 商用化缺口。
- 发布前检查。
- 安全与质量风险。

逻辑：

1. live scan。
2. 查 fake/stub/TODO/hardcoded。
3. 查测试/构建/release/proof。
4. 查 policy/plugin/MCP/hooks。
5. 输出 finding。
6. finding 必须有 evidence。

#### 4.6 `/report`

用途：

- 中文汇报。
- 项目进展。
- 当前问题。
- 下一步路线。

逻辑：

1. 只从 evidence ledger、state、live scan、verified memory 取事实。
2. 区分完成、进行中、阻塞、未知。
3. 输出可直接口述的中文。
4. 不把计划当进展。

#### 4.7 `/eda`

用途：

- 电路电子设计审查。
- EasyEDA/嘉立创项目推进。
- BOM/DFM/bring-up。

逻辑：

1. 复用 WorkflowRun。
2. 领域规则包替换软件规则包。
3. Evidence 类型扩展到硬件文件。
4. 高风险操作必须 approval。

### 5. 软件边界

Kiana Core 不应直接关心具体 UI。

Core 只输出：

- events。
- state。
- packets。
- reports。
- policy decisions。
- review findings。

CLI/TUI/Web 只是 projection。

### 6. 端到端闭环

#### 6.1 从目标到任务

输入：

- 用户自然语言。
- PRD。
- issue。
- roadmap。
- 文档。

输出：

- Goal。
- Success Criteria。
- Constraints。
- NOT_BUILDING。
- WBS。
- Task Cards。

失败：

- 目标不清楚：Clarify Gate。
- 范围过大：Split Goal。
- 高风险：Approval Required。
- 不安全：Cancel/Block。

#### 6.2 从任务到执行

输入：

- Task Card。
- ContextPack。
- WorkPacket。

输出：

- changed files。
- command results。
- errors。
- ResultPacket。

失败：

- scope violation。
- dirty state conflict。
- command failure。
- missing dependency。
- worker timeout。

#### 6.3 从执行到完成

输入：

- ResultPacket。
- verification profile。
- acceptance criteria。

输出：

- VerificationPacket。
- ReviewPacket。
- consolidated review。
- delivery summary。

完成条件：

- verification pass。
- acceptance satisfied。
- no blocking findings。
- evidence written。

#### 6.4 从完成到学习

输入：

- final packets。
- eventlog。
- review findings。

输出：

- learnings.md。
- memory proposal。
- stale memory notes。
- next task。

禁止：

- 自动把低置信结论写进长期 memory。
- 删除用户任务。
- 把失败经验伪装成规则。

### 7. 关键产品判断

#### 7.1 为什么不是普通 coding CLI

普通 coding CLI 关注：

- 当前 prompt。
- 当前工具调用。
- 当前 diff。
- 当前回答。

Kiana 关注：

- 项目目标。
- 持久状态。
- 证据账本。
- 任务板。
- 长期记忆。
- 并发协作。
- 完成门禁。
- 汇报和学习。

#### 7.2 为什么先做 `/project`

因为 `/project` 是差异化核心：

- `/quick` 和 `/task` 已经是常规 coding agent 能力。
- `/project` 才能形成长期闭环。
- `/swarm` 需要 Project 的 Task Card 和 WorkPacket。
- `/report` 需要 Project 的 evidence ledger。
- `/eda` 需要 Project 的流程和 gate。

#### 7.3 为什么不先做 Dashboard

Dashboard 没有核心协议时只会变成展示壳。

正确顺序：

1. WorkflowRun。
2. State/EventLog。
3. Task Card。
4. Evidence Ledger。
5. Context/Recovery。
6. CLI/TUI projection。
7. Web/dashboard。

#### 7.4 为什么 EDA P0 不自动画板

硬件错误成本高，且需要专业判断。

P0 更有价值的是：

- 需求捕获。
- 资料完整性。
- BOM 风险。
- DFM 检查。
- Bring-up 计划。
- 风险归档。

自动画板和自动下单应进入更晚阶段，并带强 approval。

### 8. 系统完整性检查

Kiana 的每个核心功能都要能回答：

- 输入是什么。
- 输出是什么。
- 状态存在哪里。
- 失败如何记录。
- 什么时候需要用户。
- 如何恢复。
- 如何验证。
- 如何汇报。
- 如何写入学习。

如果一个能力不能回答这些问题，就不能进入 P0。


---

<!-- source: docs/kiana_project_os/02-workflow-runtime-and-state-machine.md -->

## Volume 02: Workflow Runtime 与状态机

### 1. WorkflowRun 定义

WorkflowRun 是 Kiana 中复杂任务的持久运行单元。它不是一次聊天，也不是一个模型 turn，也不是一个 git branch。它是一个可恢复、可审计、可分解、可验证的项目推进容器。

WorkflowRun 负责承载：

- 目标。
- 约束。
- 运行状态。
- DAG 节点。
- 任务卡。
- 事件日志。
- 执行结果。
- 验证结果。
- 审查结果。
- 学习记录。

### 2. WorkflowRun 与其他对象的关系

```text
Session
  has many WorkflowRun

WorkflowRun
  has many TaskCard
  has many Event
  has many WorkPacket
  has many ResultPacket
  has many VerificationPacket
  has many ReviewPacket

TaskCard
  may have one or more WorkPacket

WorkPacket
  produces ResultPacket

ResultPacket
  feeds VerificationPacket

VerificationPacket
  feeds ReviewPacket

ReviewPacket
  feeds Ship or Fix Loop
```

### 3. 目录结构

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
  reports/
  pr/
  learnings.md
```

#### 3.1 `workflow.yaml`

职责：

- 保存运行元数据。
- 保存目标和模式。
- 保存 approval 策略。
- 保存 verification profile。
- 保存 artifact 目录。

它是相对稳定的配置文件，运行中不应频繁改写。

允许变更：

- 通过 decision event 修改 goal。
- 通过 approval event 修改允许操作。
- 通过 plan revision 修改 DAG template。

禁止：

- 工具执行时直接改 goal。
- worker 自己改 workflow policy。
- 手写覆盖导致 eventlog 无法解释。

#### 3.2 `state.json`

职责：

- 保存当前节点。
- 保存 Kanban 状态。
- 保存 project fingerprint。
- 保存 memory stale 状态。
- 保存 last_event_id。

`state.json` 是 eventlog 的 materialized view。

要求：

- 可以通过 eventlog 重建。
- 每次写入带 last_event_id。
- crash 后可以校验是否落后。

#### 3.3 `eventlog.jsonl`

职责：

- append-only。
- 记录所有状态变化。
- 记录所有 gate 决策。
- 记录所有 command result。
- 记录所有 approval。

事件类型：

- `workflow_created`
- `capture_completed`
- `context_pack_built`
- `task_created`
- `task_status_changed`
- `workpacket_created`
- `worker_started`
- `command_started`
- `command_completed`
- `result_packet_created`
- `verification_completed`
- `review_completed`
- `approval_requested`
- `approval_granted`
- `approval_rejected`
- `blocked`
- `resumed`
- `forked`
- `learned`

### 4. Workflow 节点

#### 4.1 Runtime Init

输入：

- user request。
- cwd。
- current git state。
- selected mode。

动作：

1. 生成 workflow_id。
2. 创建 artifact directory。
3. 写 workflow.yaml。
4. 写 initial state.json。
5. 写 workflow_created event。

失败：

- artifact directory 已存在：生成新 id 或 resume。
- 无写权限：Block。
- cwd 非项目目录：Ask/Block。

#### 4.2 Capture

提取：

- goal。
- success criteria。
- constraints。
- risk。
- approval needs。
- NOT_BUILDING。
- input type。

Gate：

- clear -> Context Intake。
- unclear -> Clarify。
- too large -> Split。
- high risk -> Approval Required。
- unsafe -> Cancel/Block。

#### 4.3 Context Intake

动作：

1. Restore previous state if resuming。
2. Load memory summary。
3. Check project fingerprint。
4. Targeted retrieval。
5. Detect stack。
6. Detect scripts/tests/CI。
7. Build ContextPack。

Gate：

- fresh -> Research。
- partial/stale -> Refresh。
- major project change -> Rebuild memory。
- too large -> Compact。
- dirty git -> Isolation Decision。

#### 4.4 Research

动作：

- search code/docs/logs/issues。
- search existing patterns。
- trace affected symbols。
- record findings。

Gate：

- enough -> Design。
- missing facts -> more research。
- conflict -> resolve。
- critical unknown -> ask or mark unknown。

#### 4.5 Design Candidate

动作：

- generate design candidate。
- map affected modules。
- estimate risk surface。
- define interfaces。
- define test surface。
- define compatibility impact。
- define NOT_BUILDING。

Gate：

- accepted -> Plan。
- tradeoff -> Discuss。
- mismatch -> Capture。
- high architecture risk -> Alternative。
- requires approval -> Approval。

#### 4.6 Plan

动作：

- create/update task_plan.md。
- decompose tasks。
- define dependencies。
- define verification strategy。
- define rollback。
- define review strategy。
- define DAG node dependencies。

Gate：

- executable -> Plan Confirmation。
- too vague -> Re-plan。
- missing tests -> Add test strategy。
- missing verification -> Add commands。
- missing scope limits -> Add NOT_BUILDING。

#### 4.7 Plan Confirmation

动作：

- verify referenced files exist。
- verify target patterns still exist。
- verify commands available。
- verify branch/worktree state。
- verify scope limits captured。
- verify acceptance criteria testable。

Gate：

- continue -> WorkPacket。
- repo changed -> Context Intake。
- needs user decision -> Discuss。
- invalid -> Plan。

#### 4.8 WorkPacket

动作：

- define task goal。
- define in-scope。
- define out-of-scope。
- define allowed/forbidden files。
- attach tests。
- attach verification commands。
- attach rollback。
- attach risk flags。
- attach review focus。
- attach retry policy。

Gate：

- valid -> Router。
- missing bounds -> Plan。
- risky -> Approval。
- invalid dependency -> Plan。

#### 4.9 Router

动作：

- classify task type。
- select commands/skills。
- select specialist agents。
- select rules pack。
- select verification profile。
- select execution mode。

Gate：

- single -> Execute。
- parallel -> Dispatch Bounded Swarm。
- missing rule pack -> Generic Safe Rules。
- needs isolation -> Worktree。

#### 4.10 Execute

动作：

- read WorkPacket。
- lock scope。
- write failing test if required。
- minimal implementation。
- bounded edit。
- targeted check after file change。
- collect changed files。
- detect out-of-scope diff。

Result：

- step done -> Quality Gate。
- failed -> Fix Loop。
- requirement problem -> Plan。
- architecture problem -> Discuss。
- risk discovered -> Approval。

#### 4.11 Quality Gate

检查：

- format。
- lint。
- type。
- build。
- static security。
- dependency/license。

结果：

- pass -> Behavior Verify。
- fail -> Fix Loop。
- security finding -> Security Fix/Block。

#### 4.12 Behavior Verify

检查：

- targeted tests。
- regression tests。
- acceptance criteria。
- user goal satisfaction。
- NOT_BUILDING not violated。

结果：

- pass -> Review Scope。
- test failed -> Fix Loop。
- tests missing -> Add Tests。
- cannot verify -> Block。

#### 4.13 Review Scope

动作：

- collect changed files。
- collect diff summary。
- collect rules。
- copy NOT_BUILDING。
- add review focus。
- add verification evidence。
- write review/scope.md。

#### 4.14 Multi Review

审查角色：

- Product Review。
- Architecture Review。
- Code Review。
- Security Review。
- QA/Test Coverage Review。
- Docs/DX Review。
- Error Handling Review。
- Red Team Review。

ReviewPacket 字段：

- reviewer。
- focus。
- findings。
- severity。
- confidence。
- evidence。
- recommendation。

#### 4.15 Review Triage

finding 决策：

- FIX。
- SKIP with reason。
- BLOCKED。
- ASK USER。

禁止：

- blocking finding 直接跳过。
- 没有 skip reason。
- 把 review 文本当 verification。

#### 4.16 Fix Loop

动作：

1. read failure/finding packet。
2. classify root cause。
3. apply minimal fix。
4. record fix attempt。
5. check scope did not expand。

Gate：

- fixed -> Quality Gate。
- retryable -> retry。
- need decision -> Ask。
- need re-plan -> Plan。
- budget exceeded -> Block。

#### 4.17 Ship

动作：

- prepare delivery summary。
- summarize changed files。
- summarize tests/checks。
- summarize review findings。
- summarize risks/follow-ups。

Gate：

- report only -> Learn。
- create PR draft -> PR artifact。
- push/merge/deploy -> Approval。
- rejected -> Block。

#### 4.18 Learn

动作：

- append eventlog。
- update progress。
- write learnings。
- propose memory updates。
- update project fingerprint。
- update workflow metrics。

Gate：

- next WorkPacket -> Loop Controller。
- next Goal -> Loop Controller。
- done -> Completed。

### 5. 状态集合

#### 5.1 WorkflowRun 状态

| 状态 | 含义 |
| --- | --- |
| `created` | artifact directory 已创建 |
| `capturing` | 正在抽取目标和约束 |
| `contextualizing` | 正在构建上下文 |
| `researching` | 正在查证 |
| `designing` | 正在形成设计候选 |
| `planning` | 正在拆任务 |
| `confirming_plan` | 正在确认计划可执行 |
| `ready` | 有 Ready task |
| `executing` | 正在执行 |
| `verifying` | 正在验证 |
| `reviewing` | 正在审查 |
| `fixing` | 正在修复 |
| `blocked` | 需要用户或外部状态 |
| `completed` | 完成 |
| `cancelled` | 取消 |

#### 5.2 Task 状态

| 状态 | 含义 |
| --- | --- |
| `draft` | 未确认 |
| `ready` | 依赖满足，可执行 |
| `doing` | 正在执行 |
| `review` | 等待审查 |
| `blocked` | 被阻塞 |
| `done` | 完成 |
| `cancelled` | 取消 |

#### 5.3 Worker 状态

| 状态 | 含义 |
| --- | --- |
| `assigned` | 已收到 WorkPacket |
| `running` | 正在执行 |
| `waiting` | 等待命令或资源 |
| `reporting` | 正在提交 ResultPacket |
| `failed` | 执行失败 |
| `completed` | 执行完成 |
| `reaped` | 被回收 |

### 6. 允许迁移

WorkflowRun 允许迁移：

```text
created -> capturing
capturing -> contextualizing
contextualizing -> researching
researching -> designing
designing -> planning
planning -> confirming_plan
confirming_plan -> ready
ready -> executing
executing -> verifying
verifying -> reviewing
reviewing -> fixing
fixing -> verifying
reviewing -> completed
any -> blocked
blocked -> contextualizing
blocked -> planning
blocked -> ready
any -> cancelled
```

禁止迁移：

- `created -> completed`
- `executing -> completed` without verification。
- `verifying -> completed` without review decision。
- `blocked -> done` without unblock evidence。
- `worker failed -> integrated` without triage。

### 7. Crash Recovery

恢复流程：

1. 读取 workflow.yaml。
2. 读取 state.json。
3. 读取 eventlog.jsonl。
4. 校验 `state.last_event_id` 是否存在。
5. 从 eventlog 重建 state。
6. 比对重建 state 和 state.json。
7. 如果不一致，以 eventlog 为准并写 `state_repaired` event。
8. 检查 git fingerprint。
9. 若 dirty state 与记录不同，进入 Dirty State Gate。
10. 选择 next node。

### 8. Dirty State Gate

分类：

- `clean`：可继续。
- `agent_dirty`：Kiana 上次修改未完成，可恢复或回滚。
- `user_dirty`：用户中途修改，必须保护。
- `mixed_dirty`：需要隔离或确认。

行为：

| 分类 | 行为 |
| --- | --- |
| clean | continue |
| agent_dirty | resume/fix/rollback |
| user_dirty | rebuild ContextPack，避免覆盖 |
| mixed_dirty | ask 或 create worktree |

### 9. Fork Model

fork 用于：

- 尝试替代方案。
- 从 blocked 状态拆出探索。
- 并行实验。

规则：

- fork 创建新 workflow_id。
- parent workflow 不共享 mutable state。
- fork 复制 goal/context snapshot。
- fork 写 parent_workflow_id。
- merge/final adoption 需要 decision event。

### 10. Event Sourcing 原则

Kiana 不需要一开始实现完整事件溯源框架，但必须遵守：

- 关键状态变化写 event。
- event append-only。
- state 可重建。
- packet 可追溯。
- report 可追溯到 evidence。

这样后续 dashboard、cloud workspace、team runtime 都能复用同一条事件流。


---

<!-- source: docs/kiana_project_os/03-intent-router-and-command-semantics.md -->

## Volume 03: Intent Router 与命令语义

### 1. Router 目标

Intent Router 的职责不是“猜用户想要什么”这么简单。它要把用户输入映射到合适的执行档位、命令语义、风险策略和证据要求。

Router 必须同时做到：

- 小事不重流程。
- 大事不轻率。
- 高风险不绕过审批。
- 可以升级。
- 可以降级。
- 可以解释决策原因。

### 2. 输入信号

Router 使用以下信号：

| 信号 | 来源 | 用途 |
| --- | --- | --- |
| explicit command | 用户输入 `/project`、`/audit` 等 | 作为 mode hint |
| natural language intent | 用户自然语言 | 分类任务类型 |
| repo state | git、文件、脚本、测试 | 估计复杂度 |
| workflow state | `.kiana/workflows` | 判断是否 resume |
| memory state | source/confidence/stale | 辅助恢复 |
| risk terms | deploy、delete、secret、PCB order | 升级到 L5 |
| artifact presence | PRD、BOM、Gerber、issue | 选择领域模式 |
| dirty state | git dirty/untracked | 决定是否隔离 |
| task board | ready tasks | 决定是否可 swarm |

### 3. 输出

Router 输出 `RouteDecision`：

```json
{
  "schema_version": "kiana.route_decision.v1",
  "input_id": "turn_001",
  "requested_mode": "auto",
  "selected_mode": "project",
  "level": "L3",
  "domain": "software",
  "reasons": [
    "用户目标影响多个模块",
    "需要长期状态和验证门槛",
    "存在可拆分任务"
  ],
  "upgraded_from": "task",
  "downgraded_from": null,
  "requires_approval": false,
  "next_command": "/project plan",
  "evidence_requirement": "workflow_artifact"
}
```

### 4. L0-L5 档位

#### 4.1 L0 Answer

触发：

- 解释概念。
- 纯建议。
- 不需要工具。
- 不需要写文件。

输出：

- 直接回答。
- 可选引用现有文档。

禁止：

- 创建 WorkflowRun。
- 写文件。
- 宣称验证过。

#### 4.2 L1 Quick Action

触发：

- 单命令。
- 单文件小改。
- 低风险查询。

输出：

- command result。
- small diff。
- minimal evidence。

升级条件：

- 涉及多文件。
- 需要测试。
- 发现目标比输入复杂。
- 需要长期状态。

#### 4.3 L2 Task Loop

触发：

- bugfix。
- feature slice。
- refactor。
- test addition。

输出：

- Task Card。
- ResultPacket。
- VerificationPacket。

升级条件：

- 任务依赖多个阶段。
- 需要 WBS。
- 需要跨会话继续。
- 需要多个 worker。

#### 4.4 L3 Project OS

触发：

- “继续”。
- PRD/roadmap。
- 多模块目标。
- 商业化推进。
- 论文/硬件长期项目。

输出：

- WorkflowRun。
- WBS。
- Kanban。
- Task Cards。
- Evidence Ledger。

降级条件：

- 用户只是问只读状态。
- 没有可执行动作。
- 任务实际很小。

#### 4.5 L4 Bounded Swarm

触发：

- Project 中有多个 Ready task。
- 文件边界清楚。
- 用户允许并行。
- 验证可以统一集成。

输出：

- WorkPackets。
- worker assignments。
- path lock table。
- integration report。

禁止：

- 自动派发冲突文件。
- worker 自由扩大 scope。
- worker 直接 merge。

#### 4.6 L5 High-Risk Governance

触发：

- push/merge/deploy/release。
- 删除/重写大量文件。
- secrets/credentials。
- network upload。
- plugin/MCP/hook trust change。
- hardware order。
- high-voltage/battery/safety advice execution。

输出：

- Approval request。
- risk report。
- rollback plan。
- audit event。

禁止：

- 静默执行。
- 用用户过去的偏好替代本次 approval。

### 5. 显式命令语义

#### 5.1 `/quick <prompt>`

语义：

- 用户要求快速处理。
- Router 尽量保持 L0/L1。
- 如果必须升级，必须说明原因。

错误：

- 需要写多文件：返回升级建议或直接升级。
- 需要审批：进入 L5。

#### 5.2 `/task <goal>`

语义：

- 创建或复用单任务执行上下文。
- 不默认创建完整 Project WBS。

动作：

1. Build task ContextPack。
2. Define scope。
3. Execute。
4. Verify。
5. Report。

#### 5.3 `/project plan <goal>`

语义：

- 创建 WorkflowRun。
- Capture goal。
- 生成 WBS/Kanban。

输出：

- workflow_id。
- task_plan.md。
- state.json。
- board summary。

#### 5.4 `/project board`

语义：

- 展示当前项目任务板。

输出：

- Ready。
- Doing。
- Review。
- Blocked。
- Done。
- 每个任务的 evidence 状态。

#### 5.5 `/project next`

语义：

- 选择下一步最应该做的 task。

选择规则：

1. Ready 状态。
2. 依赖满足。
3. 最高优先级。
4. 最低阻塞风险。
5. 能产生最大推进证据。

#### 5.6 `/project split <task_id>`

语义：

- 把过大的 task 拆小。

输出：

- child tasks。
- dependency edges。
- parent task 状态更新。

#### 5.7 `/project resume <workflow_id>`

语义：

- 恢复指定 WorkflowRun。

动作：

1. Load workflow。
2. Replay eventlog。
3. Check dirty state。
4. Refresh ContextPack。
5. Select next node。

#### 5.8 `/swarm dispatch --max-workers N`

语义：

- 派发最多 N 个 worker。

前置条件：

- Project mode。
- 至少 2 个 Ready task。
- path lock 无冲突。
- verification profile 存在。

#### 5.9 `/audit strict`

语义：

- 执行严格审计。

检查：

- fake/stub/TODO。
- hardcoded。
- test gap。
- release proof。
- schema drift。
- policy/trust gap。
- security issue。

#### 5.10 `/report progress`

语义：

- 生成中文进度报告。

来源：

- Evidence Ledger。
- state.json。
- VerificationPacket。
- ReviewPacket。
- live scan。

禁止：

- 用 stale memory 直接生成当前状态。

#### 5.11 `/eda review`

语义：

- 硬件项目审查。

输入：

- EasyEDA project。
- schematic export。
- BOM。
- Gerber。
- CPL。
- constraints。

输出：

- eda_review.json。
- bom_risk.md。
- bringup-plan.md。

### 6. 升级规则

| 从 | 到 | 条件 |
| --- | --- | --- |
| L0 | L1 | 需要读取/运行命令 |
| L1 | L2 | 需要改文件并验证 |
| L2 | L3 | 涉及多阶段、多模块、长期状态 |
| L3 | L4 | 多个 Ready task 可安全并行 |
| any | L5 | 高风险操作 |
| any | EDA domain | 硬件/PCB/BOM/Gerber/EasyEDA 信号 |

### 7. 降级规则

| 从 | 到 | 条件 |
| --- | --- | --- |
| L3 | L0 | 用户只是问状态/解释 |
| L3 | L2 | 只有一个清晰 task |
| L4 | L3 | path lock 冲突或依赖不满足 |
| L5 | lower | 用户取消高风险动作，仅保留只读分析 |

### 8. 冲突优先级

当信号冲突：

1. Safety/Policy wins。
2. Explicit user command wins over auto classification。
3. Live repo state wins over memory。
4. Project state wins over single-turn guess。
5. Lower-risk mode wins when evidence is insufficient。

### 9. Router 自解释

每次非显式 obvious 的路由都要可解释：

- selected mode。
- reasons。
- risk flags。
- why not lower。
- why not higher。
- next artifact。

示例：

```text
选择 /project：
- 目标是长期商业化推进，不是单次修复。
- 需要恢复 blocker 和验证命令。
- 涉及 docs、scripts、schema、release 多个面。
- 当前没有足够安全边界进入 /swarm。
```

### 10. Router 失败模式

#### 10.1 误升级

表现：

- 小问题进入完整 WorkflowRun。
- 用户体验变慢。

修正：

- 增加降级。
- 快速输出。
- 只创建 minimal evidence。

#### 10.2 误降级

表现：

- 长期目标只做了局部回答。
- 没有状态。

修正：

- 对“继续”“完整”“商用化”“项目”设复杂度下限。

#### 10.3 风险漏判

表现：

- 执行了高风险操作。

修正：

- policy gate 在 tool 层再次检查。
- Router 决策不能成为最终授权。

#### 10.4 EDA 误判

表现：

- 把普通软件任务当硬件。
- 把硬件任务当普通项目。

修正：

- EDA domain 需要关键词 + artifact 或用户显式命令之一。


---

<!-- source: docs/kiana_project_os/04-project-orchestrator-wbs-kanban.md -->

## Volume 04: Project Orchestrator、WBS 与 Kanban

### 1. Project Orchestrator 职责

Project Orchestrator 是 `/project` 的核心。它把长期目标变成可执行、可恢复、可验证的项目状态。

职责：

- 维护 WorkflowRun。
- 抽取 goal/success criteria。
- 建 WBS。
- 建 Task Card。
- 维护 Kanban。
- 选择 next task。
- 控制 scope。
- 生成 progress report。

不负责：

- 直接执行所有工具。
- 自己绕过 policy。
- 替 worker 写代码。
- 把失败隐藏成进展。

### 2. WBS 层级

Kiana 使用三层 WBS：

```text
Project Goal
  -> Workstream
    -> Milestone
      -> Task Card
```

#### 2.1 Project Goal

字段：

- title。
- objective。
- why。
- success criteria。
- constraints。
- NOT_BUILDING。
- risk profile。

#### 2.2 Workstream

用于区分方向：

- Runtime。
- Project OS。
- Memory。
- Repo Intelligence。
- Swarm。
- Policy。
- EDA。
- UI。
- Docs。

字段：

- id。
- name。
- purpose。
- owner mode。
- priority。
- dependencies。

#### 2.3 Milestone

Milestone 是可验收阶段。

字段：

- id。
- workstream。
- output。
- acceptance。
- verification。
- risks。

#### 2.4 Task Card

Task Card 是最小执行单位。

字段：

- task_id。
- title。
- objective。
- scope。
- dependencies。
- allowed_paths。
- forbidden_paths。
- acceptance_criteria。
- verification_commands。
- status。
- evidence。

### 3. Kanban 状态

状态：

- Draft。
- Ready。
- Doing。
- Review。
- Blocked。
- Done。
- Cancelled。

#### 3.1 Draft

含义：

- 已识别，但不可执行。

进入条件：

- WBS 初步拆分。
- 缺少验收标准。
- 缺少 scope。

离开条件：

- 补齐 dependencies。
- 补齐 verification。
- 补齐 allowed/forbidden paths。

#### 3.2 Ready

含义：

- 可以被执行或派发。

进入条件：

- dependencies done。
- scope 清楚。
- verification 存在。
- 无 approval blocker。

#### 3.3 Doing

含义：

- 正在执行。

进入条件：

- 被主 agent 或 worker claim。
- path lock 成功。

#### 3.4 Review

含义：

- 执行完成，等待审查/验证/集成。

进入条件：

- ResultPacket 存在。
- 初步 checks 完成。

#### 3.5 Blocked

含义：

- 需要外部输入或风险无法继续。

blocked reasons：

- user decision。
- missing dependency。
- failing test unclear。
- dirty git conflict。
- external service unavailable。
- approval required。

#### 3.6 Done

含义：

- 验收完成。

进入条件：

- VerificationPacket pass。
- no blocking ReviewPacket。
- Evidence Ledger 写入。

### 4. `/project plan`

输入：

- goal text。
- optional PRD path。
- optional issue id。
- optional constraints。

动作：

1. Capture。
2. Build goal object。
3. Extract success criteria。
4. Extract NOT_BUILDING。
5. Identify workstreams。
6. Create milestones。
7. Create initial tasks。
8. Write task_plan.md。
9. Write state.json。

输出：

- workflow_id。
- board summary。
- first Ready task。
- risks。
- next command。

失败：

- goal unclear -> clarify。
- goal too large -> split。
- high risk -> approval。

### 5. `/project board`

输出应包含：

- workflow_id。
- current goal。
- current node。
- each status column。
- task id/title。
- priority。
- blocker。
- last evidence。

示例输出结构：

```text
Workflow: wf_20260709_001
Goal: Kiana 个人项目 OS

Ready
  task_001 WorkflowRun schema
  task_002 Evidence Ledger

Doing
  task_003 Router rules

Blocked
  task_004 Plugin trust model - needs policy decision

Done
  task_000 Reference audit
```

### 6. `/project next`

选择算法：

1. 过滤 Ready task。
2. 排除 blocked dependencies。
3. 排除 path lock 冲突。
4. 按 priority 排序。
5. 按 critical path 排序。
6. 按 evidence value 排序。
7. 选择最小可交付 slice。

输出：

- selected task。
- why selected。
- why not others。
- required context。
- suggested command。

### 7. `/project split`

触发：

- task 太大。
- task scope 不清。
- task 不能在一次验证中完成。
- task 涉及多个 owners。

拆分规则：

- 每个 child task 有独立验收。
- child task 可以独立验证。
- child task 文件范围尽量不重叠。
- parent task 状态转为 blocked 或 decomposed。

### 8. Scope 管理

Task Card 必须区分：

- in_scope。
- out_of_scope。
- NOT_BUILDING。
- allowed_paths。
- forbidden_paths。

区别：

- `out_of_scope` 是需求边界。
- `NOT_BUILDING` 是明确不做的产品承诺。
- `allowed_paths` 是执行边界。
- `forbidden_paths` 是工具/worker 禁止触碰的边界。

### 9. 依赖模型

依赖类型：

- blocks。
- requires_decision。
- requires_artifact。
- requires_verification。
- requires_approval。
- related_but_not_blocking。

依赖必须可解释：

- 谁依赖谁。
- 为什么依赖。
- 完成条件是什么。

### 10. 优先级模型

优先级信号：

- 用户显式优先级。
- unblock value。
- risk reduction。
- critical path。
- verification availability。
- implementation size。
- dependency fanout。

优先级不应只按“看起来重要”。

### 11. Board 与 WorkflowRun 的一致性

Board 是 state projection，不是独立 source of truth。

更新路径：

```text
eventlog.jsonl -> state.json -> board render
```

禁止：

- 手动只改 board 不写 event。
- Done column 没有 evidence。
- Blocked column 没有 reason。

### 12. Project Report

报告结构：

- 背景。
- 当前目标。
- 已完成。
- 正在做。
- 阻塞。
- 风险。
- 下一步。
- 需要用户决策。

事实来源：

- state.json。
- Evidence Ledger。
- ReviewPacket。
- VerificationPacket。
- live scan。

禁止：

- 用 roadmap 替代进度。
- 用旧 memory 替代当前状态。


---

<!-- source: docs/kiana_project_os/05-evidence-verification-review-ledger.md -->

## Volume 05: Evidence、Verification、Review 与 Ledger

### 1. Evidence Ledger 定义

Evidence Ledger 是 Kiana 的完成证明系统。它回答：

- 做了什么。
- 改了哪里。
- 跑了什么命令。
- 结果是什么。
- 谁审查了。
- 哪些风险还在。
- 为什么可以说 Done。

没有 evidence，Kiana 不能宣称完成。

### 2. Evidence 类型

| 类型 | 说明 |
| --- | --- |
| command_result | 命令执行结果 |
| diff_summary | 文件修改摘要 |
| test_result | 测试结果 |
| build_result | 构建结果 |
| lint_result | lint/format/typecheck |
| security_result | 安全扫描/审查 |
| review_finding | 审查发现 |
| approval_record | 用户批准 |
| blocker_record | 阻塞原因 |
| artifact_check | 文件/产物完整性检查 |
| eda_check | 硬件资料审查 |
| memory_decision | memory 写入/拒绝原因 |

### 3. Evidence Event 字段

必需字段：

- event_id。
- workflow_id。
- timestamp。
- kind。
- status。
- summary。
- source。
- evidence payload。

可选字段：

- task_id。
- workpacket_id。
- command。
- changed_files。
- reviewer。
- confidence。
- severity。
- next_action。

### 4. Verification Gate

Verification Gate 是自动/半自动验证层。

检查类型：

- format。
- lint。
- type。
- build。
- unit tests。
- integration tests。
- smoke tests。
- security scan。
- dependency/license。
- acceptance criteria。

#### 4.1 Profile

不同任务使用不同 profile：

| Profile | 用途 |
| --- | --- |
| quick | L1 小任务 |
| task_default | L2 单任务 |
| project_p0 | P0 项目功能 |
| release | 发布前 |
| audit_strict | 严格审计 |
| eda_review | 硬件审查 |

#### 4.2 Pass 条件

Verification pass 需要：

- 必需命令成功。
- acceptance criteria 满足。
- NOT_BUILDING 未违反。
- 无阻断错误。
- evidence 写入。

#### 4.3 Fail 条件

Verification fail：

- 命令失败。
- 测试失败。
- build 失败。
- 缺少必要验证。
- acceptance 未满足。
- 发现 scope violation。

fail 后进入：

- Fix Loop。
- Add Tests。
- Re-plan。
- Block。

### 5. Review Gate

Review Gate 是判断“验证通过是否足够”的层。

Review 类型：

- Product Review。
- Architecture Review。
- Code Review。
- Security Review。
- QA Review。
- Docs/DX Review。
- Error Handling Review。
- Red Team Review。
- EDA Review。

#### 5.1 Finding 字段

- finding_id。
- severity。
- confidence。
- component。
- evidence。
- recommendation。
- decision。

Severity：

- blocker。
- high。
- medium。
- low。
- note。

Decision：

- fix。
- skip。
- block。
- ask_user。

#### 5.2 Blocking Finding

blocking finding 不能自动跳过。

允许路径：

- fix。
- block。
- ask user。
- 用户明确接受风险。

必须记录：

- reason。
- evidence。
- owner。
- next action。

### 6. ResultPacket

ResultPacket 是执行结果，不是完成证明。

字段：

- workpacket_id。
- task_id。
- changed_files。
- commands_run。
- errors。
- notes。
- scope_deviations。
- suggested_next。

ResultPacket 后必须经过 Verification。

### 7. VerificationPacket

字段：

- verification_id。
- task_id。
- profile。
- checks。
- pass_count。
- fail_count。
- skipped_count。
- blocked_count。
- final_status。
- evidence_events。

Skipped 必须有 reason。

### 8. ReviewPacket

字段：

- review_id。
- reviewer_type。
- input_scope。
- findings。
- score。
- blocking_count。
- recommendation。

ReviewPacket 不等于测试。

### 9. Consolidated Review

Review synthesis 做：

- deduplicate findings。
- severity normalize。
- confidence normalize。
- assign owner。
- decide fix/skip/block/ask。
- produce consolidated-review.md。

禁止：

- 用平均分掩盖 blocker。
- 合并后丢 evidence。

### 10. Strict Audit

`/audit strict` 检查：

- fake implementation。
- stub。
- placeholder。
- hardcoded path。
- missing tests。
- schema drift。
- unverified release claim。
- policy gap。
- plugin trust gap。
- MCP exposure gap。
- hook fail-open。
- memory stale。
- TODO in production path。

每项 finding 需要：

- file/path。
- evidence。
- risk。
- suggested fix。
- priority。

### 11. Report Progress

`/report progress` 从 ledger 生成：

- 项目背景。
- 当前阶段。
- 已完成。
- 未完成。
- 阻塞。
- 风险。
- 下一步。

报告语气：

- 中文。
- 事实优先。
- 可直接汇报。
- 不夸大。

### 12. Evidence Retention

保留策略：

- eventlog 永久保留。
- command stdout 可截断。
- 大文件输出保存 tail 和摘要。
- 关键 proof 保存完整路径。
- secret 必须 redaction。

### 13. “完成”的定义

Task Done：

- ResultPacket exists。
- VerificationPacket pass。
- ReviewGate no blocker。
- Evidence Ledger complete。

Workflow Done：

- 所有 required tasks done。
- blockers cleared or accepted。
- report generated。
- learnings written。

Commercial Ready：

- release proof。
- security proof。
- license/support proof。
- package lifecycle proof。
- policy/trust proof。


---

<!-- source: docs/kiana_project_os/06-context-memory-repo-intelligence.md -->

## Volume 06: Context、Memory 与 Repo Intelligence

### 1. ContextPack 定义

ContextPack 是一次执行前的可审计上下文包。它不是把所有文件塞进 prompt，而是把当前任务需要的事实、证据、约束和风险压缩成可追溯输入。

ContextPack 包含：

- goal。
- task。
- current state。
- live repo evidence。
- memory summary。
- relevant files。
- relevant commands。
- stack profile。
- risks。
- NOT_BUILDING。
- verification strategy。

### 2. ContextPack 来源

| 来源 | 用途 | 置信度 |
| --- | --- | --- |
| live files | 当前事实 | highest |
| git state | dirty/head/branch | highest |
| command output | 当前验证 | high |
| schema docs | contract | high |
| eventlog | workflow 历史 | high |
| memory | prior learnings | medium |
| old reports | historical | low unless verified |

### 3. Memory 层级

Kiana 采用分层 memory：

#### 3.1 Observation Memory

记录：

- what changed。
- bug/fix。
- project gotcha。
- command behavior。
- test failure pattern。

#### 3.2 Reasoning Memory

记录：

- 为什么选这个方案。
- 替代方案。
- tradeoff。
- 风险接受。
- 用户偏好。

#### 3.3 Git Memory

记录：

- commit-derived facts。
- changed files。
- feature landed。
- tests passed。
- blockers removed。

#### 3.4 Workflow Memory

记录：

- workflow goal。
- blockers。
- current next。
- verification commands。
- report history。

### 4. Memory 写入规则

允许写入：

- 有 evidence 的结论。
- 用户明确偏好。
- 已验证的命令行为。
- 已解决的问题模式。

禁止写入：

- 猜测。
- 未验证实现完成。
- 低置信推断。
- 过时事实。

Memory entry 字段：

- id。
- type。
- text。
- source。
- confidence。
- created_at。
- last_verified_at。
- stale_reason。
- related_files。

### 5. Stale Memory

stale 触发：

- git head changed。
- referenced file missing。
- tests changed。
- schema changed。
- user contradicted memory。
- age threshold exceeded。

行为：

- 降权。
- 标注 stale。
- 要求 live verification。
- 不直接注入执行 prompt。

### 6. Repo Intelligence

Repo Intelligence 的目标是让 Kiana 知道：

- 哪些文件相关。
- 哪些符号相关。
- 哪些测试相关。
- 修改会影响谁。
- 哪些路径高风险。

### 7. Repo Map

输入：

- task goal。
- changed files。
- search query。
- symbol names。
- history。

输出：

- ranked files。
- related tests。
- related docs。
- impacted modules。
- confidence。

Ranking 信号：

- path match。
- symbol match。
- imports。
- call graph。
- test references。
- git co-change。
- recent failures。
- memory references。
- graph confidence。

### 8. GitNexus 吸收点

GitNexus 贡献：

- repo analyze。
- knowledge graph。
- context。
- impact。
- trace。
- detect_changes。
- staleness。
- MCP resource/tool model。

Kiana 吸收：

- `impact` 类能力进入 repo impact。
- `trace` 类能力进入 symbol path。
- `detect_changes` 类能力进入 changed-lines -> affected tasks。
- staleness 检查进入 Context Gate。

不吸收：

- 把 graph 当唯一真相。
- 默认要求用户安装 GitNexus。
- 让 repo graph 越权读取。

### 9. graphify 吸收点

graphify 贡献：

- local graph。
- `graph.json`。
- `GRAPH_REPORT.md`。
- `graph.html`。
- EXTRACTED/INFERRED/AMBIGUOUS confidence。

Kiana 吸收：

- graph edge confidence。
- ambiguous edge human review。
- report as evidence artifact。
- graph output as optional repo intelligence artifact。

### 10. Search 模型

搜索层级：

1. exact file path。
2. symbol search。
3. text search。
4. repo map ranking。
5. graph relation。
6. memory search。

搜索输出必须带来源：

- file。
- line。
- symbol。
- memory id。
- confidence。

### 11. Context Budget

ContextPack 不应无限膨胀。

预算策略：

- goal and constraints always included。
- current task always included。
- relevant files summarized。
- full file only when necessary。
- memory compressed。
- command output tail only。
- large graph summarized。

### 12. Context Gate

Pass 条件：

- live evidence fresh。
- stack detected。
- commands known or missing commands recorded。
- dirty state classified。
- memory stale status known。

Fail/Blocked：

- missing target files。
- repo changed significantly。
- dirty state unsafe。
- context too large。
- critical unknown。

### 13. ContextPack 输出结构

章节：

- Goal。
- Current State。
- Task。
- Scope。
- Live Evidence。
- Relevant Files。
- Relevant Commands。
- Memory。
- Risks。
- NOT_BUILDING。
- Verification。
- Open Questions。

### 14. Memory 与 Evidence 的关系

Memory 可以建议：

- 可能相关文件。
- 以前失败原因。
- 用户偏好。
- 常用命令。

Memory 不能证明：

- 当前代码通过测试。
- 当前 release 可用。
- 当前 bug 已修。
- 当前安全无风险。

证明只能来自 evidence。


---

<!-- source: docs/kiana_project_os/07-bounded-swarm-worker-model.md -->

## Volume 07: Bounded Swarm 与 Worker 模型

### 1. Bounded Swarm 定义

Bounded Swarm 是有界并行执行，不是无限多 agent 群聊。它的目标是提高吞吐，同时控制冲突、成本、风险和验证复杂度。

核心原则：

- 只有 Ready task 才能派发。
- 每个 worker 只拿一个 WorkPacket。
- WorkPacket 有明确文件边界。
- 主 agent 负责集成。
- worker 不直接 merge。
- 并行后必须统一验证。

### 2. Swarm 触发条件

必须满足：

- WorkflowRun 存在。
- 至少 2 个 Ready task。
- tasks dependencies satisfied。
- allowed files 不重叠。
- verification profile 存在。
- user 或 policy 允许并行。

禁止触发：

- dirty state 未分类。
- path overlap。
- shared schema 高风险未隔离。
- release/deploy 任务。
- secrets/policy 任务。

### 3. WorkPacket

WorkPacket 是 worker 的唯一输入。

字段：

- workpacket_id。
- task_id。
- goal。
- context summary。
- allowed files。
- forbidden files。
- commands。
- acceptance criteria。
- rollback。
- review focus。
- budget。
- retry policy。

Worker 不应读取整个项目上下文，除非 WorkPacket 允许。

### 4. Worker 类型

| 类型 | 用途 |
| --- | --- |
| builder | 实现任务 |
| tester | 补测试/验证 |
| reviewer | 只读审查 |
| researcher | 查资料/代码模式 |
| eda_reviewer | 硬件审查 |

P0 可先只支持 builder/reviewer 的本地模拟模型。

### 5. Path Lock

Path Lock 目标：

- 避免 worker 同时写同一文件。
- 避免写共享高风险文件。
- 提前发现冲突。

Lock 类型：

- read。
- write。
- exclusive。
- forbidden。

冲突规则：

- write/write 同路径冲突。
- write/exclusive 父子路径冲突。
- shared schema 文件默认 high risk。
- Cargo.lock/package lock 需要主 agent 集成。

### 6. Dispatch Algorithm

步骤：

1. Load board。
2. Select Ready tasks。
3. Sort by priority。
4. Compute file scope。
5. Build path lock table。
6. Drop conflicting tasks。
7. Limit by max_workers。
8. Create WorkPackets。
9. Write dispatch events。
10. Start workers。

输出：

- dispatched。
- skipped with reason。
- path lock table。

### 7. Worker 执行约束

Worker 必须：

- 读取 WorkPacket。
- 声明理解 scope。
- 只改 allowed files。
- 运行指定命令或说明无法运行。
- 输出 ResultPacket。

Worker 禁止：

- 改 forbidden files。
- 扩大目标。
- commit。
- push。
- merge。
- 安装未批准依赖。
- 修改 policy。

### 8. ResultPacket

ResultPacket 包含：

- changed files。
- commands run。
- pass/fail。
- errors。
- scope deviations。
- notes。
- suggested next。

ResultPacket 是事实包，不是成功声明。

### 9. Integration

主 agent 集成步骤：

1. 收集 ResultPackets。
2. 检查 path lock 是否被违反。
3. 检查 dirty state。
4. 合并非冲突 diff。
5. 对冲突 task 进入 triage。
6. 运行统一 verification。
7. 生成 integration summary。

### 10. Conflict 类型

| 冲突 | 处理 |
| --- | --- |
| same file write | stop integration, triage |
| lock file changed | main agent review |
| schema changed | run broader tests |
| behavior conflict | review synthesis |
| test conflict | fix loop |
| policy conflict | approval gate |

### 11. Worker Budget

Budget 包括：

- max time。
- max tool calls。
- max changed files。
- max retries。
- max output size。

超预算：

- stop worker。
- collect partial result。
- mark blocked or retry。

### 12. Termination

Worker 终止条件：

- completed。
- failed。
- blocked。
- timeout。
- scope violation。
- user stop。

每个终止必须写 event。

### 13. Review After Swarm

并行结果必须经过：

- integration review。
- verification gate。
- review synthesis。
- evidence ledger。

不能：

- worker 自报成功就 Done。
- 单个 worker 测试通过就整体通过。

### 14. Scaling Path

P0：

- 单进程模拟 worker。
- WorkPacket 文件。
- path overlap 检查。

P1：

- typed local worker。
- worktree isolation。
- budget/termination。

P2：

- remote worker pool。
- cloud workspace。
- dashboard。


---

<!-- source: docs/kiana_project_os/08-trust-policy-plugin-mcp-hooks.md -->

## Volume 08: Trust、Policy、Plugin、MCP 与 Hooks

### 1. Trust 模型目标

Kiana 要能用于长期自用和未来企业交付，所以不能把所有能力默认可信。Trust 模型要覆盖：

- shell。
- file edit。
- network。
- plugin。
- MCP。
- hooks。
- worker。
- memory。
- EDA。

### 2. PolicyDecision

每次高价值动作都应产生 PolicyDecision。

字段：

- subject。
- capability。
- target。
- requested_action。
- decision。
- reason。
- requires_approval。
- policy_source。
- audit_event_id。

decision：

- allow。
- deny。
- ask。
- allow_with_constraints。

### 3. Capability 分类

| Capability | 风险 |
| --- | --- |
| read_file | low/medium |
| write_file | medium/high |
| shell_command | medium/high |
| network_access | high |
| install_dependency | high |
| plugin_enable | high |
| mcp_tool_expose | high |
| hook_register | high |
| worker_dispatch | medium/high |
| memory_write | medium |
| push_merge_deploy | critical |
| hardware_order | critical |

### 4. Shell Policy

允许：

- workspace 内只读命令。
- 测试命令。
- 构建命令。
- lint/typecheck。

ask：

- 删除。
- reset/checkout。
- 修改权限。
- 安装依赖。
- 网络下载。
- 启动长期服务。

deny：

- 未授权读取 secrets。
- 上传代码到未知地址。
- 删除用户未确认文件。

### 5. File Edit Policy

允许：

- allowed_paths 内 exact edit。
- 新增 task 相关测试。
- 更新本 workflow artifact。

ask：

- 跨 scope。
- lock file。
- config。
- release scripts。
- policy files。

deny：

- `.env`。
- secrets。
- 用户明确 forbidden。
- worker 修改非 allowed files。

### 6. Network Policy

默认：

- ask。

allowlist：

- 官方 docs。
- package registry if approved。
- configured APIs。

记录：

- host。
- purpose。
- data sensitivity。
- response summary。

### 7. Plugin Trust

Plugin 必须有：

- manifest。
- version。
- source。
- hash。
- capabilities。
- permissions。
- compatibility。
- install receipt。

启用前检查：

- manifest schema。
- hash/signature。
- policy allow。
- disabled list。
- capability exposure。

### 8. MCP Trust

MCP server 字段：

- name。
- command/url。
- source。
- required/optional。
- visibility。
- tools exposed。
- startup status。
- trust level。

规则：

- MCP 工具不默认全暴露。
- required server 失败可 block。
- optional server 失败只降级。
- tool visibility 按 mode/task 限制。

### 9. Hooks Policy

Hook 生命周期：

- SessionStart。
- UserPromptSubmit。
- PreToolUse。
- PostToolUse。
- Stop。
- PreCompact。

安全 hook：

- fail-closed。

普通 hook：

- fail-soft。

记录：

- lifecycle。
- hook id。
- exit status。
- payload summary。
- decision。

### 10. Worker Policy

Worker 权限来自 WorkPacket。

允许：

- allowed files。
- listed commands。
- listed tools。

禁止：

- install plugin。
- change policy。
- push/merge。
- access forbidden files。
- expand scope silently。

### 11. Memory Policy

Memory write 需要：

- source。
- confidence。
- evidence。
- retention。

高风险 memory：

- 用户偏好。
- 安全决策。
- 商业化状态。
- release readiness。

这些必须可追溯。

### 12. EDA Policy

需要 approval：

- BOM 下单。
- PCB 打样。
- 高压/电池建议执行。
- 安规/认证结论。
- 自动替换关键器件。

P0 输出只能是 review 和 plan。

### 13. Approval Gate

Approval request 内容：

- action。
- why needed。
- risk。
- rollback。
- alternatives。
- exact command or operation。

Approval 结果：

- approved。
- rejected。
- approved_once。
- approved_for_workflow。
- needs_more_info。

禁止：

- 把旧 approval 用到新目标。
- 模糊 approval。
- approval 后改变命令不重审。

### 14. Audit Trail

所有 policy decision 写入 eventlog。

Report 中应能回答：

- 哪些高风险操作被请求。
- 哪些被允许。
- 哪些被拒绝。
- 谁批准。
- 基于什么证据。


---

<!-- source: docs/kiana_project_os/09-eda-domain-workflow.md -->

## Volume 09: EDA 领域工作流

### 1. EDA 定位

`/eda` 是 Kiana 的硬件项目操作系统入口。它不是自动画板工具，而是硬件项目推进、资料审查、风险识别和 bring-up 计划工具。

P0 目标：

- 需求捕获。
- 原理图审查清单。
- BOM 风险。
- DFM/Gerber 资料完整性。
- Bring-up 计划。
- Evidence Ledger。

### 2. 输入类型

| 输入 | 说明 |
| --- | --- |
| PRD/需求 | 电源、接口、尺寸、成本、环境 |
| EasyEDA project | 工程文件或导出资料 |
| Schematic PDF/PNG | 原理图审查 |
| BOM | 器件、封装、数量、料号 |
| Gerber | PCB 制造文件 |
| CPL/坐标文件 | 贴片坐标 |
| 约束 | 嘉立创工艺、层数、线宽线距 |

### 3. EDA WorkflowRun

`/eda review` 创建或复用 WorkflowRun。

领域差异：

- rules pack = eda。
- verification profile = eda_review。
- evidence includes hardware artifacts。
- approval includes hardware_order。

### 4. Capture

抽取：

- board purpose。
- power input。
- voltage rails。
- max current。
- MCU/SoC。
- interfaces。
- sensors。
- connectors。
- mechanical constraints。
- cost target。
- assembly target。
- environment。
- safety concerns。

### 5. 原理图审查

检查项：

- 电源输入保护。
- 反接保护。
- TVS/ESD。
- regulator dropout。
- regulator current margin。
- decoupling caps。
- reset circuit。
- boot straps。
- crystal/load caps。
- programming/debug header。
- test points。
- connector pinout。
- IO voltage compatibility。
- analog/digital separation。

输出：

- issue。
- severity。
- evidence。
- recommendation。

### 6. BOM 风险

检查项：

- MPN 是否存在。
- 封装是否匹配。
- 数量是否异常。
- 嘉立创贴片可得性。
- 替代料。
- 生命周期。
- 长交期。
- 成本异常。
- 关键器件单点风险。

注意：

- 库存和价格会变化。
- 必须标注来源时间。
- 不能保证下单可用。

### 7. DFM/Gerber 检查

检查项：

- Gerber 文件是否齐全。
- drill file。
- board outline。
- solder mask。
- silkscreen。
- paste layer。
- line width。
- clearance。
- via size。
- layer count。
- impedance constraints。
- panelization。
- fiducials。
- tooling holes。

P0 可以只做文件完整性和规则清单，不做完整 CAM 解析。

### 8. CPL/坐标检查

检查项：

- designator 匹配 BOM。
- package 匹配。
- rotation。
- side。
- missing components。
- DNP。
- origin。

风险：

- 旋转错误。
- 封装不匹配。
- BOM/CPL designator 不一致。

### 9. Bring-up 计划

Bring-up plan 包含：

- 上电前检查。
- 目检。
- 短路检查。
- 电源空载测试。
- 分阶段上电。
- 电压 rail 测量。
- 电流限制。
- MCU 烧录。
- 时钟检查。
- 通信接口检查。
- 传感器检查。
- 故障记录。

每步字段：

- step。
- tool。
- measurement。
- expected。
- fail action。
- safety note。

### 10. EDA Evidence

Evidence 类型：

- schematic_check。
- bom_check。
- gerber_check。
- cpl_check。
- bringup_step。
- approval_record。

ReviewPacket 字段：

- artifact。
- issue。
- severity。
- confidence。
- recommendation。
- manual_confirmation_required。

### 11. Approval

必须 approval：

- 下单。
- 打样。
- 替换关键器件。
- 高压/电池相关操作。
- 宣称满足认证。

Kiana 默认只输出建议和审查，不执行购买动作。

### 12. EDA 与软件项目统一

`/eda` 不单独建一套 runtime。

复用：

- WorkflowRun。
- Task Card。
- WorkPacket。
- Evidence Ledger。
- VerificationPacket。
- ReviewPacket。
- PolicyDecision。

差异：

- artifact 类型。
- rules pack。
- verification profile。
- approval policy。


---

<!-- source: docs/kiana_project_os/10-p0-p1-p2-delivery-slices.md -->

## Volume 10: P0/P1/P2 交付切片

### 1. 总体原则

P0 不是功能愿望清单，而是可实现、可测试、可验收的切片。

每个切片必须有：

- 命令。
- schema。
- 状态。
- 测试。
- 验收输出。
- failure path。

### 2. P0-M1 WorkflowRun + Data Contracts

#### 2.1 目标

建立项目 OS 的持久运行单元。

#### 2.2 命令

- `/project plan <goal>`
- `/project status`
- `/project resume <workflow_id>`

#### 2.3 Schema

- workflow.yaml。
- state.json。
- eventlog.jsonl。
- task_card.json。

#### 2.4 行为

`/project plan`：

1. 创建 workflow id。
2. 创建 artifact directory。
3. 写 workflow.yaml。
4. 写 state.json。
5. 写 eventlog。
6. 输出 workflow summary。

`/project status`：

1. 读取 state。
2. 校验 last event。
3. 渲染当前节点。

`/project resume`：

1. replay eventlog。
2. rebuild state。
3. check dirty state。
4. choose next node。

#### 2.5 测试

- 创建 workflow。
- 重复 resume 幂等。
- eventlog 可回放。
- state 不一致可修复。
- dirty git 会 block 或 caution。

#### 2.6 验收

- `.kiana/workflows/<id>/` 存在。
- `/project status` 能解释 current node。
- crash 后可恢复。

### 3. P0-M2 Project Board + Task Card

#### 3.1 目标

把长期目标拆成可执行任务板。

#### 3.2 命令

- `/project board`
- `/project next`
- `/project split <task_id>`

#### 3.3 Schema

- TaskCard。
- Kanban state。
- Dependency edge。

#### 3.4 行为

`/project board`：

- 展示列。
- 展示 blockers。
- 展示 evidence 状态。

`/project next`：

- 选择 Ready task。
- 解释原因。

`/project split`：

- 创建 child tasks。
- 维护 dependencies。

#### 3.5 测试

- Ready selection。
- dependency blocking。
- blocked reason required。
- Done requires evidence。

#### 3.6 验收

- 一个目标可拆 WBS。
- board 可恢复。
- next task 可解释。

### 4. P0-M3 Evidence Ledger + Verification Gate

#### 4.1 目标

让 Done/Blocked 有证据。

#### 4.2 命令

- `kiana validate`
- `/audit strict`
- `/report progress`

#### 4.3 Schema

- EvidenceEvent。
- ResultPacket。
- VerificationPacket。
- ReviewPacket。

#### 4.4 行为

`kiana validate`：

- 运行 profile。
- 捕获 pass/fail。
- 写 evidence。

`/audit strict`：

- 扫 fake/stub/TODO/hardcoded。
- 查 tests/release/security/policy。

`/report progress`：

- 从 evidence 生成中文报告。

#### 4.5 测试

- pass command 记录。
- fail command 记录。
- blocked report。
- skipped check requires reason。
- report 不读取 stale memory 当事实。

#### 4.6 验收

- Done 必须有 evidence。
- Blocked 必须有 reason。
- 完成声明可追溯。

### 5. P0-M4 Context Pack + Recovery

#### 5.1 目标

让“继续”真正可用。

#### 5.2 命令

- `/context packet`
- `/context search <query>`
- `/project resume <workflow_id>`

#### 5.3 Schema

- ContextPack。
- Memory source/confidence。
- Project fingerprint。

#### 5.4 行为

`/context packet`：

- 收集 live evidence。
- 收集 relevant memory。
- 标注 stale。
- 生成上下文包。

`/project resume`：

- 检查 state。
- 检查 git。
- 检查 memory。
- 选择下一步。

#### 5.5 测试

- stale memory 降权。
- dirty git 分类。
- crash 恢复。
- fork 生成新 workflow。

#### 5.6 验收

- 用户说“继续”能恢复。
- 不覆盖用户修改。
- next task 有理由。

### 6. P1 切片

#### 6.1 Bounded Swarm

命令：

- `/swarm dispatch --max-workers N`
- `/swarm status`
- `/swarm integrate`

验收：

- path lock 防冲突。
- worker ResultPacket。
- integration gate。

#### 6.2 Repo Intelligence

命令：

- `/context repo-map`
- `/context impact <symbol-or-path>`

验收：

- ranked files。
- impacted tests。
- confidence。

#### 6.3 Memory Ingest

命令：

- `/context ingest --source <path>`
- `/memory propose`

验收：

- source/confidence。
- stale handling。
- no low-confidence auto inject。

#### 6.4 Rules/Skills Injection

命令：

- `/rules status`
- `/skills active`

验收：

- mode/path/task 条件注入。
- conflict explain。

#### 6.5 `/eda review`

命令：

- `/eda review`
- `/eda bom`
- `/eda bringup-plan`

验收：

- BOM risk。
- DFM checklist。
- bring-up plan。
- approval gate。

### 7. P2 切片

#### 7.1 Dashboard

功能：

- workflow board。
- evidence timeline。
- memory graph。
- worker status。

#### 7.2 Cloud Workspace

功能：

- remote session。
- worker pool。
- artifact sync。
- web console。

#### 7.3 Plugin Marketplace Trust

功能：

- install receipt。
- signature/hash。
- policy review。
- disabled plugin enforcement。

#### 7.4 Cost and Usage

功能：

- token estimate。
- worker cost。
- command duration。
- report。

### 8. 里程碑验收顺序

推荐顺序：

1. P0-M1。
2. P0-M2。
3. P0-M3。
4. P0-M4。
5. P1 swarm。
6. P1 repo intelligence。
7. P1 memory。
8. P1 EDA。
9. P2 dashboard。

不要先做 P2 UI。

### 9. Definition of Done

一个 milestone Done 需要：

- schema landed。
- commands visible。
- tests pass。
- failure path exists。
- docs updated。
- evidence generated。
- no stale claim。


---

<!-- source: docs/kiana_project_os/11-data-contracts-and-event-taxonomy.md -->

## Volume 11: 数据契约与事件分类

### 1. 设计目标

Kiana 的核心不是某一个 UI 或某一套 prompt，而是一组稳定的数据契约。只要这些契约稳定，CLI、TUI、Web、云端 worker、企业离线、插件生态都可以共享同一套运行事实。

本卷定义：

- 数据对象边界。
- 事件分类。
- packet 生命周期。
- schema 兼容策略。
- projection 规则。
- 错误和降级语义。

### 2. 契约分层

Kiana 数据契约分为五层：

| 层 | 名称 | 作用 |
| --- | --- | --- |
| L1 | Runtime Contract | session、turn、event、tool call、tool result |
| L2 | Workflow Contract | WorkflowRun、state、DAG、gate、node |
| L3 | Work Contract | TaskCard、WorkPacket、ResultPacket |
| L4 | Evidence Contract | EvidenceEvent、VerificationPacket、ReviewPacket、Report |
| L5 | Trust Contract | PolicyDecision、Approval、PluginReceipt、MCP provenance |

设计原则：

- 下层不依赖上层。
- 上层可以引用下层 id。
- 所有对象必须有 schema_version。
- 所有持久对象必须有 created_at 或 timestamp。
- 所有状态变化必须有 event。

### 3. ID 规范

ID 使用带前缀的稳定字符串：

| 对象 | 前缀 | 示例 |
| --- | --- | --- |
| WorkflowRun | `wf_` | `wf_20260709_001` |
| TaskCard | `task_` | `task_001` |
| WorkPacket | `wp_` | `wp_001` |
| ResultPacket | `rp_` | `rp_001` |
| VerificationPacket | `vp_` | `vp_001` |
| ReviewPacket | `rv_` | `rv_001` |
| EvidenceEvent | `evt_` | `evt_0012` |
| PolicyDecision | `pol_` | `pol_001` |
| Approval | `ap_` | `ap_001` |
| Memory | `mem_` | `mem_001` |

ID 要求：

- 在 workflow 内唯一。
- 可读。
- 不依赖数据库自增。
- 可在文件名中使用。

### 4. Runtime Contract

#### 4.1 Session

Session 表示用户与 Kiana 的长期交互入口。

字段：

- session_id。
- user_id optional。
- workspace_root。
- created_at。
- current_workflow_id。
- active_mode。
- model_profile。
- permission_profile。

Session 不等于 WorkflowRun。一个 Session 可以跨多个 WorkflowRun。

#### 4.2 Turn

Turn 表示一轮模型请求和工具循环。

字段：

- turn_id。
- session_id。
- workflow_id optional。
- input。
- selected_mode。
- route_decision_id。
- started_at。
- completed_at。
- stop_reason。
- tool_calls。
- token_usage optional。

Turn 可被压缩，但关键事件必须进入 eventlog。

#### 4.3 RuntimeEvent

RuntimeEvent 是 UI 和 SDK 投影的基础。

类型：

- assistant_delta。
- tool_call_started。
- tool_call_completed。
- command_output。
- file_changed。
- route_decision。
- policy_decision。
- turn_completed。

RuntimeEvent 可以是 transient，也可以被重要事件转写进 Evidence Ledger。

### 5. Workflow Contract

#### 5.1 WorkflowRun

必需字段：

- workflow_id。
- schema_version。
- goal。
- mode。
- status。
- artifact_dir。
- created_at。
- current_node。
- verification_profile。
- policy_profile。

可选字段：

- parent_workflow_id。
- fork_reason。
- owner。
- tags。
- domain。

#### 5.2 WorkflowState

WorkflowState 是 eventlog 的投影。

字段：

- workflow_id。
- status。
- current_node。
- kanban。
- dirty_state。
- memory_state。
- project_fingerprint。
- last_event_id。

规则：

- 可被重建。
- 不做唯一事实源。
- 写入必须记录 event id。

#### 5.3 WorkflowDAG

DAG 描述节点和依赖。

节点类型：

- capture。
- context。
- research。
- design。
- plan。
- confirm。
- workpacket。
- route。
- execute。
- verify。
- review。
- ship。
- learn。

边类型：

- normal。
- retry。
- fallback。
- approval。
- blocked。
- loop。

### 6. Work Contract

#### 6.1 TaskCard

TaskCard 是最小业务任务。

字段分类：

- identity。
- objective。
- scope。
- dependencies。
- acceptance。
- verification。
- risk。
- evidence。
- status。

TaskCard 不应包含：

- 大段 prompt。
- worker 私有思考。
- 未验证 memory。

#### 6.2 WorkPacket

WorkPacket 是 worker 执行输入。

原则：

- immutable。
- scope-bounded。
- evidence-aware。
- retryable。

WorkPacket 一旦派发，不应原地修改；需要新版本时创建 `wp_001_r2` 或 revision event。

#### 6.3 ResultPacket

ResultPacket 是执行事实。

包含：

- changed_files。
- commands。
- output summaries。
- errors。
- deviations。
- suggested_next。

它不包含成功判断，成功判断属于 VerificationPacket。

### 7. Evidence Contract

#### 7.1 EvidenceEvent

EvidenceEvent 是最小证据单元。

kind：

- command_result。
- test_result。
- build_result。
- diff_summary。
- file_check。
- review_finding。
- approval。
- blocker。
- eda_artifact_check。

status：

- pass。
- fail。
- blocked。
- skipped。
- unknown。

#### 7.2 VerificationPacket

VerificationPacket 汇总一组 evidence。

字段：

- verification_id。
- profile。
- checks。
- final_status。
- required_checks。
- optional_checks。
- skipped_checks。
- evidence_ids。

#### 7.3 ReviewPacket

ReviewPacket 表示审查结果。

字段：

- reviewer_type。
- scope。
- findings。
- blocking_count。
- recommendation。
- evidence_ids。

### 8. Trust Contract

#### 8.1 PolicyDecision

PolicyDecision 对所有敏感动作统一建模。

字段：

- subject。
- capability。
- target。
- action。
- decision。
- reason。
- policy_source。
- requires_approval。

#### 8.2 ApprovalRecord

ApprovalRecord 保存用户批准。

字段：

- approval_id。
- requested_action。
- exact_operation。
- risk_summary。
- approved_by。
- approved_at。
- scope。
- expires。

Approval 必须绑定 exact operation。命令改变后需要重新 approval。

### 9. Event Taxonomy

#### 9.1 Lifecycle Events

- workflow_created。
- workflow_resumed。
- workflow_forked。
- workflow_completed。
- workflow_cancelled。
- workflow_blocked。

#### 9.2 Planning Events

- capture_completed。
- context_pack_built。
- findings_recorded。
- design_candidate_created。
- decision_recorded。
- plan_created。
- plan_revised。
- task_created。
- task_split。

#### 9.3 Execution Events

- workpacket_created。
- worker_assigned。
- worker_started。
- command_started。
- command_completed。
- file_changed。
- resultpacket_created。

#### 9.4 Verification Events

- verification_started。
- check_completed。
- verification_completed。
- acceptance_checked。
- not_building_checked。

#### 9.5 Review Events

- review_scope_created。
- review_started。
- finding_created。
- finding_triaged。
- review_completed。

#### 9.6 Trust Events

- policy_checked。
- approval_requested。
- approval_granted。
- approval_rejected。
- plugin_enabled。
- mcp_server_started。
- hook_registered。

#### 9.7 Learning Events

- learning_recorded。
- memory_proposed。
- memory_accepted。
- memory_rejected。
- memory_marked_stale。

### 10. Packet 生命周期

#### 10.1 TaskCard 生命周期

```text
draft -> ready -> doing -> review -> done
                    |        |
                    v        v
                 blocked   fixing
```

#### 10.2 WorkPacket 生命周期

```text
created -> assigned -> running -> reported -> integrated
                          |          |
                          v          v
                       failed      rejected
```

#### 10.3 VerificationPacket 生命周期

```text
created -> running -> pass
                  -> fail
                  -> blocked
                  -> inconclusive
```

### 11. 兼容策略

Schema 版本规则：

- patch 增加可选字段。
- minor 增加对象类型。
- major 改变语义。

读取旧 schema：

- 尽量 migrate。
- migration 写 event。
- 无法 migrate 则 block。

写入新 schema：

- 必须带 schema_version。
- 必须通过 schema validation。
- 必须保持 unknown fields 不破坏旧读取器。

### 12. Projection 规则

Projection 是从 eventlog/state 生成视图。

视图：

- board。
- report。
- timeline。
- dashboard。
- worker status。
- evidence summary。

规则：

- projection 可重建。
- projection 不应成为唯一事实源。
- projection 错误不应破坏 eventlog。

### 13. 错误语义

错误分类：

- validation_error。
- policy_denied。
- approval_required。
- missing_artifact。
- stale_context。
- dirty_state_conflict。
- worker_failed。
- verification_failed。
- review_blocked。

错误必须携带：

- code。
- message。
- evidence。
- suggested_next。
- recoverable。

### 14. 数据契约验收

P0 验收：

- 能创建 workflow。
- 能写 eventlog。
- 能重建 state。
- 能创建 task card。
- 能创建 evidence event。
- 能校验 schema。
- 能报告旧 schema 不兼容。


---

<!-- source: docs/kiana_project_os/12-cli-tui-product-surface.md -->

## Volume 12: CLI/TUI 产品面

### 1. 产品面目标

CLI/TUI 是 Kiana P0 的主要用户界面。它不是底层逻辑本身，而是 WorkflowRun、Task Board、Evidence Ledger、Policy Gate 的投影。

目标：

- 快速输入。
- 清楚显示当前模式。
- 清楚显示任务状态。
- 清楚显示验证证据。
- 清楚显示阻塞和风险。
- 避免用户被内部协议淹没。

### 2. CLI 原则

#### 2.1 默认输出短

默认只展示：

- 当前做了什么。
- 结果是什么。
- evidence 在哪里。
- 下一步是什么。

详细内容通过 flags：

- `--json`
- `--verbose`
- `--show-evidence`
- `--show-events`
- `--show-risk`

#### 2.2 所有命令可脚本化

每个核心命令应支持 JSON 输出。

用途：

- CI。
- smoke。
- tests。
- app-server。
- dashboard。

#### 2.3 人类摘要和机器输出分离

示例：

```text
/project next
```

人类输出：

```text
Next: task_003 Router decision reasons
Why: dependencies clear, highest unblock value, verification available
Run: /task task_003
```

机器输出：

```json
{
  "selected_task": "task_003",
  "reasons": ["dependencies_clear", "highest_unblock_value"],
  "next_command": "/task task_003"
}
```

### 3. 命令族

#### 3.1 Project commands

- `/project plan <goal>`
- `/project status`
- `/project board`
- `/project next`
- `/project split <task_id>`
- `/project resume <workflow_id>`
- `/project report`

#### 3.2 Context commands

- `/context packet`
- `/context search <query>`
- `/context repo-map`
- `/context impact <path-or-symbol>`
- `/context ingest --source <path>`

#### 3.3 Swarm commands

- `/swarm dispatch --max-workers N`
- `/swarm status`
- `/swarm integrate`
- `/swarm stop <worker_id>`

#### 3.4 Audit/Verify commands

- `/audit strict`
- `kiana validate`
- `kiana checks`
- `kiana review`

#### 3.5 Report commands

- `/report progress`
- `/report blockers`
- `/report handoff`
- `/report teacher`

#### 3.6 EDA commands

- `/eda review`
- `/eda bom`
- `/eda dfm`
- `/eda bringup-plan`

#### 3.7 Trust commands

- `/policy status`
- `/policy explain <event_id>`
- `/plugin status`
- `/mcp status`
- `/hooks status`

### 4. TUI 信息架构

TUI 只展示状态投影：

```text
┌─────────────────────────────────────────────┐
│ Kiana Project: Personal Project OS          │
│ Mode: /project L3     Workflow: wf_001      │
├─────────────────────────────────────────────┤
│ Board                                       │
│ Ready: 3  Doing: 1  Review: 0  Blocked: 2  │
├─────────────────────────────────────────────┤
│ Current Task                                │
│ task_003 Intent Router Decision Rules       │
├─────────────────────────────────────────────┤
│ Evidence                                    │
│ Last check: pass  Command: cargo test ...   │
├─────────────────────────────────────────────┤
│ Risks                                       │
│ policy: ok  dirty: user_dirty caution       │
└─────────────────────────────────────────────┘
```

### 5. 状态条

状态条字段：

- mode。
- workflow_id。
- current task。
- git dirty。
- blockers count。
- verification status。
- policy risk。
- worker count。

状态条不能隐藏 blocker。

### 6. Tool Cards

工具调用显示：

- tool name。
- status。
- target。
- duration。
- risk。
- output summary。

失败显示：

- exit code。
- failure class。
- suggested next。

### 7. Evidence View

Evidence View 展示：

- latest command。
- checks。
- review findings。
- approvals。
- blockers。

每条 evidence 可展开：

- event id。
- timestamp。
- payload summary。
- source。

### 8. Board View

Board View 列：

- Ready。
- Doing。
- Review。
- Blocked。
- Done。

Task Card 显示：

- id。
- title。
- priority。
- risk。
- evidence status。

### 9. Prompt 入口

用户可以输入：

- 自然语言。
- slash command。
- task id。
- workflow id。
- file path。

Prompt 入口要显示 router 决策：

```text
Route: /task -> /project
Reason: long-running goal with multiple milestones
```

### 10. Approval UI

Approval 必须清楚：

- 将执行什么。
- 为什么需要。
- 风险。
- 回滚。
- exact operation。
- scope。

选项：

- approve once。
- approve for workflow。
- reject。
- ask for more info。

### 11. Report UI

报告模式：

- short。
- detailed。
- teacher。
- manager。
- handoff。

报告必须标注：

- evidence-based。
- unknown。
- blocked。
- memory-derived。

### 12. EDA UI

EDA 显示：

- artifact list。
- schematic issues。
- BOM risk。
- DFM issues。
- bring-up steps。
- approval required。

硬件风险必须高亮。

### 13. 错误体验

错误输出包括：

- what failed。
- why it matters。
- evidence。
- next action。
- whether user input is required。

不要只输出 stack trace。

### 14. CLI/TUI 验收

P0 验收：

- `/project board` 可读。
- `/project next` 可解释。
- `/report progress` 可直接汇报。
- `/audit strict` finding 可追溯。
- approval prompt 不模糊。
- JSON 输出可被测试。


---

<!-- source: docs/kiana_project_os/13-plugin-skill-ecosystem-design.md -->

## Volume 13: Plugin 与 Skill 生态设计

### 1. 目标

Kiana 需要插件和 skill 生态，但不能让生态变成不可控风险。插件系统要服务 Project OS，而不是替代核心协议。

目标：

- 可安装。
- 可禁用。
- 可审计。
- 可限制权限。
- 可测试。
- 可归因。

### 2. 能力类型

| 类型 | 用途 |
| --- | --- |
| command | 新增 slash/CLI 命令 |
| skill | 工作流知识和方法 |
| agent | 专家角色或 worker 类型 |
| rule | 条件注入规则 |
| hook | 生命周期触发 |
| mcp | 外部工具连接 |
| template | 文档/packet/report 模板 |
| check | 验证器或审查器 |

### 3. Plugin Manifest

Manifest 字段：

- name。
- version。
- source。
- description。
- capabilities。
- permissions。
- commands。
- skills。
- agents。
- hooks。
- mcp_servers。
- compatibility。
- checks。

Manifest 不足以信任插件，还需要 receipt 和 policy。

### 4. Install Receipt

Receipt 记录：

- plugin id。
- source。
- version。
- hash。
- installed_at。
- installed_by。
- files installed。
- capabilities exposed。
- policy decision。

Receipt 用于：

- audit。
- uninstall。
- disable。
- reproducibility。

### 5. Skill 模型

Skill 包含：

- trigger。
- instructions。
- required inputs。
- outputs。
- failure modes。
- verification。
- examples。

Skill 不应：

- 悄悄扩大权限。
- 修改 policy。
- 直接宣称完成。
- 覆盖项目规则。

### 6. Command 模型

Command 字段：

- name。
- mode。
- input schema。
- output schema。
- required permissions。
- evidence behavior。
- failure behavior。

Command 必须能被 router 理解。

### 7. Agent 模型

Agent 字段：

- agent_type。
- allowed tasks。
- allowed tools。
- default budget。
- required review。
- output packet type。

Agent 不是人格设定，而是能力边界。

### 8. Rule 模型

Rule 类型：

- always。
- mode。
- path。
- glob。
- task type。
- domain。

Rule conflict 处理：

- explicit deny wins。
- project rule wins over plugin rule。
- user override wins if safe。
- conflict must be reported。

### 9. Hook 模型

Hook 生命周期：

- SessionStart。
- UserPromptSubmit。
- PreToolUse。
- PostToolUse。
- Stop。
- PreCompact。

Hook 权限：

- read-only。
- audit。
- blocking。
- mutating。

安全：

- blocking hook fail-closed。
- non-critical hook fail-soft。

### 10. Marketplace Trust

Marketplace 需要：

- source registry。
- signature/hash。
- verified publisher。
- compatibility。
- permission summary。
- install preview。

安装前展示：

- 将安装哪些能力。
- 需要哪些权限。
- 会写哪些文件。
- 是否启用 hooks/MCP。

### 11. Disabled Plugin

禁用后：

- command 不可见。
- skill 不注入。
- agent 不可派发。
- hook 不运行。
- MCP 不启动。

但保留：

- receipt。
- audit trail。
- historical events。

### 12. Skill Eval

Skill 上架前需要：

- static lint。
- trigger test。
- output format test。
- safety review。
- example run。

P0 可以只支持本地 skill eval。

### 13. 与 Reference 的关系

吸收：

- ruflo 的 plugin/workflow/swarm 生态思路。
- everything-claude-code 的 ecosystem taxonomy。
- superpowers 的 workflow skill。
- ECC 的 cross-harness packaging。
- ai-coding-guide 的中文教程式 onboarding。

不吸收：

- 只列 catalog。
- 无权限 manifest。
- 自动启用全部 hooks。
- 未签名 marketplace 默认信任。

### 14. P0 验收

- manifest 可校验。
- install receipt 可生成。
- disabled plugin 生效。
- command/skill/rule 可列出。
- hook/MCP 权限可解释。
- plugin 不可绕过 PolicyDecision。


---

<!-- source: docs/kiana_project_os/14-mcp-tooling-and-provenance.md -->

## Volume 14: MCP、工具与 Provenance

### 1. 目标

MCP 和工具系统让 Kiana 接外部世界，但也带来风险。Kiana 需要把工具可见性、来源、权限、启动状态和失败策略做成一等对象。

### 2. 工具分类

| 类型 | 示例 | 风险 |
| --- | --- | --- |
| local read | read file, search | low/medium |
| local write | edit file, patch | medium/high |
| shell | test/build | medium/high |
| network | docs/API | high |
| browser | web interaction | high |
| MCP tool | external server | variable |
| EDA tool | BOM/Gerber checker | medium/high |

### 3. Tool Registry

字段：

- tool_id。
- name。
- provider。
- type。
- capabilities。
- input schema。
- output schema。
- permission requirement。
- visibility。
- trust level。

Tool Registry 不是单纯列表，它参与 router 和 policy。

### 4. MCP Server Registry

字段：

- server_id。
- name。
- source。
- command/url。
- transport。
- required。
- startup_status。
- trust_level。
- exposed_tools。
- exposed_resources。
- exposed_prompts。

### 5. Provenance

Provenance 记录：

- 谁安装。
- 从哪里来。
- 什么版本。
- hash/signature。
- 哪个 policy 允许。
- 哪些工具暴露给哪些 mode。

没有 provenance 的 MCP 不应默认启用。

### 6. Visibility

可见性层级：

- hidden。
- listed。
- callable。
- callable_with_approval。
- blocked。

Visibility 由以下因素决定：

- mode。
- task type。
- policy。
- plugin status。
- server status。
- user approval。

### 7. Startup Strategy

MCP server 启动状态：

- not_configured。
- configured。
- starting。
- running。
- failed。
- disabled。
- blocked_by_policy。

Required server fail：

- 如果 task 依赖它，block。

Optional server fail：

- 降级并记录。

### 8. Tool Call Pairing

每个 tool call 必须有：

- call id。
- input。
- started event。
- completed/failed event。
- output summary。

孤儿 tool call 必须在 turn 结束时补齐 failure event。

### 9. Output Handling

输出分类：

- small output。
- large output。
- binary artifact。
- sensitive output。
- streaming output。

策略：

- small 直接记录。
- large 记录 head/tail/path。
- binary 记录 artifact path。
- sensitive redaction。
- streaming 写 runtime event，summary 入 ledger。

### 10. Tool Failure

Failure 类型：

- command_failed。
- timeout。
- permission_denied。
- policy_denied。
- server_unavailable。
- invalid_output。
- schema_mismatch。

每个 failure 必须有 recoverability：

- retryable。
- needs_user。
- needs_config。
- blocked。

### 11. MCP Tool Scope

MCP 工具不能默认全部暴露给 worker。

规则：

- worker 只看 WorkPacket allowlist。
- high-risk MCP 需要 approval。
- unknown MCP 默认 hidden。
- plugin-disabled MCP 不可见。

### 12. Resource Model

MCP resources 用于：

- repo context。
- indexed graph。
- external docs。
- project memory。

资源读取也需要 provenance。

### 13. Prompt Model

MCP prompts 是 workflow hint，不是强制规则。

使用时：

- 标注来源。
- 标注版本。
- 标注适用 mode。
- 与 project rules 冲突时 project wins。

### 14. P0 验收

- tool registry 可列出。
- MCP status 可解释。
- required/optional 行为不同。
- tool call/result 成对。
- policy denial 有 event。
- large output 不撑爆 context。


---

<!-- source: docs/kiana_project_os/15-security-audit-and-commercial-readiness.md -->

## Volume 15: 安全审计与商用化准备

### 1. 目标

Kiana 如果要长期自用并走向可商用，必须把安全和交付证明做成流程，而不是发布前临时检查。

本卷定义：

- 安全审计面。
- 商用化 readiness。
- release proof。
- plugin/MCP trust。
- enterprise offline 要求。

### 2. 安全审计面

检查：

- secrets。
- hardcoded credentials。
- unsafe shell。
- untrusted network。
- dependency risk。
- plugin trust。
- MCP exposure。
- hooks fail-open。
- permission bypass。
- path traversal。
- unsafe file edit。

### 3. Strict Audit Finding

字段：

- finding_id。
- category。
- severity。
- confidence。
- evidence。
- affected_files。
- exploitability。
- remediation。
- owner。

Severity：

- critical。
- high。
- medium。
- low。
- note。

### 4. Security Gate

Critical：

- stop。
- block ship。
- require fix。

High：

- fix before ship。
- user risk acceptance only for non-release work。

Medium：

- fix or record risk。

Low：

- follow-up。

### 5. Commercial Readiness

可商用不等于“功能很多”。

需要：

- install works。
- package lifecycle works。
- license known。
- auth/provider config works。
- plugin policy works。
- release smoke passes。
- docs complete。
- support path clear。
- security proof present。

### 6. Release Proof

Release proof 包含：

- build proof。
- package proof。
- install proof。
- smoke proof。
- signature/notarization if applicable。
- source control proof。
- release notes。
- rollback plan。

### 7. Enterprise Offline

要求：

- no mandatory cloud。
- offline license path。
- local plugin install。
- local MCP policy。
- audit export。
- deterministic config。
- redaction。

### 8. Trust Report

Trust report 输出：

- plugins enabled。
- MCP servers。
- hooks。
- network allowlist。
- shell policy。
- high-risk approvals。
- disabled components。

### 9. Security Review 与 Strix 吸收

Strix 贡献：

- validated findings。
- PoC mindset。
- CI security gate。
- remediation report。

Kiana 吸收：

- finding 必须可复现或有 evidence。
- critical finding block。
- security report 可交付。

不吸收：

- P0 自动攻击性测试。
- 未授权目标扫描。

### 10. Fake/Stub Audit

检查：

- `todo!()`。
- `unimplemented!()`。
- “not implemented”。
- placeholder values。
- fake pass。
- hardcoded success。
- tests-only implementation。
- docs claim without code。

Finding 必须区分：

- production gap。
- test helper。
- intentional placeholder。
- docs-only future work。

### 11. Supply Chain

检查：

- dependency source。
- lock files。
- package scripts。
- postinstall。
- plugin source。
- MCP binary/source。

Policy：

- new dependency ask or review。
- unknown plugin block。
- external binary requires provenance。

### 12. Secrets

行为：

- never print secret。
- redact output。
- detect `.env` access。
- block upload。
- evidence uses redacted summary。

### 13. 商用化 Gate

Gate：

- local_blocking = 0。
- external_blocking known。
- release smoke pass。
- docs match behavior。
- trust report pass。
- security critical = 0。

### 14. 验收

P0 验收：

- `/audit strict` 输出 finding。
- security finding 有 severity。
- release proof 区分 local/external blockers。
- plugin/MCP trust 可报告。
- 不把 roadmap 当商用完成。


---

<!-- source: docs/kiana_project_os/16-reporting-learning-and-memory-governance.md -->

## Volume 16: Reporting、Learning 与 Memory Governance

### 1. 目标

Kiana 的报告和学习系统要让项目长期推进，而不是每次重新解释。报告必须基于证据，学习必须可治理。

### 2. 报告类型

| 类型 | 用途 |
| --- | --- |
| progress | 当前进展 |
| blocker | 当前阻塞 |
| handoff | 交接 |
| teacher | 给老师/导师汇报 |
| manager | 项目管理汇报 |
| audit | 审计结果 |
| release | 发布准备 |
| eda | 硬件审查 |

### 3. Progress Report

结构：

- 背景。
- 当前目标。
- 已完成。
- 正在做。
- 阻塞。
- 风险。
- 下一步。
- 需要用户决策。

要求：

- 每个完成项有 evidence。
- 未验证项标未知。
- memory-derived 事实标注可能过期。

### 4. Handoff Report

用于：

- 新 session。
- worker handoff。
- 人类接手。

内容：

- objective。
- current state。
- decisions。
- files touched。
- commands run。
- blockers。
- next task。
- caution。

### 5. Teacher Report

面向不熟代码细节的人。

要求：

- 中文。
- 背景清楚。
- 少代码细节。
- 讲清进度和问题。
- 能直接口述。

### 6. Audit Report

结构：

- scope。
- methodology。
- findings by severity。
- evidence。
- blockers。
- recommended P0 fixes。
- unknowns。

### 7. Learning Loop

Learn 阶段输入：

- eventlog。
- ResultPacket。
- VerificationPacket。
- ReviewPacket。
- user decisions。

输出：

- learnings.md。
- memory proposals。
- follow-up tasks。
- stale memory updates。

### 8. Memory Proposal

Memory 不应自动写入最终库。

Proposal 字段：

- proposed_text。
- type。
- source evidence。
- confidence。
- related files。
- suggested retention。
- risk。

决策：

- accept。
- reject。
- revise。
- defer。

### 9. Memory Governance

规则：

- 低置信不注入。
- stale 降权。
- 用户偏好高优先。
- release/commercial 状态必须 live verify。
- 安全相关 memory 必须 evidence。

### 10. Stale 管控

Stale 原因：

- file changed。
- command changed。
- schema changed。
- time elapsed。
- user contradicted。
- tests contradicted。

处理：

- mark stale。
- request refresh。
- do not use for claim。
- keep as historical context。

### 11. Report Source Labels

报告中事实标签：

- `live`。
- `verified`。
- `memory-derived`。
- `historical`。
- `unknown`。
- `blocked`。

### 12. Follow-up Tasks

Learn 阶段可以创建 follow-up，但必须：

- 标明来源。
- 标明优先级。
- 标明是否 blocking。
- 不自动塞入 Done。

### 13. Metrics

Workflow metrics：

- time to first action。
- tasks completed。
- blockers count。
- verification pass rate。
- retry count。
- worker conflict count。
- stale memory hits。

这些用于改进，不作为虚假 KPI。

### 14. 验收

P0 验收：

- `/report progress` 基于 evidence。
- handoff 可让新 session 继续。
- learnings.md 写入。
- memory proposal 有 source/confidence。
- stale memory 不用于完成声明。


---

<!-- source: docs/kiana_project_os/17-enterprise-offline-and-cloud-workspace.md -->

## Volume 17: Enterprise Offline 与 Cloud Workspace

### 1. 目标

P0 优先 Personal CLI，但架构必须保留企业离线和云端 workspace 的演进路径。否则后续会重构核心协议。

### 2. 三形态共享 Core

共享：

- WorkflowRun。
- EventLog。
- State。
- Packet。
- Evidence Ledger。
- PolicyDecision。
- Plugin Receipt。
- MCP provenance。

不同：

- 执行位置。
- UI。
- worker 管理。
- policy 默认值。
- storage backend。
- sync 模型。

### 3. Personal CLI

特点：

- local-first。
- file-based artifacts。
- local git。
- local commands。
- simple worker。

默认：

- 网络 ask。
- plugin ask。
- push/merge/deploy ask。

### 4. Enterprise Offline

特点：

- no mandatory cloud。
- audited plugin install。
- offline license。
- exportable reports。
- strict policy。
- local MCP。

需求：

- airgap install。
- offline docs。
- signed bundles。
- admin policy。
- redaction。
- audit archive。

### 5. Cloud Workspace

特点：

- remote session。
- worker pool。
- web console。
- artifact sync。
- team visibility。

风险：

- source upload。
- secret exposure。
- worker isolation。
- network policy。

必须有：

- explicit sync consent。
- workspace boundary。
- encrypted storage。
- audit trail。

### 6. Remote Worker

Remote worker 输入仍然是 WorkPacket。

它不能获得：

- full repo unless allowed。
- secrets。
- policy files。
- unrelated workflows。

输出仍然是 ResultPacket。

### 7. Sync Model

同步对象：

- workflow metadata。
- state。
- eventlog。
- packets。
- reports。
- artifacts。

不同步：

- secrets。
- forbidden files。
- unapproved code。

### 8. Conflict Model

Cloud sync 冲突：

- same event id。
- divergent state。
- duplicate task。
- worker stale base。

解决：

- eventlog order。
- vector/sequence metadata。
- manual reconciliation。
- block unsafe merge。

### 9. Enterprise Policy

企业策略：

- allowed commands。
- denied commands。
- plugin allowlist。
- MCP allowlist。
- network allowlist。
- approval roles。
- retention policy。
- redaction policy。

### 10. Audit Export

导出：

- workflow summary。
- eventlog。
- evidence。
- policy decisions。
- approvals。
- plugin receipts。
- MCP status。
- release proof。

### 11. License/Entitlement

Entitlement 不应影响本地数据访问。

可限制：

- cloud worker。
- enterprise features。
- team dashboard。
- hosted sync。

不能限制：

- 用户读取自己的 workflows。
- 导出自己的 evidence。

### 12. Deployment Modes

| 模式 | 描述 |
| --- | --- |
| local | 本地 CLI |
| local-server | 本地 app-server |
| offline-enterprise | 离线企业包 |
| private-cloud | 企业私有云 |
| hosted-cloud | Kiana 托管 |

### 13. P2 前置约束

在做云端前必须完成：

- stable event schema。
- stable packet schema。
- policy model。
- evidence ledger。
- worker boundary。

否则云端只会放大混乱。

### 14. 验收

Enterprise Offline 验收：

- 不联网可启动。
- plugin policy 可用。
- audit export 可生成。
- license 状态可解释。

Cloud Workspace 验收：

- remote worker 只拿 WorkPacket。
- sync 有 audit。
- source upload 有 approval。
- worker conflict 可 block。


---

<!-- source: docs/kiana_project_os/18-reference-to-feature-playbook.md -->

## Volume 18: 参考仓库到功能落地剧本

### 1. 目标

参考仓库不能只停留在“看过”。每个参考必须转成 Kiana 的功能机制、边界和验收。

### 2. 落地模板

每个参考结论必须回答：

- 参考谁。
- 借鉴什么。
- 不借鉴什么。
- 落到哪个 Kiana 模块。
- P0/P1/P2 哪一阶段。
- 验收证据是什么。

### 3. Runtime 类参考

#### 3.1 codex

借鉴：

- protocol。
- exec policy。
- sandbox/trust。
- MCP。

落地：

- PolicyDecision。
- tool registry。
- exec profile。

验收：

- shell 命令能解释 allow/ask/deny。

#### 3.2 claude-code-rev-main

借鉴：

- query loop。
- tool_use/tool_result pairing。
- structured IO。

落地：

- runtime event。
- tool call pairing。
- orphan tool result repair。

#### 3.3 cline/Roo-Code

借鉴：

- patch apply。
- checkpoint。
- safe edit。
- worktree/Kanban。

落地：

- file edit policy。
- checkpoint。
- path lock。

### 4. Project OS 类参考

#### 4.1 planning-with-files

借鉴：

- task_plan。
- findings。
- progress。
- state。

落地：

- `.kiana/workflows/<id>/`。
- persistent planning。

#### 4.2 get-shit-done

借鉴：

- capture/spec/plan/execute/validate/ship/review。

落地：

- WorkflowRun phases。
- Project commands。

#### 4.3 gstack

借鉴：

- plan review。
- eng review。
- QA。
- ship。
- retro。

落地：

- acceptance gates。
- review synthesis。
- delivery summary。

#### 4.4 architect-loop

借鉴：

- orchestrator/strategist/builder。
- fresh context。
- frozen checks。
- typed evidence。

落地：

- bounded swarm。
- WorkPacket。
- verification before integration。

边界：

- Kiana 不采用无 approval gate 的高风险默认。

### 5. Memory 类参考

#### 5.1 memorix

借鉴：

- Observation Memory。
- Reasoning Memory。
- Git Memory。
- orchestration locks。
- dashboard。

落地：

- memory layers。
- memory proposal。
- stale memory。

#### 5.2 claude-memory / MemPalace

借鉴：

- session ingest。
- local memory。
- semantic/full-text search。

落地：

- ContextPack。
- source/confidence。

### 6. Repo Intelligence 类参考

#### 6.1 GitNexus

借鉴：

- analyze。
- context。
- impact。
- trace。
- detect_changes。
- staleness。

落地：

- repo map。
- impact analysis。
- context gate staleness。

#### 6.2 graphify

借鉴：

- graph.json。
- GRAPH_REPORT。
- EXTRACTED/INFERRED/AMBIGUOUS。

落地：

- graph confidence。
- ambiguous review。

#### 6.3 aider

借鉴：

- repo map。
- git-aware edit。

落地：

- edit scope。
- diff safety。

### 7. Multi-Agent 类参考

#### 7.1 autogen

借鉴：

- worker protocol。
- termination。

边界：

- 不采用自由 speaker 群聊。

#### 7.2 MetaGPT

借鉴：

- role/action。

边界：

- 不堆角色名。

#### 7.3 ruflo

借鉴：

- workflow。
- swarm。
- autopilot。
- cost。
- witness。

落地：

- bounded swarm。
- usage ledger。
- evidence witness。

### 8. Ecosystem 类参考

#### 8.1 everything-claude-code

借鉴：

- commands。
- skills。
- agents。
- rules。
- review。
- testing。

落地：

- capability map。
- plugin taxonomy。

#### 8.2 ECC

借鉴：

- cross-harness packaging。
- operator status。
- AgentShield/security。
- MCP policy。

落地：

- plugin trust。
- status payload。
- security readiness。

#### 8.3 ai-coding-guide

借鉴：

- 中文教程式 onboarding。
- 权限/安全/MCP/subagent/worktree 教学。

落地：

- docs/DX。
- `/help` 风格。

### 9. Security 类参考

#### 9.1 Strix

借鉴：

- validated findings。
- PoC evidence。
- security report。

落地：

- security finding evidence。
- critical blocker。

边界：

- P0 不自动做攻击性扫描。

### 10. UI 类参考

参考：

- pi。
- emdash。
- herdr。
- OpenHands。
- ECC HUD。

落地：

- statusline。
- tool cards。
- workflow board。
- evidence timeline。

边界：

- UI 不替代 core protocol。

### 11. EDA 落地

EDA 没有单一参考仓库。

来源：

- 用户明确方向。
- Project OS。
- Review gates。
- Policy model。

落地：

- `/eda review`。
- BOM risk。
- DFM checklist。
- bring-up plan。

### 12. 参考吸收验收

一个参考机制进入 Kiana backlog 前必须有：

- Kiana 模块。
- phase。
- schema/command。
- evidence。
- risk boundary。


---

<!-- source: docs/kiana_project_os/19-testing-evaluation-and-smoke-strategy.md -->

## Volume 19: 测试、评估与 Smoke 策略

### 1. 目标

Kiana 的测试策略必须覆盖软件逻辑，而不只是单元函数。因为 Kiana 的价值在于 workflow、状态恢复、证据和 policy。

### 2. 测试层级

| 层级 | 目标 |
| --- | --- |
| unit | 数据对象和纯逻辑 |
| schema | JSON/YAML contract |
| command | CLI command behavior |
| workflow | end-to-end WorkflowRun |
| recovery | crash/resume/fork |
| policy | allow/deny/ask |
| evidence | Done/Blocked proof |
| integration | multiple modules |
| smoke | release confidence |

### 3. Fixture Repo

需要 fixture：

- clean repo。
- dirty repo。
- repo with failing test。
- repo with fake/stub。
- repo with missing tests。
- repo with plugin。
- repo with MCP。
- EDA sample project。

### 4. Router Tests

测试：

- L0 explanation。
- L1 command。
- L2 bugfix。
- L3 project。
- L4 swarm。
- L5 high-risk。
- EDA domain。
- downgrade。
- conflict priority。

验收：

- RouteDecision has reasons。
- safety wins。

### 5. Workflow Tests

测试：

- create workflow。
- replay eventlog。
- rebuild state。
- plan confirmation。
- task status transition。
- block/unblock。

### 6. Recovery Tests

测试：

- crash after event before state。
- crash after state before event。
- dirty user file。
- stale memory。
- fork workflow。
- interrupted worker。

### 7. Policy Tests

测试：

- shell allow。
- shell ask。
- shell deny。
- file allowed。
- file forbidden。
- network ask。
- plugin blocked。
- MCP hidden。
- approval exact operation。

### 8. Evidence Tests

测试：

- command pass writes evidence。
- command fail writes evidence。
- Done requires evidence。
- Blocked requires reason。
- skipped check requires reason。
- report cites evidence。

### 9. Swarm Tests

测试：

- non-overlap dispatch。
- overlap blocked。
- worker scope violation。
- integration after results。
- unified verification。
- conflict triage。

### 10. Memory Tests

测试：

- source/confidence。
- stale detection。
- live evidence priority。
- memory proposal。
- reject low confidence。

### 11. Repo Intelligence Tests

测试：

- repo map ranks relevant files。
- impact finds tests。
- graph confidence affects ranking。
- missing graph degrades gracefully。

### 12. EDA Tests

测试：

- BOM missing MPN。
- BOM/CPL mismatch。
- Gerber missing drill。
- bring-up plan generated。
- hardware order requires approval。

### 13. Commercial Smoke

Smoke：

- install。
- run first project。
- create workflow。
- validate。
- report。
- plugin status。
- trust status。
- package lifecycle。

### 14. Evaluation

评估维度：

- task completion。
- evidence quality。
- recovery correctness。
- false Done rate。
- policy bypass rate。
- context freshness。
- worker conflict rate。
- report usefulness。

### 15. Regression Matrix

每个 P0 milestone 需要：

- happy path。
- failure path。
- resume path。
- policy path。
- report path。

### 16. Definition of Verified

Verified 需要：

- command run。
- result captured。
- expected checked。
- evidence written。
- failure mode considered。

不能：

- “看起来应该可以”。
- “文档写了”。
- “模型说完成了”。


---

<!-- source: docs/kiana_project_os/20-roadmap-governance-and-scope-control.md -->

## Volume 20: Roadmap Governance 与 Scope Control

### 1. 目标

Kiana 的范围很大，如果没有治理，会变成“什么都想做、什么都做不完”。本卷定义路线、阶段、scope 控制和反膨胀规则。

### 2. 路线原则

- Core protocol before UI。
- Evidence before report。
- State before swarm。
- Policy before plugin marketplace。
- Review before release。
- EDA review before EDA automation。

### 3. P0 范围

P0 只做：

- WorkflowRun。
- Task Board。
- Evidence Ledger。
- Verification Gate。
- Context/Recovery。
- Basic Trust Policy。
- Strict Audit。
- Progress Report。

P0 不做：

- full cloud worker。
- full dashboard。
- auto PCB layout。
- auto marketplace trust。
- team SaaS。

### 4. P1 范围

P1 做：

- bounded swarm。
- repo intelligence。
- memory ingest。
- rules/skills injection。
- MCP provenance。
- `/eda review`。

### 5. P2 范围

P2 做：

- dashboard。
- cloud workspace。
- enterprise offline。
- cost usage。
- plugin marketplace UX。
- EDA advanced automation。

### 6. Scope Creep 信号

危险信号：

- UI 先于协议。
- 自动化先于 approval。
- worker 先于 WorkPacket。
- memory 先于 source/confidence。
- graph 先于 live evidence。
- plugin 先于 policy。

出现时必须回到 P0 milestone。

### 7. Feature Admission

新功能进入 backlog 必须回答：

- 用户场景是什么。
- 属于哪个 mode。
- 数据对象是什么。
- evidence 是什么。
- failure path 是什么。
- policy 风险是什么。
- P0/P1/P2 哪个阶段。

答不出来就不进入 P0。

### 8. Milestone Gate

每个 milestone 开始前：

- scope locked。
- schema identified。
- command identified。
- tests identified。
- failure path identified。

结束时：

- evidence generated。
- docs updated。
- report updated。

### 9. Review Governance

Review 必须区分：

- blocker。
- high。
- medium。
- low。
- note。

所有 blocker 必须：

- fix。
- block。
- user accepted risk。

### 10. Reference Governance

参考仓库进入设计的规则：

- 必须有实际目录或明确来源。
- 必须归因到能力域。
- 必须有不借鉴边界。
- 必须转成 Kiana backlog。

### 11. Documentation Governance

文档分层：

- main spec：产品级决策。
- spec library：深度软件逻辑。
- reference audit：来源归因。
- implementation plan：代码任务。

不要混：

- 文档不能假装代码完成。
- 计划不能假装验证完成。
- reference 不能假装实现完成。

### 12. Versioning

规格版本：

- date-based。
- schema version。
- milestone version。

重大变更：

- 更新 main spec。
- 更新 affected volume。
- 更新 reference audit if source changed。

### 13. Decision Log

记录：

- decision。
- alternatives。
- reason。
- impact。
- date。
- evidence。

必须记录：

- scope cut。
- approval policy。
- P0/P1/P2 migration。
- trust boundary。

### 14. Anti-Patterns

避免：

- 把所有参考都复制。
- 把 UI 做成核心。
- 让 agent 自己判自己完成。
- 无 evidence 的报告。
- 无边界 swarm。
- 无 policy 插件。
- 无恢复状态。
- 旧 memory 当事实。

### 15. Roadmap Review Cadence

建议：

- 每完成一个 P0 milestone，更新规格。
- 每次 strict audit 后更新 blockers。
- 每次 reference 新增后更新 coverage matrix。
- 每次插件/政策变化后更新 trust docs。

### 16. 成功标准

Kiana 成功不是“功能最多”，而是：

- 继续能恢复。
- 做事有证据。
- 并行不乱。
- 风险可控。
- 进度能汇报。
- 长期能演进。


---

<!-- source: docs/kiana_project_os/21-scheduler-dependency-and-priority-model.md -->

## Volume 21: Scheduler、依赖与优先级模型

### 1. 目标

Scheduler 是 Kiana Project OS 的任务选择器。它不负责写代码，也不负责做审查，而是回答：

- 哪些任务 Ready。
- 哪个任务最应该先做。
- 哪些任务可以并行。
- 哪些任务需要等待。
- 哪些任务必须升级到用户决策或 approval。

Scheduler 的输出必须可解释，不能只给一个排序结果。

### 2. 输入

Scheduler 输入：

- WorkflowState。
- Task Cards。
- Dependency Graph。
- current git state。
- path lock table。
- policy state。
- verification availability。
- user priority。
- blocker records。
- memory hints。

输入中，live state 优先于 memory。

### 3. 输出

Scheduler 输出 `ScheduleDecision`：

```json
{
  "schema_version": "kiana.schedule_decision.v1",
  "workflow_id": "wf_001",
  "selected": ["task_003"],
  "parallel_candidates": ["task_004", "task_005"],
  "skipped": [
    {
      "task_id": "task_006",
      "reason": "blocked_by_dependency",
      "dependency": "task_002"
    }
  ],
  "reasons": [
    "task_003 is ready",
    "task_003 has highest unblock value",
    "verification commands are available"
  ],
  "requires_approval": false
}
```

### 4. Dependency Graph

依赖图节点：

- Project Goal。
- Workstream。
- Milestone。
- Task Card。
- External Decision。
- Artifact。
- Verification。
- Approval。

边类型：

- blocks。
- requires。
- produces。
- verifies。
- supersedes。
- related。

### 5. Ready 判定

Task Ready 必须满足：

- status = ready。
- dependencies done。
- required artifacts exist。
- required decisions made。
- required approvals granted or not needed。
- allowed paths known。
- verification commands known or intentionally absent with reason。
- no active conflicting path lock。

任何一个条件不满足，都不能进入 dispatch。

### 6. Priority 信号

优先级由多个信号组合：

| 信号 | 意义 |
| --- | --- |
| user_priority | 用户明确优先级 |
| blocker_unblock_value | 能解除多少阻塞 |
| critical_path | 是否在关键路径上 |
| risk_reduction | 是否降低最大风险 |
| evidence_value | 是否能产出关键证据 |
| implementation_size | 是否是小而完整切片 |
| dependency_fanout | 被多少任务依赖 |
| verification_readiness | 是否可马上验证 |
| stale_risk | 是否拖久会过期 |

Scheduler 不应只按创建顺序选任务。

### 7. Critical Path

Critical Path 用于长期项目：

- release blocker。
- core protocol。
- data contract。
- verification。
- recovery。
- policy。

如果一个任务阻塞多个后续任务，它的优先级提高。

### 8. Parallel Candidate 判定

可以并行的条件：

- 都是 Ready。
- allowed files 不重叠。
- 不共享 high-risk files。
- 不都修改 schema。
- verification 可合并。
- policy 允许。
- worker budget 足够。

不能并行的情况：

- 同一 lock file。
- 同一 config。
- 同一 generated artifact。
- 互相依赖。
- 一个任务改接口，另一个依赖该接口。

### 9. Path Risk

高风险路径：

- lock files。
- release scripts。
- policy files。
- schema files。
- core runtime files。
- auth/secret files。
- plugin manifest。
- MCP config。

这些路径即使不重叠，也可能需要主 agent 串行处理。

### 10. Scheduler Modes

#### 10.1 Conservative

用于：

- P0。
- release。
- policy。
- security。
- EDA。

行为：

- 少并行。
- 强 approval。
- 强 verification。

#### 10.2 Balanced

用于：

- 日常 project。
- 中等任务。

行为：

- 允许非冲突并行。
- 保留主 agent 集成。

#### 10.3 Aggressive

用于：

- 用户明确要求高并发。
- 风险低的文档/测试/参考审计。

行为：

- 更多 worker。
- 但仍有 path lock。

### 11. Starvation 防止

避免某些任务长期不做：

- blocked task 定期复查。
- low priority task 超时提升。
- stale task 标记。
- dependency chain 定期重算。

### 12. Blocked Task 复查

复查触发：

- dependency done。
- user decision arrived。
- artifact created。
- git state changed。
- memory refreshed。

复查后：

- blocked -> ready。
- blocked -> cancelled。
- blocked -> still blocked with updated reason。

### 13. 用户干预

用户可以：

- pin task。
- boost priority。
- defer task。
- cancel task。
- force serial。
- allow swarm。

但不能绕过：

- policy。
- approval。
- forbidden files。
- safety gate。

### 14. Scheduler 验收

P0 验收：

- `/project next` 输出可解释。
- blocked task 不被选中。
- dependency 完成后 task 可变 Ready。
- path 冲突任务不进入 swarm。
- 用户优先级能影响排序。
- high-risk task 自动升级到 approval。


---

<!-- source: docs/kiana_project_os/22-error-failure-and-blocker-taxonomy.md -->

## Volume 22: 错误、失败与 Blocker 分类

### 1. 目标

Kiana 必须把失败当成一等状态，而不是把失败隐藏在聊天文本里。错误分类清楚，恢复路径才清楚。

本卷定义：

- error taxonomy。
- failure packet。
- blocker 类型。
- retry 策略。
- escalation 策略。

### 2. Error 与 Blocker 区别

Error：

- 某个动作失败。
- 可能可恢复。
- 需要记录原因和输出。

Blocker：

- 当前流程不能继续。
- 需要用户、外部状态、设计调整或安全决策。

一个 error 可能变成 blocker，但不是所有 error 都是 blocker。

### 3. Error 分类

| Error | 说明 |
| --- | --- |
| validation_error | 数据不符合 schema |
| command_failed | 命令退出非 0 |
| command_timeout | 命令超时 |
| policy_denied | policy 拒绝 |
| approval_required | 需要用户批准 |
| missing_artifact | 必要文件不存在 |
| stale_context | 上下文过期 |
| dirty_state_conflict | 工作区状态冲突 |
| worker_failed | worker 失败 |
| verification_failed | 验证失败 |
| review_blocked | 审查阻断 |
| mcp_unavailable | MCP 不可用 |
| plugin_disabled | 插件禁用 |
| network_denied | 网络未授权 |
| eda_artifact_invalid | 硬件资料无效 |

### 4. FailurePacket

字段：

- failure_id。
- workflow_id。
- task_id optional。
- node。
- error_code。
- severity。
- recoverable。
- evidence。
- suggested_next。
- retry_count。

### 5. Severity

| Severity | 行为 |
| --- | --- |
| critical | stop/block |
| high | fix before continue |
| medium | fix or record risk |
| low | record follow-up |
| info | report only |

### 6. Recoverability

| 类型 | 行为 |
| --- | --- |
| retryable | 可按 retry policy 重试 |
| user_decision | 需要用户 |
| replan_required | 回到 Plan |
| context_refresh | 刷新 Context |
| policy_approval | Approval Gate |
| external_wait | Block |
| unrecoverable | Cancel/Block |

### 7. Blocker 分类

| Blocker | 说明 |
| --- | --- |
| missing_user_decision | 缺用户方向 |
| missing_approval | 缺高风险批准 |
| missing_artifact | 缺输入文件 |
| external_service | 外部服务不可用 |
| test_failure_unknown | 测试失败原因不明 |
| architecture_conflict | 架构冲突 |
| security_risk | 安全风险 |
| dirty_git | 工作区冲突 |
| scope_unclear | 范围不清 |
| impossible_goal | 目标不可实现 |

### 8. Retry Policy

Retry policy 字段：

- max_attempts。
- backoff。
- retryable_errors。
- stop_on_scope_change。
- stop_on_policy_denied。

默认：

- 同一失败最多重试 2 次。
- policy_denied 不重试。
- approval_required 不重试。
- scope violation 不重试。

### 9. Fix Loop 失败

Fix Loop 自身失败时：

1. 记录 fix attempt。
2. 比较错误是否变化。
3. 如果同一错误重复，停止盲目修。
4. 升级到 Research/Plan/Ask。

禁止：

- 无限改。
- 隐藏失败。
- 用 IFERROR 风格掩盖。

### 10. Command Failure

记录：

- command。
- cwd。
- exit code。
- stdout tail。
- stderr tail。
- duration。
- environment summary。

分类：

- expected red test。
- unexpected fail。
- missing command。
- timeout。
- permission denied。

### 11. Verification Failure

Verification fail 不等于 workflow fail。

路径：

- Fix Loop。
- Add Tests。
- Re-plan。
- Block。

必须保存：

- failed check。
- expected。
- actual。
- related files。

### 12. Review Blocker

Review blocker 处理：

- fix。
- user accepts risk。
- block。

Skip blocker 需要：

- explicit user acceptance。
- reason。
- evidence。

### 13. Dirty Git Blocker

分类：

- user_dirty。
- agent_dirty。
- mixed_dirty。
- unknown_dirty。

行为：

- user_dirty：保护用户修改。
- agent_dirty：恢复或回滚。
- mixed_dirty：隔离或问用户。
- unknown_dirty：停止写入。

### 14. EDA Blocker

类型：

- missing BOM。
- missing Gerber。
- schematic unreadable。
- high voltage risk。
- battery safety risk。
- order approval missing。
- inconsistent designator。

EDA blocker 默认不能自动忽略。

### 15. Block Report

Block Report 包含：

- blocked node。
- reason。
- evidence。
- user needed。
- options。
- recommended default。
- safe next action。

### 16. 验收

P0 验收：

- 每个失败都有 FailurePacket。
- Blocked 有 blocker reason。
- retry 次数有限。
- policy denied 不重试。
- dirty git 不覆盖用户修改。
- report 能展示 blocker。


---

<!-- source: docs/kiana_project_os/23-artifact-storage-retention-and-redaction.md -->

## Volume 23: Artifact 存储、保留与脱敏

### 1. 目标

Kiana 的长期价值依赖 artifact。没有可追溯 artifact，就没有恢复、审计和报告。

Artifact 包括：

- workflow files。
- packets。
- command outputs。
- reports。
- review findings。
- approvals。
- EDA files。
- plugin receipts。

### 2. Artifact 分类

| 类型 | 示例 |
| --- | --- |
| state | workflow.yaml、state.json |
| log | eventlog.jsonl |
| plan | task_plan.md、decision-log.md |
| packet | workpacket/result/verification/review |
| evidence | command output、diff summary |
| report | progress、handoff、audit |
| proof | release/security/license |
| eda | BOM、Gerber summary、bring-up |
| trust | plugin receipt、MCP status |

### 3. 存储位置

默认：

```text
.kiana/workflows/<workflow_id>/
```

跨 workflow 共享：

```text
.kiana/memory/
.kiana/plugins/
.kiana/policy/
.kiana/reports/
```

P0 可以先使用文件系统。

### 4. 命名规则

文件名应：

- 稳定。
- 可排序。
- 可读。
- 包含 id。

示例：

- `workpackets/wp_001.json`
- `resultpackets/rp_001.json`
- `verification/vp_001.json`
- `review/rv_001.json`
- `reports/progress-2026-07-09.md`

### 5. EventLog 保留

EventLog：

- append-only。
- 永久保留。
- 可压缩旧 segment。
- 不应手动编辑。

压缩方式：

- snapshot state。
- archive old events。
- keep hash chain optional。

### 6. Command Output 保留

策略：

- 小输出完整保留。
- 大输出保留 head/tail/summary。
- 完整输出可保存 artifact path。
- 敏感输出必须 redaction。

字段：

- command。
- exit_code。
- stdout_summary。
- stderr_summary。
- full_output_path optional。

### 7. Redaction

需要脱敏：

- API keys。
- tokens。
- passwords。
- private URLs。
- internal hostnames if policy says。
- customer data。

Redaction 规则：

- 原始 secret 不写入 report。
- event 可记录 `redacted: true`。
- 用户可配置保留级别。

### 8. Retention Policy

保留级别：

- keep_forever。
- keep_until_workflow_done。
- keep_until_release。
- keep_summary_only。
- purge_sensitive。

默认：

- eventlog keep_forever。
- reports keep_forever。
- command full output keep_summary_only。
- secrets purge_sensitive。

### 9. Export

导出用途：

- handoff。
- enterprise audit。
- teacher report。
- support bundle。

导出包包含：

- workflow summary。
- state。
- eventlog。
- packets。
- evidence summaries。
- reports。
- trust status。

不包含：

- secrets。
- forbidden files。
- raw user private data unless approved。

### 10. Import

Import 用于恢复或迁移。

要求：

- schema validation。
- trust boundary。
- source record。
- conflict detection。

不允许：

- 导入后直接覆盖 current workflow。
- 不校验外部 artifact。

### 11. Artifact Integrity

可选：

- hash。
- size。
- created_at。
- writer。
- parent event id。

用于：

- tamper detection。
- export validation。
- support proof。

### 12. EDA Artifact

EDA 文件可能很大。

策略：

- 原文件不强制复制。
- 保存 path/hash/summary。
- 对 BOM 保存规范化表。
- 对 Gerber 保存完整性检查结果。

### 13. Artifact Gate

缺少 artifact 时：

- mark missing。
- block if required。
- continue with caution if optional。

### 14. 验收

P0 验收：

- workflow artifacts 可创建。
- eventlog append-only。
- report 可导出。
- command output 可摘要。
- secret redaction 生效。
- missing artifact 能 block。


---

<!-- source: docs/kiana_project_os/24-command-catalog-and-output-contracts.md -->

## Volume 24: 命令目录与输出契约

### 1. 目标

命令是用户触发 Kiana 的主要方式。每个命令必须有稳定语义、输入、输出、错误和 evidence 行为。

### 2. 命令契约字段

每个命令定义：

- name。
- mode。
- purpose。
- input。
- options。
- output human。
- output json。
- artifacts written。
- evidence behavior。
- policy checks。
- errors。

### 3. `/project plan`

输入：

- goal。
- optional source path。
- optional constraints。

输出：

- workflow id。
- goal summary。
- created task count。
- first next task。
- artifact path。

错误：

- unclear goal。
- cannot write artifacts。
- unsafe request。

### 4. `/project status`

输出：

- workflow id。
- status。
- current node。
- dirty state。
- blockers。
- last evidence。

JSON 必须包含：

- `workflow_id`。
- `status`。
- `current_node`。
- `last_event_id`。

### 5. `/project board`

输出：

- columns。
- task cards。
- blockers。
- evidence status。

错误：

- missing workflow。
- corrupt state。

### 6. `/project next`

输出：

- selected task。
- reasons。
- skipped tasks。
- next command。

JSON：

- selected。
- reasons。
- skipped。
- requires_approval。

### 7. `/context packet`

输出：

- context path。
- included sources。
- stale warnings。
- missing facts。

Evidence：

- context_pack_built event。

### 8. `/swarm dispatch`

输入：

- max workers。
- optional task ids。

输出：

- dispatched。
- skipped。
- path locks。
- worker ids。

错误：

- no ready tasks。
- path conflict。
- policy denied。

### 9. `/audit strict`

输出：

- findings。
- severity counts。
- blockers。
- report path。

JSON：

- findings array。
- evidence ids。
- final_status。

### 10. `/report progress`

输出：

- human report。
- source labels。
- evidence references。

错误：

- no workflow。
- no evidence。
- stale only。

### 11. `/eda review`

输入：

- artifact paths。
- constraints。

输出：

- eda review path。
- risk summary。
- missing artifacts。
- approval needs。

### 12. `/policy status`

输出：

- active policy。
- denied capabilities。
- ask capabilities。
- approvals。
- risk flags。

### 13. `/plugin status`

输出：

- installed plugins。
- enabled/disabled。
- capabilities。
- receipt status。

### 14. `/mcp status`

输出：

- servers。
- startup status。
- tools visible。
- required/optional。
- failures。

### 15. Human Output Style

要求：

- 中文优先。
- 简洁。
- 结果先行。
- evidence 明确。
- blocker 明确。

避免：

- 内部对象堆满屏。
- 没有下一步。
- 模糊成功。

### 16. JSON Output Style

要求：

- schema_version。
- command。
- status。
- data。
- errors。
- evidence。

失败也输出 JSON。

### 17. Exit Codes

建议：

- 0 success。
- 1 command failure。
- 2 validation failure。
- 3 policy denied。
- 4 approval required。
- 5 blocked。
- 6 internal error。

### 18. 验收

P0 验收：

- 每个核心命令有 JSON 输出。
- failure 输出可解析。
- human 输出能读。
- evidence path 存在。
- policy denial 不伪装成功。


---

<!-- source: docs/kiana_project_os/25-gate-engine-and-quality-profiles.md -->

## Volume 25: Gate 引擎与 Quality Profile

### 1. 目标

Gate Engine 是 Kiana 的质量控制层。它负责把“能不能继续”变成明确的、可记录的决策。

### 2. GateResult

字段：

- gate_name。
- status。
- reasons。
- evidence。
- next_node。
- required_actions。

status：

- pass。
- caution。
- fail。
- blocked。
- needs_user。

### 3. Gate 类型

- Capture Gate。
- Context Gate。
- Research Gate。
- Design Gate。
- Plan Gate。
- Plan Confirmation Gate。
- WorkPacket Gate。
- Routing Gate。
- Execution Gate。
- Quality Gate。
- Verify Gate。
- Review Gate。
- Ship Gate。
- Learn Gate。

### 4. Quality Profile

Profile 决定检查组合。

| Profile | 检查 |
| --- | --- |
| quick | syntax/basic |
| task | format/lint/test |
| project | format/lint/type/build/test |
| audit_strict | plus security/release/policy |
| release | full release proof |
| eda | artifact/BOM/DFM/bring-up |

### 5. Capture Gate

Pass：

- goal clear。
- success criteria exists。
- constraints known。
- NOT_BUILDING captured。

Fail：

- goal vague。
- unsafe。
- impossible。

### 6. Context Gate

Pass：

- live state known。
- memory freshness known。
- stack/scripts known。
- dirty state classified。

Fail：

- missing files。
- context stale。
- dirty conflict。

### 7. Plan Gate

Pass：

- tasks executable。
- dependencies clear。
- verification defined。
- rollback defined。

Fail：

- vague tasks。
- no tests。
- no scope。

### 8. WorkPacket Gate

Pass：

- allowed files present。
- forbidden files present。
- commands present。
- review focus present。

Fail：

- missing bounds。
- risky without approval。
- dependency invalid。

### 9. Quality Gate

Checks：

- format。
- lint。
- type。
- build。
- security。
- dependency。

Skipped 必须有 reason。

### 10. Verify Gate

Pass：

- targeted tests pass。
- acceptance met。
- NOT_BUILDING not violated。

Fail：

- tests fail。
- acceptance missing。
- cannot verify。

### 11. Review Gate

Pass：

- no blockers。
- high findings fixed or accepted。

Fail：

- blocker。
- security issue。
- insufficient tests。

### 12. Ship Gate

Report only：

- no approval needed。

Approval required：

- push。
- merge。
- deploy。
- release。

### 13. Learn Gate

Pass：

- eventlog updated。
- progress updated。
- learnings written。

Fail：

- cannot write artifacts。
- memory proposal invalid。

### 14. Gate Composition

一个节点可以有多个 gate。

规则：

- fail stops。
- blocked stops。
- caution continues with evidence。
- needs_user asks。

### 15. 验收

P0 验收：

- gate result 可序列化。
- failed gate 有 reasons。
- skipped check 有 reason。
- gate 决策写 eventlog。
- report 能显示 gate 状态。


---

<!-- source: docs/kiana_project_os/26-eda-rules-deep-checklists.md -->

## Volume 26: EDA 深度规则清单

### 1. 目标

本卷把 `/eda` 的规则从高层流程展开成可执行审查清单。P0 不自动画板，但必须能系统化审查。

### 2. 电源输入

检查：

- 输入电压范围是否明确。
- 反接保护。
- 保险丝/限流。
- TVS。
- 输入电容耐压。
- 接口额定电流。
- 接地路径。

风险：

- 输入范围不清。
- 保护缺失。
- 电容耐压不足。

### 3. 电源树

检查：

- 每个 rail 电压。
- 每个 rail 电流预算。
- regulator 余量。
- dropout。
- thermal。
- enable sequencing。
- power good。

输出：

- power_tree_summary。
- rail risk。

### 4. MCU/SoC

检查：

- 供电 rail。
- decoupling。
- reset。
- boot mode。
- programming pins。
- debug header。
- clock。
- unused pins。

### 5. 接口电平

检查：

- UART。
- I2C。
- SPI。
- USB。
- CAN。
- RS485。
- GPIO。

关注：

- 电平兼容。
- 上拉。
- 终端电阻。
- ESD。
- 方向控制。

### 6. 模拟信号

检查：

- ADC input range。
- anti-aliasing。
- reference。
- ground split。
- sensor excitation。
- shielding。

### 7. 时钟

检查：

- crystal frequency。
- load caps。
- routing。
- startup。
- fallback clock。

### 8. 连接器

检查：

- pinout。
- keying。
- current rating。
- mechanical。
- polarity。
- label。

### 9. 测试点

必须考虑：

- power rails。
- reset。
- boot。
- UART。
- SWD/JTAG。
- critical analog。
- ground。

### 10. BOM 字段

必需：

- designator。
- quantity。
- value。
- footprint。
- MPN。
- manufacturer。
- supplier。
- assembly。
- DNP。

### 11. BOM 风险规则

风险：

- no MPN。
- no footprint。
- uncommon package。
- low stock。
- single source。
- lifecycle risk。
- high price。
- substitute missing。

### 12. DFM 规则

检查：

- min trace。
- min spacing。
- min drill。
- annular ring。
- solder mask sliver。
- silkscreen overlap。
- board outline。
- copper to edge。

### 13. Gerber 完整性

文件：

- copper layers。
- soldermask。
- silkscreen。
- paste。
- drill。
- outline。

缺失则 Block。

### 14. Assembly 检查

- BOM/CPL designator match。
- rotation。
- side。
- fiducials。
- package mismatch。
- DNP excluded。

### 15. Bring-up 安全

要求：

- current-limited supply。
- no-load power。
- short check。
- thermal check。
- rail by rail。
- stop on overcurrent。

### 16. EDA Review Severity

| Severity | 示例 |
| --- | --- |
| blocker | Gerber missing drill、BOM/CPL mismatch |
| high | no input protection、rail over current |
| medium | missing test point、substitute missing |
| low | silkscreen clarity |
| note | improvement suggestion |

### 17. 输出结构

EDA review 输出：

- summary。
- artifacts。
- blockers。
- risks。
- checklist results。
- bring-up plan。
- approvals required。

### 18. 验收

P1 验收：

- BOM 风险可识别。
- Gerber 缺失可 block。
- bring-up plan 可生成。
- 下单需要 approval。
- 高压/电池风险高亮。


---

<!-- source: docs/kiana_project_os/27-worker-integration-and-conflict-resolution.md -->

## Volume 27: Worker 集成与冲突解决

### 1. 目标

worker 并行只解决执行吞吐，不解决集成责任。Kiana 必须把集成收回主 agent 或 orchestrator。

### 2. Integration 输入

- WorkPackets。
- ResultPackets。
- path lock table。
- git diff。
- command outputs。
- worker notes。
- verification profile。

### 3. Postflight

每个 worker 完成后：

1. 检查 ResultPacket。
2. 检查 changed files。
3. 检查 forbidden files。
4. 检查 commands。
5. 检查 output schema。
6. 标记 ready_for_integration 或 rejected。

### 4. Touch-set Audit

比较：

- allowed files。
- actual changed files。
- generated files。
- deleted files。

结果：

- clean。
- extra_files。
- forbidden_touched。
- deletion_detected。
- unknown。

### 5. Conflict 分类

| 类型 | 说明 |
| --- | --- |
| path_conflict | 同一文件 |
| semantic_conflict | 行为冲突 |
| schema_conflict | 契约变化 |
| test_conflict | 测试互相影响 |
| dependency_conflict | 依赖版本冲突 |
| policy_conflict | 权限冲突 |
| artifact_conflict | 生成物冲突 |

### 6. Merge Policy

自动集成允许：

- non-overlap。
- tests independent。
- no high-risk files。
- no policy changes。

必须人工/主 agent 审查：

- schema。
- lock files。
- release scripts。
- policy。
- security。
- EDA order files。

### 7. Integration Steps

1. Load packets。
2. Validate packets。
3. Audit touch set。
4. Check conflicts。
5. Apply clean diffs。
6. Run verification。
7. Synthesize review。
8. Update board。
9. Write evidence。

### 8. Rejected Result

Reject 原因：

- scope violation。
- missing ResultPacket。
- command missing。
- forbidden file touched。
- evidence missing。
- output invalid。

Rejected 不等于失败，可以重新派发。

### 9. Fix Wave

Review 后发现问题：

- 创建 fix tasks。
- 可并行则 dispatch。
- 不可并行则 serial fix。

Fix wave 必须继承原 finding。

### 10. Worker Trust

worker 输出不可信直到：

- packet validates。
- touch set clean。
- verification pass。
- review pass。

### 11. Integration Evidence

记录：

- integrated packets。
- rejected packets。
- conflicts。
- commands run。
- final status。

### 12. 验收

P1 验收：

- forbidden file touch 被拒。
- non-overlap 可以集成。
- conflict 会 block。
- integration 后统一 verification。
- worker 自报成功不直接 Done。


---

<!-- source: docs/kiana_project_os/28-memory-formation-and-stale-invalidation.md -->

## Volume 28: Memory 形成与失效

### 1. 目标

Memory 要让 Kiana 长期变聪明，但不能让旧事实污染当前判断。

### 2. Memory Formation Pipeline

步骤：

1. collect candidate。
2. classify type。
3. attach evidence。
4. score confidence。
5. detect sensitivity。
6. propose memory。
7. accept/reject。
8. index。

### 3. Candidate 来源

- successful fix。
- repeated failure。
- user preference。
- project decision。
- command pattern。
- architecture rationale。
- release blocker。
- EDA gotcha。

### 4. Type 分类

- observation。
- reasoning。
- git。
- preference。
- warning。
- command。
- architecture。
- blocker。

### 5. Confidence

| 等级 | 条件 |
| --- | --- |
| high | 有 live evidence + user confirmation |
| medium | 有 evidence 但未长期验证 |
| low | 推断 |
| stale | 可能过期 |

Low 不自动注入。

### 6. Evidence Binding

Memory 必须绑定：

- event id。
- file path。
- command。
- review。
- user decision。

无 evidence 的 memory 只能是 draft。

### 7. Stale Detection

触发：

- referenced file changed。
- command no longer exists。
- tests changed。
- workflow completed long ago。
- user contradicts。
- dependency changed。

### 8. Invalidation

失效行为：

- mark stale。
- remove from auto context。
- keep searchable。
- require refresh before claim。

### 9. Retrieval

检索排序：

- relevance。
- confidence。
- freshness。
- source quality。
- task match。

### 10. Memory Injection

注入规则：

- high confidence allowed。
- medium summarized with caveat。
- low omitted。
- stale only under historical section。

### 11. Memory Conflict

冲突来源：

- memory vs file。
- memory vs test。
- memory vs user。
- memory vs newer memory。

解决：

- live evidence wins。
- newer verified wins。
- user explicit wins if safe。

### 12. Privacy

不要记：

- secrets。
- private credentials。
- sensitive personal data。
- temporary tokens。

### 13. Memory Report

`/memory status` 应显示：

- entries count。
- stale count。
- high confidence count。
- recent proposals。
- rejected count。

### 14. 验收

P1 验收：

- memory proposal 有 evidence。
- stale memory 不进入执行上下文。
- conflict 能解释。
- user preference 可保留。
- release 状态需要 live verify。


---

<!-- source: docs/kiana_project_os/29-report-template-library.md -->

## Volume 29: 中文报告模板库

### 1. 目标

报告模板让 Kiana 输出稳定、可读、可汇报的中文文本。模板不能替代事实来源，所有结论都必须来自 evidence。

### 2. 通用报告原则

- 先结论。
- 再证据。
- 区分完成/未完成/阻塞/未知。
- 不夸大。
- 不把计划当进展。
- 不把 memory 当当前事实。

### 3. Progress Report 模板

```text
当前进展：
- 目标：<goal>
- 已完成：<done with evidence>
- 正在推进：<doing>
- 当前阻塞：<blocked>
- 风险：<risks>
- 下一步：<next>
```

### 4. Teacher Report 模板

```text
老师，这个项目目前的定位是 <thesis>。
现在已经完成的是 <verified progress>。
目前主要问题是 <blockers>。
下一步我准备先做 <next milestone>，因为 <reason>。
```

### 5. Handoff 模板

```text
Handoff:
- Workflow: <id>
- Goal: <goal>
- Current node: <node>
- Last verified: <evidence>
- Dirty state: <dirty>
- Next task: <task>
- Do not touch: <forbidden>
- Blockers: <blockers>
```

### 6. Audit Report 模板

```text
审计范围：<scope>
结论：<pass/fail/blocked>
阻断问题：
1. <finding> - evidence: <evidence>
高风险问题：
1. <finding>
建议 P0 修复：
1. <task>
```

### 7. Commercial Readiness 模板

```text
商用化状态：<status>
本地阻塞：<local_blockers>
外部阻塞：<external_blockers>
已验证证据：<proofs>
缺口：<gaps>
下一步：<next>
```

### 8. Swarm Report 模板

```text
并行执行结果：
- 派发 worker：<count>
- 完成：<completed>
- 拒绝集成：<rejected>
- 冲突：<conflicts>
- 统一验证：<verification>
- 下一步：<next>
```

### 9. EDA Review 模板

```text
EDA 审查结果：
- 资料完整性：<artifacts>
- 原理图风险：<schematic>
- BOM 风险：<bom>
- DFM 风险：<dfm>
- Bring-up 建议：<bringup>
- 需要人工确认：<approvals>
```

### 10. Block Report 模板

```text
当前阻塞：
- 节点：<node>
- 原因：<reason>
- 证据：<evidence>
- 需要你决定：<decision>
- 可选方案：<options>
- 推荐默认：<default>
```

### 11. Release Report 模板

```text
发布检查：
- build：<status>
- package：<status>
- install：<status>
- smoke：<status>
- security：<status>
- license：<status>
- blockers：<blockers>
```

### 12. Memory Report 模板

```text
Memory 状态：
- 可用高置信记忆：<count>
- stale：<count>
- 本轮使用：<used>
- 被拒绝：<rejected>
- 需要刷新：<refresh>
```

### 13. 报告 source label

标签：

- `[live]`
- `[verified]`
- `[memory-derived]`
- `[historical]`
- `[unknown]`
- `[blocked]`

### 14. 禁止话术

禁止：

- “应该完成了”。
- “看起来没问题”。
- “基本可商用”但无 release proof。
- “已修复”但无验证。
- “无风险”但未审计。

### 15. 验收

P0 验收：

- `/report progress` 使用模板。
- audit report 有 severity。
- blocker report 有 options。
- EDA report 有 approval needs。
- source labels 可见。


---

<!-- source: docs/kiana_project_os/30-implementation-governance-without-code.md -->

## Volume 30: 非代码实施治理

### 1. 目标

本规格库现在仍是软件逻辑和功能设计，不直接写代码。为了后续进入实现阶段，需要定义如何从规格转计划，而不丢边界。

### 2. 从规格到计划

转换路径：

```text
Spec Volume
  -> Implementation Slice
  -> Task Card
  -> WorkPacket
  -> Code/Test
  -> Evidence
```

不能从参考仓库直接跳到代码。

### 3. Plan 输入

实现计划必须读取：

- main spec。
- relevant volume。
- reference audit。
- existing schemas。
- current code。
- tests。

### 4. Task 生成规则

每个实现 task 必须：

- 对应一个 volume section。
- 有 exact files。
- 有 schema。
- 有 tests。
- 有 failure path。
- 有 acceptance。

### 5. 不写代码阶段的完成定义

设计阶段完成：

- 功能逻辑清楚。
- 数据契约清楚。
- 命令语义清楚。
- 失败路径清楚。
- 验收清楚。
- 参考归因清楚。

不要求：

- 代码实现。
- 测试通过。
- release proof。

### 6. 防止规格漂移

漂移信号：

- 新需求没有进入 volume。
- 实现计划引用不存在命令。
- 文档说 P0，但 roadmap 说 P2。
- reference audit 与主规格矛盾。

处理：

- 更新 affected volume。
- 更新 main spec if product-level。
- 更新 reference audit if source-level。

### 7. Change Request

规格变更需要：

- change reason。
- affected volumes。
- affected milestones。
- risk。
- migration note。

### 8. Traceability

每个 P0 task 应能追溯：

- main spec section。
- volume section。
- reference source。
- schema。
- test。
- evidence。

### 9. Review Before Implementation

实施前审查：

- scope 是否过大。
- P0/P1/P2 是否正确。
- 是否缺 policy。
- 是否缺 failure path。
- 是否缺 verification。
- 是否依赖未实现模块。

### 10. Implementation Plan 格式

计划应包含：

- Goal。
- Architecture。
- Tech stack。
- Files。
- Tasks。
- Tests。
- Commands。
- Acceptance。
- Rollback。

但这些属于下一阶段，不写入本规格库主体。

### 11. Spec Debt

Spec debt 类型：

- vague command。
- missing schema。
- missing failure path。
- missing policy。
- missing report behavior。
- missing EDA approval。

Spec debt 应进入 backlog。

### 12. Governance Reports

定期输出：

- spec coverage。
- P0 readiness。
- reference coverage。
- stale facts。
- open decisions。

### 13. 人类决策点

必须让用户决定：

- P0 范围变更。
- 高风险自动化。
- 云端同步。
- plugin trust。
- EDA 自动下单。

### 14. 验收

本阶段验收：

- 规格库超过上万行。
- 主规格不臃肿。
- 分册覆盖核心软件逻辑。
- 后续可转 implementation plan。
- 没有过期 GitNexus/36 仓库事实。


---

<!-- source: docs/kiana_project_os/31-project-board-data-model-and-queries.md -->

## Volume 31: Project Board 数据模型与查询语义

### 1. 目标

Project Board 是 Kiana 的项目管理表面。它不是一个简单 todo list，而是 WorkflowRun、Task Card、WorkPacket、Evidence、Blocker、Decision、Gate 的统一投影。

它要回答：

- 当前项目到底有哪些目标。
- 每个目标拆成了哪些工作流、里程碑和任务。
- 哪些任务 Ready，哪些 Blocked，哪些 Done。
- 哪些任务适合并行，哪些必须串行。
- 当前最大的交付风险在哪里。
- 用户说“继续”时应该推进哪一项。

Project Board 本身不执行任务。它只维护可解释的项目状态和可查询视图。

### 2. Board 与 Workflow 的关系

一个 Project Board 可以包含多个 WorkflowRun：

- 一个长期产品项目。
- 一个独立 bugfix。
- 一个 release 审计。
- 一个 EDA 项目审查。
- 一个参考仓库迁移计划。

Board 是长期容器，WorkflowRun 是一次执行闭环。

关系：

```text
ProjectBoard
  -> Goals
  -> Workstreams
  -> Milestones
  -> WorkflowRuns
  -> TaskCards
  -> WorkPackets
  -> EvidenceEvents
  -> Decisions
  -> Blockers
```

Board 必须允许用户跨会话恢复，而不是只能看到当前对话里的任务。

### 3. Board 数据对象

最小对象：

```json
{
  "schema_version": "kiana.project_board.v1",
  "project_id": "proj_kiana",
  "name": "Kiana Personal Project OS",
  "created_at": "2026-07-09T10:00:00+08:00",
  "updated_at": "2026-07-09T18:00:00+08:00",
  "default_mode": "project",
  "status": "active",
  "goals": ["goal_project_os"],
  "active_workflows": ["wf_20260709_design"],
  "views": ["kanban", "wbs", "risk", "release", "eda"],
  "policy_profile": "personal_local",
  "memory_scope": "project"
}
```

字段含义：

| 字段 | 含义 |
| --- | --- |
| project_id | 稳定项目 ID |
| name | 人类可读名称 |
| default_mode | 默认入口模式 |
| status | active/paused/archived/blocked |
| goals | 当前目标集合 |
| active_workflows | 未完成 workflow |
| views | 可用投影视图 |
| policy_profile | 权限默认配置 |
| memory_scope | memory 查询边界 |

### 4. Goal 对象

Goal 是用户意图的长期版本。

```json
{
  "goal_id": "goal_project_os",
  "title": "把 Kiana 做成个人项目操作系统",
  "thesis": "目标管理、记忆、代码理解、并行执行、验证交付在一个闭环里",
  "success_criteria": [
    "支持 /project 长任务恢复",
    "支持 WBS/Kanban 任务拆解",
    "支持 evidence-first done",
    "支持 bounded swarm 并行执行",
    "支持 EDA 审查入口"
  ],
  "not_building": [
    "P0 不做云端团队协作",
    "P0 不做自动 PCB 下单",
    "P0 不做无限制 worker swarm"
  ],
  "status": "active"
}
```

Goal 不应频繁改变。需求变化应形成 ChangeRequest，而不是静默覆盖 Goal。

### 5. Workstream 对象

Workstream 是 PMP/WBS 结构中的中层分组。

示例：

- Runtime。
- Project OS。
- Memory。
- Repo Intelligence。
- Bounded Swarm。
- Policy。
- EDA。
- Commercial Readiness。

Workstream 字段：

```json
{
  "workstream_id": "ws_policy",
  "goal_id": "goal_project_os",
  "title": "Trust / Policy / Approval",
  "owner": "main_agent",
  "status": "active",
  "risk_level": "high",
  "milestones": ["ms_policy_p0"],
  "depends_on": ["ws_runtime"],
  "outputs": ["policy_profile", "approval_log", "tool_permission_matrix"]
}
```

### 6. Milestone 对象

Milestone 必须可验收。

坏 milestone：

- “完善 policy”。
- “做好项目管理”。
- “支持并行”。

好 milestone：

- “实现 P0 Project Board schema、状态投影和 /project status 输出契约”。
- “实现 approval required/approved/rejected 的事件流和恢复行为”。
- “实现 allowed files/path lock/workpacket 合并策略”。

Milestone 字段：

```json
{
  "milestone_id": "ms_project_board_p0",
  "title": "Project Board P0",
  "definition_of_done": [
    "board schema defined",
    "kanban view can be generated",
    "blocked reason visible",
    "next task selection explainable"
  ],
  "verification": [
    "schema fixture validates",
    "sample project renders board summary",
    "stale workflow is marked correctly"
  ],
  "status": "ready"
}
```

### 7. Kanban 状态

Kiana Kanban 不是自由文本列。列必须能映射到执行状态。

标准列：

| Column | 语义 |
| --- | --- |
| Backlog | 已捕获但未拆解 |
| Spec | 正在澄清规格 |
| Ready | 可执行 |
| In Progress | 正在执行 |
| Review | 等待审查或 gate |
| Blocked | 被依赖/决策/权限/失败阻塞 |
| Done | 已有 evidence 的完成 |
| Archived | 已关闭且不再参与调度 |

禁止状态：

- done_without_evidence。
- blocked_without_reason。
- in_progress_without_owner。
- ready_without_verification。

### 8. WBS 视图

WBS 视图强调结构：

```text
Goal
  Workstream
    Milestone
      Task Card
        WorkPacket
```

WBS 用于：

- 长期项目说明。
- PMP 式拆解。
- 与老师/团队汇报。
- 发现范围膨胀。
- 判断哪些任务不是当前 milestone 必需。

WBS 不应用来直接调度 worker。调度要用 ready queue 和 dependency graph。

### 9. Risk 视图

Risk 视图按风险聚合：

- release blocker。
- security blocker。
- data contract drift。
- missing verification。
- stale memory。
- dirty git state。
- conflicting worker edits。
- EDA irreversible action。

Risk 视图必须显示：

- 风险等级。
- 影响范围。
- 证据来源。
- owner。
- next mitigation。
- deadline if any。

### 10. Release 视图

Release 视图用于商用化：

- 当前 release 目标。
- 必须通过的 gates。
- 本地阻塞。
- 外部阻塞。
- 缺失证据。
- 未决 approval。
- 可交付 artifact。
- 不可交付原因。

Release 视图不能只显示百分比。百分比必须能下钻到 blocker ledger。

### 11. EDA 视图

EDA 视图用于硬件项目：

- schematic review。
- PCB layout review。
- BOM availability。
- DFM constraints。
- Gerber readiness。
- power tree。
- interface pinout。
- bring-up plan。
- irreversible action approval。

EDA 视图复用同一套 Evidence/Gate/Approval，不另起系统。

### 12. 查询语义

Project Board 必须支持 query，而不是只能输出一整页摘要。

核心查询：

| Query | 用途 |
| --- | --- |
| next_task | 用户说继续时选择下一项 |
| blocked_tasks | 查阻塞 |
| stale_tasks | 查长期未更新任务 |
| ready_parallel | 查可并行候选 |
| release_blockers | 查商用阻塞 |
| missing_evidence | 查没有证据的完成声明 |
| decision_needed | 查需要用户拍板的灰区 |
| eda_risks | 查硬件风险 |

### 13. next_task 查询

`next_task` 返回一个解释型结果：

```json
{
  "query": "next_task",
  "selected_task": "task_policy_profile",
  "why": [
    "ready",
    "unblocks 3 downstream tasks",
    "verification command available",
    "touches no locked path"
  ],
  "alternatives": [
    {
      "task_id": "task_dashboard_projection",
      "why_not": "depends on board schema"
    }
  ],
  "mode": "balanced"
}
```

Router 使用这个结果决定是否进入 `/task`、`/project` 或 `/swarm`。

### 14. blocked_tasks 查询

Blocked 查询必须区分原因：

- dependency_blocked。
- decision_blocked。
- approval_blocked。
- evidence_blocked。
- verification_blocked。
- external_blocked。
- policy_blocked。
- conflict_blocked。
- stale_context_blocked。

每条 blocked task 必须有解除条件。

### 15. ready_parallel 查询

并行候选输出：

```json
{
  "query": "ready_parallel",
  "candidates": [
    {
      "task_id": "task_docs_audit",
      "allowed_paths": ["docs/reference_audit/**"],
      "conflict_score": 0.1
    },
    {
      "task_id": "task_report_templates",
      "allowed_paths": ["docs/kiana_project_os/29-*"],
      "conflict_score": 0.1
    }
  ],
  "not_parallel": [
    {
      "task_id": "task_schema_runtime",
      "reason": "schema file high risk"
    }
  ]
}
```

并行候选只是建议，实际 dispatch 还要经过 policy 和 path lock。

### 16. Board 更新规则

Board 更新来源：

- 用户输入。
- WorkflowRun event。
- Task status event。
- Evidence event。
- Review result。
- Approval event。
- Git state probe。
- Memory refresh。

禁止直接手改投影结果。投影应从事件和对象生成。

### 17. 投影一致性

Project Board 是投影，不是唯一真相。

真相来源优先级：

1. Append-only EventLog。
2. Workflow state file。
3. Task Card objects。
4. Evidence Ledger。
5. Review/Approval records。
6. Derived board projection。

如果 Board 与 EventLog 冲突，应重建 Board。

### 18. 搜索与过滤

搜索字段：

- title。
- goal。
- owner。
- status。
- risk_level。
- path。
- tag。
- evidence type。
- blocker reason。
- decision owner。

过滤组合：

```text
status=blocked AND risk>=high AND workstream=release
status=ready AND verification=available AND path_conflict=false
mode=eda AND approval_required=true
```

### 19. 排序规则

默认排序：

1. blocked release-critical。
2. ready critical path。
3. user priority。
4. high unblock value。
5. stale risk。
6. small verified slice。
7. creation time。

排序必须解释，不允许黑盒。

### 20. Board 输出格式

人类摘要：

```text
当前项目：Kiana Personal Project OS
状态：active
Ready：4
In Progress：1
Blocked：3
最大风险：policy approval model 未定
下一步：完成 WorkPacket schema deep dive
```

机器输出：

```json
{
  "schema_version": "kiana.board_summary.v1",
  "project_id": "proj_kiana",
  "counts": {
    "ready": 4,
    "in_progress": 1,
    "blocked": 3,
    "done": 18
  },
  "top_risks": ["policy approval model 未定"],
  "next_task": "task_workpacket_schema"
}
```

### 21. 冲突处理

冲突类型：

- 同一 task 多状态。
- evidence 指向不存在 task。
- blocker 已解除但状态仍 blocked。
- task done 但 verification failed。
- workstream archived 但 task active。

处理：

- 优先 EventLog。
- 生成 consistency finding。
- 标记 board projection stale。
- 触发 repair projection。
- 高风险冲突升级给用户。

### 22. 验收

Project Board 设计可验收标准：

- 能表示 Goal/Workstream/Milestone/Task/WorkPacket。
- 能输出 Kanban/WBS/Risk/Release/EDA 五类视图。
- 能回答 next_task、blocked_tasks、ready_parallel。
- 能解释排序和跳过原因。
- 能从 EventLog 重建。
- 能发现 done_without_evidence。
- 能将 `/eda` 映射进同一套项目对象。


---

<!-- source: docs/kiana_project_os/32-workpacket-schema-deep-dive.md -->

## Volume 32: WorkPacket Schema 深潜

### 1. 目标

WorkPacket 是 Kiana 并行执行和任务边界控制的核心对象。它把一个 Task Card 转换成 worker 可以执行、主 agent 可以审查、系统可以恢复的工作包。

WorkPacket 必须解决四个问题：

- 做什么。
- 不做什么。
- 能改哪里。
- 怎么证明完成。

如果 WorkPacket 边界不清，并行只会制造冲突。

### 2. WorkPacket 与 Task Card 的区别

Task Card 面向项目管理：

- 目标。
- 状态。
- 优先级。
- 依赖。
- owner。

WorkPacket 面向执行：

- 输入证据。
- allowed files。
- forbidden files。
- required commands。
- expected outputs。
- rollback。
- review focus。

一个 Task Card 可以拆成多个 WorkPacket。

### 3. 最小结构

```json
{
  "schema_version": "kiana.workpacket.v1",
  "workpacket_id": "wp_policy_profile_001",
  "task_id": "task_policy_profile",
  "workflow_id": "wf_20260709_policy",
  "title": "Define P0 policy profile contract",
  "objective": "Define the behavior and schema for personal_local policy profile",
  "in_scope": [
    "policy profile fields",
    "approval required actions",
    "default deny behavior"
  ],
  "not_building": [
    "enterprise SSO",
    "cloud policy sync",
    "automatic credential rotation"
  ],
  "allowed_paths": [
    "docs/kiana_project_os/08-trust-policy-plugin-mcp-hooks.md",
    "docs/kiana_project_os/33-approval-and-human-decision-protocol.md"
  ],
  "forbidden_paths": [
    "src/**",
    "Cargo.toml"
  ],
  "verification": [
    "rg -n \"approval_required|default deny\" docs/kiana_project_os"
  ],
  "review_focus": [
    "permission boundaries",
    "high-risk operation coverage"
  ],
  "risk_level": "high",
  "requires_approval": false
}
```

### 4. 字段分组

字段分为八组：

| 组 | 字段 |
| --- | --- |
| Identity | workpacket_id/task_id/workflow_id/title |
| Scope | objective/in_scope/not_building |
| Inputs | context_pack/findings/decisions/artifacts |
| File Boundary | allowed_paths/forbidden_paths/path_locks |
| Execution | mode/worker_type/steps/budget |
| Verification | commands/acceptance/evidence_required |
| Review | review_focus/risk_flags/escalation |
| Recovery | retry_policy/rollback/partial_result |

字段缺失时不能进入 dispatch。

### 5. Identity 规则

WorkPacket ID 必须稳定：

- 不依赖当前时间秒级随机值。
- 可从 task slug 派生。
- 重试时保留原 ID，attempt 单独递增。
- fork 时生成新 ID 并记录 parent_workpacket_id。

示例：

```json
{
  "workpacket_id": "wp_board_query_001",
  "attempt": 2,
  "parent_workpacket_id": null
}
```

### 6. Scope 规则

`objective` 必须是一句话。

`in_scope` 必须是可检查条目。

`not_building` 必须防止范围膨胀。

坏例子：

- “完善项目系统”。
- “优化体验”。
- “顺便整理代码”。

好例子：

- “定义 Project Board 的 next_task 查询输出契约”。
- “不修改 runtime 执行逻辑”。
- “不引入新数据库依赖”。

### 7. Inputs

输入对象：

- ContextPack。
- Findings。
- DecisionLog。
- Reference notes。
- Existing files。
- Test outputs。
- User constraints。

WorkPacket 必须记录输入来源，不能只写“参考上下文”。

```json
{
  "inputs": [
    {
      "type": "context_pack",
      "path": ".kiana/runs/wf_001/context_pack.md",
      "freshness": "fresh"
    },
    {
      "type": "decision",
      "id": "dec_parallel_bounded",
      "summary": "bounded swarm only"
    }
  ]
}
```

### 8. File Boundary

allowed_paths 不是建议，是硬边界。

规则：

- worker 只能读取更宽范围，但只能修改 allowed_paths。
- forbidden_paths 优先级高于 allowed_paths。
- high-risk paths 需要主 agent 或 approval。
- generated files 必须声明。
- lock files 默认禁止并行改。

路径匹配必须支持：

- exact file。
- directory prefix。
- glob。
- generated artifact label。

### 9. Path Lock

WorkPacket dispatch 前申请 lock：

```json
{
  "lock_id": "lock_docs_policy",
  "workpacket_id": "wp_policy_profile_001",
  "paths": [
    "docs/kiana_project_os/08-trust-policy-plugin-mcp-hooks.md"
  ],
  "mode": "write",
  "expires_at": "2026-07-09T19:00:00+08:00"
}
```

Lock 过期不能自动释放为可写。系统要先检查 worker 是否仍活跃。

### 10. Execution Mode

执行模式：

| mode | 用途 |
| --- | --- |
| inline | 主 agent 直接执行 |
| worker | 单个 worker 执行 |
| parallel | 多 worker 并行 |
| review_only | 只审查不改动 |
| design_only | 只写规格不写代码 |

当前用户要求属于 design_only。

### 11. Step 语义

WorkPacket 可以包含步骤，但步骤不是自然语言愿望。

每个 step 至少有：

- step_id。
- action。
- expected_output。
- evidence_type。
- failure_behavior。

示例：

```json
{
  "step_id": "s1",
  "action": "extend_project_board_spec",
  "expected_output": "board query contract documented",
  "evidence_type": "file_diff",
  "failure_behavior": "return partial result and mark review_needed"
}
```

### 12. Verification

Verification 分三层：

- structural：文件、schema、标题、引用存在。
- semantic：要求被覆盖，无矛盾。
- behavioral：命令/测试/审查通过。

设计文档阶段主要用 structural 和 semantic。

代码阶段必须补 behavioral。

### 13. Acceptance

Acceptance 必须可判定：

```json
{
  "acceptance": [
    {
      "criterion": "WorkPacket includes allowed_paths and forbidden_paths",
      "evidence": "schema section exists",
      "required": true
    },
    {
      "criterion": "parallel dispatch conflict rules are explicit",
      "evidence": "path lock section exists",
      "required": true
    }
  ]
}
```

不接受：

- “写得足够完整”。
- “感觉可实现”。
- “后续补充”。

### 14. Review Focus

review_focus 告诉审查者看什么：

- scope creep。
- missing evidence。
- unsafe permission。
- unstated dependency。
- schema mismatch。
- conflict risk。
- rollback missing。
- stale input。

Review 不应重新审所有东西，而是聚焦 WorkPacket 风险。

### 15. ResultPacket

WorkPacket 产出 ResultPacket：

```json
{
  "schema_version": "kiana.result_packet.v1",
  "workpacket_id": "wp_board_query_001",
  "status": "done",
  "changed_paths": [
    "docs/kiana_project_os/31-project-board-data-model-and-queries.md"
  ],
  "commands_run": [
    "wc -l docs/kiana_project_os/*.md"
  ],
  "evidence": [
    "ev_file_created",
    "ev_line_count"
  ],
  "scope_deviations": [],
  "notes": [
    "design only; no code changed"
  ]
}
```

### 16. Partial Result

如果 worker 没完成，不能丢失产出。

Partial Result 包含：

- done steps。
- failed step。
- changed files。
- command outputs。
- unresolved questions。
- rollback suggestion。

主 agent 根据 partial result 决定：

- retry。
- split。
- merge partial。
- discard。
- ask user。

### 17. Split 规则

WorkPacket 过大时拆分。

拆分信号：

- allowed_paths 超过 8 个。
- objective 包含多个动词。
- verification 命令不相关。
- review_focus 超过 6 项。
- worker 预计超过预算。
- 同时触及 schema、runtime、UI、docs。

拆分后必须保留 parent-child 关系。

### 18. Merge 规则

小 WorkPacket 可以合并，但只能在以下情况：

- 同一 task。
- 同一 allowed path。
- 同一 verification。
- 风险等级一致。
- 没有并行价值。

合并不能掩盖 blocked reason。

### 19. Retry Policy

Retry 字段：

```json
{
  "retry_policy": {
    "max_attempts": 2,
    "retry_on": ["transient_command_failure", "merge_conflict_resolved"],
    "do_not_retry_on": ["approval_rejected", "scope_invalid"]
  }
}
```

连续失败后必须升级，不允许无限循环。

### 20. Rollback

Rollback 不一定是执行 git reset。

设计阶段 rollback：

- 删除新增文档。
- 回退某个 section。
- 标记 superseded。

代码阶段 rollback：

- revert patch。
- disable feature flag。
- restore config。
- regenerate artifact。

Rollback 计划必须写明会影响哪些文件。

### 21. Worker Handoff

Worker 收到的内容必须足够独立：

- 目标。
- 上下文。
- allowed/forbidden。
- expected evidence。
- failure behavior。
- reporting format。

不要让 worker 去猜整个项目战略。

### 22. 主 Agent 集成

主 agent 负责：

- dispatch。
- path lock。
- result review。
- conflict resolution。
- final verification。
- user reporting。

worker 不负责最终 ship。

### 23. WorkPacket 反例

反例：

```json
{
  "title": "完善 Kiana",
  "allowed_paths": ["**/*"],
  "verification": [],
  "not_building": []
}
```

问题：

- 目标不可验收。
- 允许修改范围无限。
- 没有验证。
- 没有边界。
- 不能安全并行。

### 24. 验收

WorkPacket 设计完成标准：

- schema 字段覆盖 identity/scope/input/file/execution/verification/review/recovery。
- 明确 Task Card 与 WorkPacket 区别。
- 明确 split/merge/retry/rollback。
- 明确 ResultPacket 和 Partial Result。
- 明确 worker 与主 agent 职责边界。
- 能支撑 bounded swarm。


---

<!-- source: docs/kiana_project_os/33-approval-and-human-decision-protocol.md -->

## Volume 33: Approval 与人工决策协议

### 1. 目标

Kiana 需要同时支持自动推进和人工控制。Approval 与 Decision Protocol 的目标不是让用户频繁点确认，而是在方向改变、高风险、不可逆、成本敏感或证据不足时，把人类决策变成可追踪状态。

它要解决：

- 什么时候必须问用户。
- 问什么才是有效问题。
- 用户回答如何进入状态机。
- 拒绝后如何继续。
- 旧决策何时失效。

### 2. Approval 与 Decision 的区别

Decision 是方向选择。

例如：

- P0 先做 Project OS 还是 EDA。
- 并行策略保守还是激进。
- 插件生态先支持本地还是远程 marketplace。

Approval 是风险授权。

例如：

- 是否 push。
- 是否 merge。
- 是否 deploy。
- 是否运行网络命令。
- 是否安装插件。
- 是否自动下单 PCB。

Decision 可以改变计划。Approval 只授权某个操作。

### 3. Decision 对象

```json
{
  "schema_version": "kiana.decision.v1",
  "decision_id": "dec_parallel_strategy",
  "workflow_id": "wf_20260709_design",
  "question": "Kiana P0 的并行策略采用哪一种？",
  "options": [
    {
      "id": "conservative",
      "label": "保守",
      "effect": "默认串行，只对文档和审计并行"
    },
    {
      "id": "bounded",
      "label": "有界并行",
      "effect": "WorkPacket + path lock + 主 agent 集成"
    }
  ],
  "selected": "bounded",
  "decided_by": "user",
  "decided_at": "2026-07-09T14:00:00+08:00",
  "applies_to": ["swarm", "scheduler", "workpacket"],
  "expires_when": ["policy_profile_changes", "major_architecture_change"]
}
```

### 4. Approval 对象

```json
{
  "schema_version": "kiana.approval.v1",
  "approval_id": "ap_push_release_branch",
  "operation": "git_push",
  "risk_level": "high",
  "requested_by": "workflow:wf_release",
  "reason": "需要推送 release candidate 分支",
  "scope": {
    "branch": "release/v0.1",
    "remote": "origin"
  },
  "status": "requested",
  "expires_at": "2026-07-09T23:59:00+08:00"
}
```

Approval 必须绑定具体 operation 和 scope，不能是泛化授权。

### 5. 必问场景

必须问用户：

- 目标不清且无法安全默认。
- 多个路线会改变架构。
- 操作不可逆或难回滚。
- 可能产生费用。
- 访问外部网络或第三方账号。
- 修改安全/权限策略。
- 安装或启用插件。
- 执行 push/merge/deploy。
- EDA 下单、BOM 替代、生产文件导出。
- 用户明确要求先确认。

### 6. 不该问的场景

不应打断用户：

- 低风险文档补全。
- 已有明确默认策略。
- 用户已经给出“继续/ok/按这个来”。
- 查询当前状态。
- 运行只读检查。
- 修复明显拼写/过期事实。
- 在当前规格边界内扩展细节。

过度提问会破坏 Project OS 的推进感。

### 7. 问题质量

好问题：

- 一次只问一个决策。
- 给出默认推荐。
- 说明每个选项的后果。
- 可被记录为 Decision。

坏问题：

- “你想怎么做？”
- “要不要完善？”
- “A/B/C/D 都可以，你选。”
- 一次问多个互相独立的问题。

### 8. Decision Gate

Decision Gate 输入：

- unresolved decisions。
- risk level。
- default availability。
- user preference。
- time sensitivity。

输出：

- ask_user。
- use_default。
- block。
- proceed_with_caution。
- replan。

use_default 必须记录原因。

### 9. Approval Gate

Approval Gate 输入：

- operation。
- policy profile。
- operation risk。
- current scope。
- prior approvals。
- expiration。

输出：

- approved。
- requested。
- rejected。
- expired。
- not_required。

Approval Gate 不负责执行操作，只负责授权判断。

### 10. 用户回答解析

短回答映射：

| 用户输入 | 语义 |
| --- | --- |
| ok | 接受当前推荐 |
| 继续 | 按当前计划推进 |
| a/b/c | 选择对应选项 |
| 都要 | 选择组合方案，但需要拆分范围 |
| 先别 | 暂停当前操作 |
| 不要 | reject |

如果回答含糊但低风险，可以采用推荐默认。高风险必须复问。

### 11. Decision Log

Decision Log 是 append-only。

事件：

- decision_requested。
- decision_defaulted。
- decision_selected。
- decision_superseded。
- decision_expired。

不能直接覆盖旧决策。新决策 supersede 旧决策。

### 12. 失效规则

Decision 失效条件：

- 用户改变目标。
- P0/P1/P2 范围改变。
- 关键参考事实变化。
- 项目结构大改。
- policy profile 改变。
- 时间超过有效期。

失效后不能继续引用为当前依据。

### 13. Approval 有效期

Approval 必须有有效期。

默认：

- shell/network：当前 workflow。
- push/merge/deploy：一次 operation。
- plugin install：一次 install。
- EDA irreversible action：一次 artifact/version。

禁止永久授权高风险操作。

### 14. 灰区处理

灰区是影响方向但不一定高风险的选择。

例子：

- 文档要“主规格厚”还是“分册库厚”。
- 并行 worker 更激进还是更稳。
- `/eda` 先做审查还是自动生成。

灰区处理：

- 给出 2-3 方案。
- 推荐默认。
- 记录选择。
- 如果用户多次选同类偏好，形成 preference memory。

### 15. 拒绝后行为

Approval rejected 后：

- 不执行操作。
- 记录 rejection。
- 查找替代路径。
- 如果无替代，block。
- 如果可降级，继续低风险方案。

例如：

- 不允许 push -> 生成 PR draft 文案。
- 不允许网络 -> 使用本地资料。
- 不允许自动下单 -> 生成人工检查清单。

### 16. Fork 决策

用户可能要求两个方向都做。

处理：

- 创建两个 subgoal。
- 共享上层 thesis。
- 分离 Workstream。
- 分离 acceptance。
- 如果资源冲突，由 Scheduler 排序。

例如：

- 高并发并行干活。
- 电路电子设计方向。

二者共享 Project OS，但落入不同 workstream。

### 17. 决策与 Memory

Memory 可以记住偏好，但不能自动授权。

可记：

- 用户喜欢中文文档。
- 用户偏好“软件逻辑和功能设计，不直接写代码”。
- 用户偏好 Project OS/PMP 方向。

不可记为永久授权：

- deploy。
- push。
- 安装插件。
- 网络访问。
- 花费。
- EDA 下单。

### 18. 决策可解释性

每次自动默认要解释：

```json
{
  "decision_id": "dec_doc_volume_strategy",
  "selected": "multi_volume_spec_library",
  "decision_mode": "defaulted",
  "reason": "user requested large design volume and Chinese docs; multi-volume keeps main spec readable"
}
```

### 19. 人类汇报

在 `/report` 中，Decision 部分应显示：

- 已做关键决策。
- 当前未决决策。
- 需要用户批准的高风险操作。
- 默认决策及原因。
- 已拒绝操作及替代方案。

不要把所有 event dump 给用户。

### 20. 验收

Approval/Decision Protocol 完成标准：

- 区分 Decision 与 Approval。
- 定义必问/不问场景。
- 定义对象 schema。
- 定义失效和有效期。
- 定义拒绝后的替代行为。
- 支持用户短回答。
- 支持两个方向都做时的 fork。


---

<!-- source: docs/kiana_project_os/34-domain-rules-and-conditional-injection.md -->

## Volume 34: Domain Rules 与条件注入

### 1. 目标

Kiana 需要根据任务类型动态加载规则，而不是把所有规则永久塞进上下文。Domain Rules 的目标是让每个任务拿到刚好够用的约束：

- 通用工程规则。
- 语言规则。
- 框架规则。
- 测试规则。
- 安全规则。
- 项目本地规则。
- EDA 领域规则。
- 文档/报告规则。

条件注入的核心是“相关、可解释、可追踪”。

### 2. Rule Pack

Rule Pack 是一组可版本化规则。

```json
{
  "schema_version": "kiana.rule_pack.v1",
  "rule_pack_id": "rules_eda_power_p0",
  "domain": "eda",
  "version": "0.1.0",
  "applies_when": [
    "mode=eda",
    "artifact contains schematic or pcb"
  ],
  "rules": [
    {
      "rule_id": "eda_power_decoupling",
      "severity": "high",
      "statement": "每个 IC 电源脚必须有就近去耦策略或明确豁免原因"
    }
  ]
}
```

Rule Pack 可以来自内置、项目、插件、用户配置或组织策略。

### 3. 规则来源优先级

优先级：

1. 用户当前明确指令。
2. 项目本地规则。
3. 安全/政策规则。
4. 当前 mode/domain 规则。
5. 语言/框架规则。
6. 通用工程规则。
7. memory preference。

安全规则不能被普通项目规则覆盖。

### 4. 注入时机

注入点：

- Capture 后：识别模式。
- Context Intake 后：识别项目栈。
- WorkPacket 生成时：写入执行约束。
- Review Scope 构建时：写入审查重点。
- Verification 前：选择 gate profile。
- Report 前：选择报告模板。

注入不是一次性行为。不同阶段需要不同规则。

### 5. Rule Selector 输入

输入：

- user request。
- mode。
- task_type。
- language/framework。
- touched paths。
- artifacts。
- risk level。
- policy profile。
- project config。
- memory preferences。

输出：

- selected rule packs。
- skipped rule packs。
- conflict notes。
- injected constraints。

### 6. 条件表达式

条件表达式必须简单。

支持：

- mode equals。
- path matches。
- artifact type。
- language detected。
- framework detected。
- risk level。
- operation type。
- policy profile。

不支持任意代码执行条件。

### 7. 冲突处理

冲突类型：

- 两个规则给出相反要求。
- 一个规则要求自动化，另一个 policy 禁止。
- 项目规则与用户当前指令冲突。
- memory preference 与 live evidence 冲突。

处理顺序：

1. 高优先级覆盖低优先级。
2. 不能覆盖时生成 conflict finding。
3. 高风险冲突 ask user。
4. 低风险冲突采用保守默认。

### 8. Common Rules

通用规则：

- 不声明未验证完成。
- 不扩大范围。
- 不覆盖用户改动。
- 不把 memory 当 live truth。
- 不对高风险操作静默执行。
- 所有 Done 必须有 evidence。

这些规则几乎总是注入。

### 9. Coding Rules

代码任务注入：

- 遵循现有风格。
- 小 diff。
- 测试先行或至少先定义验证。
- 避免无关重构。
- 文件边界明确。
- 修改后运行 targeted check。

当前文档任务不注入代码实现规则，只保留设计阶段 traceability。

### 10. Project OS Rules

Project OS 任务注入：

- 每个功能必须有对象、状态、命令、失败路径、验收。
- 长任务必须可恢复。
- 用户说继续必须有 next task 选择逻辑。
- Board 是投影，不是真相源。
- Evidence-first。
- L0-L5 必须可升级降级。

### 11. Swarm Rules

并行任务注入：

- WorkPacket 必须完整。
- allowed_paths 必须有限。
- forbidden_paths 必须明确。
- path lock 必须申请。
- schema/runtime/high-risk 文件默认串行。
- 主 agent 负责集成。

没有 WorkPacket 不允许 swarm。

### 12. EDA Rules

EDA 注入：

- schematic/ERC。
- power tree。
- connector pinout。
- BOM availability。
- DFM。
- Gerber。
- bring-up。
- irreversible approval。

EDA 自动化默认 review/planning，不默认下单。

### 13. Security Rules

安全规则：

- secrets 不写入日志。
- 网络访问可追踪。
- plugin/MCP/hook 需要 trust boundary。
- shell 操作按 risk 分类。
- deploy/push/merge 需要 approval。
- 第三方工具输出不能无验证信任。

安全规则优先级高于效率。

### 14. Report Rules

报告任务注入：

- 中文优先。
- 先结论后证据。
- 面向听众调整粒度。
- 明确当前进展、问题、下一步。
- 不把技术细节堆给非技术听众。
- 引用 live evidence。

### 15. Rule Pack 元数据

每个 Rule Pack 需要：

- id。
- version。
- source。
- owner。
- applies_when。
- conflicts_with。
- priority。
- last_updated。

没有来源的规则不能用于 high-risk gate。

### 16. Project Local Rules

项目本地规则位置建议：

- `.kiana/rules/common.md`
- `.kiana/rules/project.md`
- `.kiana/rules/eda.md`
- `.kiana/rules/security.md`
- `.kiana/rules/report.md`

这些是设计建议，不是当前要求实现的文件。

### 17. Plugin Rules

插件可以贡献 rule pack，但默认不可信。

启用条件：

- plugin trusted。
- rule pack manifest valid。
- no policy conflict。
- source visible。
- user or project allowed。

插件规则不能静默扩大权限。

### 18. Rule Injection Log

每次注入记录：

```json
{
  "event_type": "rules.injected",
  "workflow_id": "wf_001",
  "workpacket_id": "wp_001",
  "selected": ["rules_common", "rules_project_os", "rules_security"],
  "skipped": [
    {
      "rule_pack": "rules_eda",
      "reason": "mode is project, not eda"
    }
  ]
}
```

### 19. Rule Drift

Rule drift 信号：

- 文档说 P0 需要规则，router 没注入。
- review 使用了过期 rule。
- plugin 更新改变规则。
- 项目规则删除但 memory 还引用。

处理：

- 标记 stale。
- 重算 rule selection。
- 记录 drift event。
- 需要时重新审查受影响任务。

### 20. 验收

Domain Rules 设计完成标准：

- 定义 Rule Pack schema。
- 定义来源优先级。
- 定义注入时机。
- 定义冲突处理。
- 覆盖 Project OS、Swarm、EDA、Security、Report。
- 插件规则有 trust 边界。
- 注入过程可记录和回放。


---

<!-- source: docs/kiana_project_os/35-dashboard-projection-model.md -->

## Volume 35: Dashboard 投影模型

### 1. 目标

Dashboard 是 Kiana 给用户看的项目驾驶舱。它不替代 CLI/TUI，也不替代文档，而是把 Project Board、Evidence Ledger、Gate Result、Blocker Ledger、Decision Log 投影成可扫描状态。

Dashboard 要回答：

- 现在项目在哪里。
- 下一步做什么。
- 哪些东西卡住。
- 哪些完成有证据。
- 哪些风险需要人类决策。
- 商用化/发布还差什么。

### 2. Dashboard 不是事实源

Dashboard 是 projection。

事实源：

- EventLog。
- WorkflowState。
- TaskCard。
- EvidenceLedger。
- ApprovalLog。
- BlockerLedger。
- Git/command live probes。

Dashboard 可以缓存，但必须可重建。

### 3. Projection 对象

```json
{
  "schema_version": "kiana.dashboard_projection.v1",
  "project_id": "proj_kiana",
  "generated_at": "2026-07-09T20:00:00+08:00",
  "freshness": "fresh",
  "summary": {
    "status": "active",
    "mode": "project",
    "health": "yellow",
    "next_action": "complete WorkPacket schema deep dive"
  },
  "cards": [],
  "alerts": [],
  "metrics": {}
}
```

freshness 可为：

- fresh。
- stale。
- partial。
- inconsistent。

### 4. 首页信息架构

首页分区：

- Project Header。
- Next Action。
- Health Summary。
- Workstream Progress。
- Blockers。
- Decisions Needed。
- Evidence Coverage。
- Recent Activity。
- Release Readiness。

不要把所有 event 展开到首页。

### 5. Project Header

显示：

- 项目名。
- 当前目标。
- 当前 mode。
- 当前 workflow。
- 最近更新时间。
- 当前 branch/worktree。
- policy profile。

如果 git state dirty，要显示但不恐吓用户。

### 6. Next Action Card

Next Action 来源于 Scheduler。

字段：

- selected task。
- why now。
- required approval。
- estimated mode。
- can_parallelize。
- verification available。

示例：

```json
{
  "card_type": "next_action",
  "task_id": "task_dashboard_projection",
  "title": "Define Dashboard projection model",
  "why": [
    "needed by /project status",
    "unblocks reporting surface",
    "design-only low conflict"
  ],
  "entry_command": "/task task_dashboard_projection"
}
```

### 7. Health Summary

Health 不是完成百分比。

状态：

- green：目标清楚，next action ready，无阻塞 gate。
- yellow：可推进，但有风险或缺证据。
- red：关键阻塞或安全/发布 gate failed。
- gray：上下文不足或 projection stale。

Health 必须给原因。

### 8. Progress Metrics

可显示指标：

- task counts by status。
- milestone completion。
- evidence coverage。
- verification pass rate。
- blocker count。
- decision count。
- stale task count。
- release gate pass count。

禁止单独使用“90% 完成”而没有分解。

### 9. Evidence Coverage

Evidence Coverage 衡量完成声明可信度。

```json
{
  "done_tasks": 18,
  "done_with_evidence": 18,
  "done_without_evidence": 0,
  "failed_verification": 1,
  "missing_review": 2
}
```

如果 done_without_evidence > 0，Health 至少 yellow。

### 10. Blocker Panel

Blocker 显示：

- blocker id。
- type。
- severity。
- owner。
- affected tasks。
- unblock condition。
- age。

Blocker age 用于防止长期卡住没人处理。

### 11. Decision Panel

Decision Panel 显示：

- 需要用户决定。
- 已默认的决策。
- 已拒绝的 approval。
- 即将过期的 approval。
- 被 supersede 的旧决策。

高风险 approval 要突出显示。

### 12. Workstream Progress

Workstream 不是百分比条即可。

每个 workstream 显示：

- status。
- current milestone。
- ready tasks。
- blocked tasks。
- top risk。
- latest evidence。

这样用户能判断为什么一个方向慢。

### 13. Release Readiness

Release Readiness 维度：

- local blockers。
- external blockers。
- security blockers。
- test blockers。
- docs blockers。
- packaging blockers。
- policy blockers。
- user approval blockers。

每个 blocker 必须能跳到 evidence。

### 14. EDA Dashboard

EDA 模式显示：

- schematic status。
- PCB status。
- BOM status。
- DFM status。
- Gerber export status。
- bring-up checklist。
- irreversible approval。

硬件风险不应混在普通代码 blocker 里。

### 15. Activity Feed

Activity Feed 显示最近事件：

- workflow created。
- task moved。
- evidence added。
- gate passed/failed。
- decision made。
- approval requested。
- blocker opened/closed。

Feed 应支持折叠，避免淹没用户。

### 16. Projection Refresh

刷新触发：

- workflow event appended。
- task status changed。
- evidence added。
- gate result changed。
- git state changed。
- user asks status/report。
- resume。

刷新失败时显示 stale，而不是继续展示旧状态。

### 17. 数据一致性

Dashboard 必须检测：

- task count mismatch。
- missing evidence target。
- blocker references missing task。
- decision references old workflow。
- stale projection timestamp。

发现不一致：

- 标记 inconsistent。
- 输出 repair suggestion。
- 高风险时阻止 ship。

### 18. 用户操作

Dashboard 可提供操作入口：

- continue。
- open next task。
- view blockers。
- approve/reject。
- generate report。
- refresh context。
- run audit。

操作仍走 Router/Policy，不绕过 gate。

### 19. CLI/TUI 输出关系

CLI 输出应和 Dashboard 一致。

`/project status` 是 Dashboard 的文本投影。

`/report` 是 Dashboard + Evidence 的叙述投影。

`/audit` 是 Dashboard + Gate 的风险投影。

### 20. 验收

Dashboard 设计完成标准：

- 明确 projection 非事实源。
- 定义首页分区。
- 定义 health/freshness。
- 定义 next action、blocker、decision、evidence、release readiness。
- 支持 EDA 投影。
- 支持 stale/inconsistent 检测。
- 所有操作回到 Router/Policy。


---

<!-- source: docs/kiana_project_os/36-release-proof-and-commercial-blocker-ledger.md -->

## Volume 36: Release Proof 与商用阻塞账本

### 1. 目标

Kiana 如果要走向长期自用、公开发布或商用交付，必须有一套 release proof 与 blocker ledger。它不能只靠“测试过了”“感觉能用”“文档写了”来判断。

本卷定义：

- 什么是 release proof。
- 什么是 commercial blocker。
- blocker 如何分类、归属、关闭。
- release readiness 如何从证据生成。
- 文档设计阶段如何为后续实现留出验收面。

### 2. Release Proof

Release Proof 是一组可复查证据。

包括：

- build proof。
- test proof。
- smoke proof。
- package proof。
- docs proof。
- security proof。
- policy proof。
- migration proof。
- user workflow proof。

每个 proof 都要有来源、时间、命令或文件、结果。

### 3. Proof 对象

```json
{
  "schema_version": "kiana.release_proof.v1",
  "proof_id": "proof_schema_contract_smoke",
  "release_id": "rel_0_1_candidate",
  "proof_type": "smoke",
  "source": "command",
  "command": "scripts/schema-contract-smoke.sh",
  "status": "pass",
  "captured_at": "2026-07-09T21:00:00+08:00",
  "artifacts": [
    ".kiana/release/proofs/schema-contract-smoke.log"
  ],
  "notes": "P0 schema fixtures validated"
}
```

设计阶段可以定义 proof 类型，但不能伪造 pass。

### 4. Commercial Blocker

Commercial Blocker 是阻止项目达到“可长期自用/可发布/可交付”的问题。

不是所有 bug 都是 commercial blocker。

判定条件：

- 阻止核心用户流程。
- 阻止安装/启动/配置。
- 阻止数据安全。
- 阻止恢复/继续。
- 阻止交付验证。
- 阻止合规或权限边界。
- 阻止用户理解当前状态。

### 5. Blocker 对象

```json
{
  "schema_version": "kiana.commercial_blocker.v1",
  "blocker_id": "cb_resume_state_missing",
  "title": "Resume state lacks stale context detection",
  "severity": "high",
  "class": "recovery",
  "owner": "runtime",
  "owner_status": "local_blocking",
  "affected_workflows": ["/project", "/task"],
  "evidence": ["ev_resume_audit_001"],
  "acceptance_artifacts": [
    "stale context fixture",
    "resume report output"
  ],
  "verification_commands": [
    "run resume-state smoke"
  ],
  "status": "open"
}
```

### 6. Blocker 分类

分类：

| class | 含义 |
| --- | --- |
| install | 安装/启动/配置 |
| runtime | 核心执行 |
| recovery | crash/resume/continue |
| context | memory/repo/context |
| policy | 权限/approval/trust |
| workflow | task/project/swarm |
| verification | test/gate/evidence |
| security | secrets/network/plugin |
| docs | 用户无法理解或操作 |
| packaging | 发布包/平台兼容 |
| external | 外部依赖或账号 |
| eda | 硬件流程风险 |

分类用于 owner 和 release view。

### 7. Owner Status

owner_status：

- local_blocking：本仓库能解决。
- external_blocking：依赖外部系统/账号/网络/硬件。
- user_blocking：需要用户决策。
- upstream_blocking：依赖参考或上游变更。
- deferred：明确不是当前 release。

不能把 local_blocking 伪装成 deferred。

### 8. Severity

Severity：

- critical：核心流程不可用或安全风险严重。
- high：核心流程不完整，不能发布。
- medium：影响可信度或部分用户流程。
- low：不阻止发布，但应记录。

critical/high 必须在 release 前关闭或明确降级并记录风险。

### 9. Release Readiness

Readiness 不直接用百分比。

输出：

```json
{
  "schema_version": "kiana.release_readiness.v1",
  "release_id": "rel_0_1_candidate",
  "status": "not_ready",
  "critical_blockers": 0,
  "high_blockers": 3,
  "local_blocking": 2,
  "external_blocking": 1,
  "proofs": {
    "build": "pass",
    "test": "partial",
    "smoke": "missing",
    "security": "missing"
  }
}
```

人类摘要可以说“大致 60/100”，但必须说明分解依据。

### 10. Blocker 生命周期

状态：

- open。
- triaged。
- in_progress。
- waiting_external。
- waiting_user。
- fixed_pending_verification。
- closed。
- deferred。
- rejected。

关闭必须有 evidence。

### 11. Blocker 开启规则

开启 blocker 的触发：

- gate failed。
- smoke failed。
- user workflow 不通。
- recovery 不可信。
- policy 边界缺失。
- security finding。
- docs 与实际不符。
- release proof missing。

缺 proof 也可以是 blocker。

### 12. Blocker 关闭规则

关闭要求：

- root cause 已处理。
- acceptance artifacts 存在。
- verification commands 通过或有合理替代证据。
- release view 更新。
- no regression finding。

不能因为“计划里会做”而关闭。

### 13. Deferred 规则

Deferred 必须满足：

- 不阻止当前 release 目标。
- 用户或 release policy 接受。
- 有后续 milestone。
- 有风险说明。

Deferred 不是垃圾桶。

### 14. Proof 与 Evidence 的关系

Evidence 是通用证据。

Release Proof 是 release gate 认可的证据集合。

一个 EvidenceEvent 可以进入多个 proof：

- test output -> test proof。
- package smoke -> package proof。
- security scan -> security proof。

Release Proof 必须引用 Evidence，而不是复制文本。

### 15. 商用化 Gate

商用化 gate：

- install gate。
- first-run gate。
- project workflow gate。
- resume gate。
- permission gate。
- plugin gate。
- verification gate。
- report gate。
- packaging gate。
- docs gate。

每个 gate 必须有 pass/fail/block。

### 16. P0 Release Boundary

P0 release 不要求：

- 云端团队协作。
- 完整插件市场。
- 自动 EDA 下单。
- 企业 SSO。
- 高级成本统计。
- browser companion。

P0 release 必须要求：

- /project 可持续推进。
- /task 可闭环验证。
- evidence-first done。
- resume/continue 不丢目标。
- policy/approval 高风险可控。
- report 能说明当前进展和问题。

### 17. 商用阻塞报告

报告结构：

```text
Release: rel_0_1_candidate
Status: not_ready
Top blockers:
1. Resume state lacks stale context detection
2. Plugin trust policy missing install approval
3. Smoke proof missing for package lifecycle
Next local slice:
- Close cb_resume_state_missing
External blockers:
- API quota/account verification
```

报告要短，但可下钻。

### 18. 与 Dashboard 的关系

Dashboard 展示 release readiness。

Blocker Ledger 保存事实。

Release Proof 保存证据集合。

Report 生成面向人类的解释。

三者不能互相替代。

### 19. 与参考仓库的关系

参考吸收：

- gstack：关卡式审查、ship/canary/retro 思想。
- Archon：PR/validate/review artifact。
- ECC：跨 harness 状态、policy、MCP 可信边界。
- everything-claude-code：命令、rules、agents、验证生态。
- strix：security finding 必须可验证。

不照搬：

- 不把攻击自动化作为 P0。
- 不把 PR 自动化当成唯一交付面。
- 不把 dashboard 做成只有展示没有 proof 的外壳。

### 20. 验收

Release Proof 与 Commercial Blocker Ledger 设计完成标准：

- 定义 proof/blocker/readiness schema。
- 定义 blocker 分类、severity、owner_status。
- 定义 blocker 生命周期。
- 定义关闭证据要求。
- 定义 P0 release boundary。
- 定义商用化 gate。
- 明确 Dashboard/Proof/Ledger/Report 的边界。


# 第三部分：参考仓库覆盖与机制归因

---

<!-- source: docs/reference_audit/kiana_personal_project_os_reference_audit_2026-07-09.md -->

## Kiana 个人项目操作系统参考审计

日期：2026-07-09

本文是 `docs/superpowers/specs/2026-07-09-kiana-personal-project-os-design.md` 的详细参考审计附录。主设计文档只保留产品级决策和第一版实现规格；本文保留参考来源、可借鉴机制、风险边界和 backlog 映射。

### 1. 扫描范围

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

### 2. 38-Repo Coverage Matrix

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

### 3. Reference-Derived Capability Map

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

### 4. P0 Backlog

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

### 5. P1 Backlog

| P1 能力 | 参考来源 | Kiana 模块 | 可落地机制 | 风险边界 |
| --- | --- | --- | --- | --- |
| Bounded swarm / worker runtime | `superpowers`、`autogen`、`MetaGPT`、`architect-loop`、`ECC` | `kiana-coordinator` | typed worker、budget、termination、tool allowlist、path lock | 不过早做分布式 worker |
| Repo map / impact analysis | `GitNexus`、`graphify`、`aider`、`Archon` | `kiana-query::repo_map` | path/symbol/imports/tests/history/graph confidence ranking | graph 不能替代 source evidence |
| Memory ingest | `memorix`、`claude-memory`、`MemPalace` | memory consolidation | Observation/Reasoning/Git memory proposal、stale detection | 不自动写低置信 memory |
| Rules / skills 条件注入 | `continue`、`Roo-Code`、`ECC`、`everything-claude-code` | `kiana-skills` | alwaysApply、glob、regex、mode restrictions | 规则冲突必须可审计 |
| MCP provenance / visibility | `codex`、`ECC`、`everything-claude-code` | `kiana-services::mcp` | server provenance、visibility、required/optional、startup status | MCP 工具不能默认全暴露 |
| `/eda review` | 用户方向、`gstack`、`OpenSpec` | `kiana-eda` | BOM/DFM/Gerber/CPL/bring-up checklist | 不自动画板、不自动下单 |

### 6. P2 Backlog

| P2 能力 | 参考来源 | Kiana 模块 | 可落地机制 | 风险边界 |
| --- | --- | --- | --- | --- |
| Dashboard / graph / timeline | `memorix`、`OpenHands`、`GitNexus`、`graphify` | app-server context panel | workflow board、memory graph、repo graph timeline | 复用 Kiana app contract |
| Browser companion / web shell | `superpowers`、`OpenHands` | app-server、future Web UI | 设计审查、浏览器测试、远程 workspace | 先稳 RPC/event contract |
| Cost / usage attribution | `ruflo`、`ECC`、`cline` | usage ledger | provider、token、估算成本、worker budget | 成本必须标注估算 |
| Ship / canary / deploy | `gstack`、`get-shit-done` | release profile | ship、canary、deploy、retro、release channels | 不拖慢个人版早期迭代 |
| Hardware project dashboard | `/eda` workflow、OpenHands workspace view | future app-server UI | 原理图/BOM/Gerber/bring-up evidence timeline | 只显示 evidence，不替代专业 EDA |

### 7. WorkflowRun 节点到实现模块映射

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

### 8. 归因规则

- 参考结论必须保留来源归因。
- `reference/` live 事实优先于旧文档记忆；本轮确认 GitNexus 已存在。
- 近似目录不能替代目标仓库；同名目录存在时必须按实际内容归因。
- “可借鉴”不等于“已实现”。
- feature matrix 只有在代码、schema、测试、smoke、文档证据同时存在时，才能标记完成。
- 主设计文档只能写产品级决策和实现规格；实现细节必须映射到 backlog、schema、测试或 smoke。
- 每个外部参考最多贡献可落地机制，不贡献口号；无法落地的观察只能留在审计备注，不进入 P0。

---

## 单文件验收说明

- 本文件必须保持自包含，读者不打开其他文档也能理解 Kiana 的完整软件逻辑。
- 分卷中的 schema 示例属于数据契约设计，不代表当前代码已经实现。
- 参考项目只贡献可落地机制，不自动成为 Kiana 的产品承诺。
- 后续进入实现阶段时，应按 P0 milestone 拆 implementation plan，而不是直接按本文章节顺序写代码。
