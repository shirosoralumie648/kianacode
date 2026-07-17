# Kiana 产品功能与实现逻辑设计

日期：2026-07-10

状态：产品功能设计主规格

本文是 Kiana 的主设计文档。它只回答两类问题：

1. 用户能用 Kiana 完成什么事情。
2. Kiana 内部通过什么模块、状态、数据和执行协议完成这些事情。

本文不按参考仓库、抽象名词或技术分卷组织。参考项目只用于解释具体功能为什么采用某种实现机制。

旧文档 `docs/kiana-personal-project-os-complete-design.md` 保留为架构、流程和参考资料汇编，不再作为产品功能规格。

---

## 1. 产品定义

Kiana 是面向个人开发者、研究者和硬件项目操作者的项目执行系统。

它不是一个只能回答问题的聊天 CLI，也不是一个只会修改代码的 Coding Agent。它要持续管理：

- 项目目标。
- 项目当前状态。
- 需求和约束。
- 任务和依赖。
- 代码上下文。
- 并行 Worker。
- 测试、审查和交付证据。
- 长期记忆。
- EDA 项目资料和风险。

Kiana 的核心产品承诺是：

> 用户可以随时离开、回来并说“继续”，系统能够恢复项目、判断当前真实状态、选择下一项工作、执行并验证，而不是重新询问全部背景。

## 2. 目标用户

### 2.1 长期项目用户

典型需求：

- 项目跨越多天或多月。
- 同时维护多个仓库。
- 经常中断和恢复。
- 不希望每次重新解释背景。
- 需要清晰看到当前进度和下一步。

### 2.2 工程交付用户

典型需求：

- 从需求、实现、测试到交付形成闭环。
- 修改必须有验证。
- 失败必须有原因。
- 能区分“写完了”和“验证完成了”。

### 2.3 高并发开发用户

典型需求：

- 同时推进多个互不冲突的任务。
- 避免多个 Agent 修改同一文件。
- 主 Agent 统一集成和验证。
- 能暂停、终止和重新派发 Worker。

### 2.4 EDA 和硬件项目用户

典型需求：

- 管理原理图、PCB、BOM、Gerber、CPL 和打样约束。
- 审查电源、接口、器件和工艺风险。
- 生成 bring-up 计划。
- 高风险动作必须人工确认。

## 3. 产品功能地图

| 编号 | 产品功能 | 用户价值 | 第一阶段 |
| --- | --- | --- | --- |
| F01 | 项目空间与“继续” | 跨会话恢复项目并选择下一步 | P0 |
| F02 | 新建项目与需求捕获 | 把想法或 PRD 转为可执行项目 | P0 |
| F03 | WBS、里程碑、任务和看板 | 结构化管理长期项目 | P0 |
| F04 | 上下文、代码理解和长期记忆 | 给任务提供新鲜且可信的上下文 | P0 |
| F05 | 单任务 Coding Agent 执行 | 完成一个可验证的代码任务 | P0 |
| F06 | 参考仓扫描与差距修补 | 从本地参考项目学习机制并修补 Kiana | P0 |
| F07 | 有界并行和 Worker 调度 | 安全地同时推进多个任务 | P1 |
| F08 | 测试、验证、审查和证据 | 防止虚假完成声明 | P0 |
| F09 | Checkpoint、失败恢复和回滚 | 修改失败后能够恢复 | P0 |
| F10 | 项目报告和交付 | 输出可信的进度、问题和交付说明 | P0 |
| F11 | 工具、插件、Skill、MCP 和权限 | 扩展能力但保持安全边界 | P1 |
| F12 | EDA 项目工作台 | 复用项目系统管理硬件设计 | P1 |

## 4. 总体实现分层

Kiana 的内部结构按职责分为五层。

```text
CLI / TUI / SDK / App Server
          |
          v
Application Services
          |
          v
Project / Task / Workflow Domain
          |
          v
Tools / Agents / Query / Policy / Verification
          |
          v
Files / Git / Process / Network / Provider / Storage
```

### 4.1 入口层

对应模块：

- `kiana-entrypoints`
- `kiana-screens`

职责：

- 解析 CLI、TUI、SDK 和远程请求。
- 识别用户操作。
- 展示状态、确认和结果。
- 不直接实现项目业务逻辑。

### 4.2 Application Service 层

对应模块：

- `kiana-commands`

职责：

- 接收入口层请求。
- 调用多个领域组件完成一个用户功能。
- 管理事务边界。
- 生成用户可见结果。

### 4.3 领域状态层

对应模块：

- `kiana-tasks`
- `kiana-coordinator`

职责：

- 保存 Project、Workflow、Task、Dependency 和 Worker 状态。
- 决定状态是否允许迁移。
- 维护调度和单写者规则。

### 4.4 执行能力层

对应模块：

- `kiana-tools`
- `kiana-query`
- `kiana-skills`

职责：

- 读取和修改文件。
- 执行 shell、MCP、Agent 和其他工具。
- 构建上下文和代码索引。
- 加载规则、Skill、Plugin 和 Agent 定义。

### 4.5 基础设施层

对应模块：

- `kiana-services`
- `kiana-remote`
- `kiana-bridge`
- Git、文件系统、进程和网络适配器。

职责：

- 提供模型、远程 Session、Worker、Git、网络和存储能力。
- 不保存产品流程判断。

---

## 5. F01：项目空间与“继续”

### 5.1 用户目标

用户打开 Kiana 后，可以：

- 查看已有项目。
- 切换当前项目。
- 查看项目当前状态。
- 输入“继续”恢复工作。
- 指定一个历史 Workflow 继续。
- 在项目状态不可信时看到明确提示。

### 5.2 产品入口

目标命令：

```text
kiana project list
kiana project open <project>
kiana project status
kiana project continue
kiana project continue --workflow <id>
```

自然语言入口：

```text
继续
继续上次那个商业化任务
恢复 reference parity 项目
```

### 5.3 用户可见行为

`project status` 至少显示：

- 当前项目。
- 当前目标。
- 活跃 Workflow。
- 当前里程碑。
- Ready、Doing、Blocked 和 Done 数量。
- 当前最大风险。
- 推荐下一步。
- Git branch 和 dirty 状态。
- 上下文是否新鲜。

### 5.4 内部执行链

```text
User Input
  -> IntentRouter
  -> ProjectResolver
  -> WorkflowRepository
  -> StateReconciler
  -> RepoStateProbe
  -> ContextFreshnessService
  -> Scheduler
  -> PolicyEngine
  -> ProjectProjection
  -> User Confirmation / Task Execution
```

### 5.5 组件职责

| 组件 | 职责 |
| --- | --- |
| IntentRouter | 识别这是普通问答、项目状态查询还是恢复执行 |
| ProjectResolver | 根据显式 ID、当前目录、历史 Session 和最近项目选择目标项目 |
| WorkflowRepository | 加载 snapshot、state 和 EventLog |
| StateReconciler | 重放事件，修复或报告状态不一致 |
| RepoStateProbe | 读取 HEAD、branch、dirty files 和项目指纹 |
| ContextFreshnessService | 判断代码索引、记忆和 ContextPack 是否过期 |
| Scheduler | 从 Ready Task 中选择下一项 |
| PolicyEngine | 判断下一步是否需要用户批准 |
| ProjectProjection | 生成 CLI/TUI/API 状态视图 |

### 5.6 状态机

```text
IDLE
  -> RESOLVING_PROJECT
  -> LOADING_STATE
  -> RECONCILING
  -> READY
  -> EXECUTING

RECONCILING
  -> BLOCKED_STATE_CORRUPT
  -> BLOCKED_DIRTY_CONFLICT
  -> NEEDS_CONTEXT_REFRESH
```

### 5.7 上下文新鲜度

每个代码索引和代码绑定记忆必须标记：

- `current`
- `suspect`
- `stale`
- `unbound`

判断依据：

- index commit 是否等于当前 HEAD。
- 文件 hash 是否变化。
- symbol hash 是否变化。
- 依赖和调用者是否变化。
- 项目 fingerprint 是否变化。

`stale` 和 `unbound` 不能作为自动修改代码的依据。

### 5.8 持久化

目标目录：

```text
.kiana/projects/<project_id>/
  project.json
  workflows/
  tasks/
  decisions.jsonl
  evidence.jsonl
  memory/
```

Workflow 内：

```text
workflow.json
state.json
events.jsonl
context-pack.json
```

### 5.9 失败处理

| 场景 | 行为 |
| --- | --- |
| 找不到项目 | 展示候选项目，不猜测创建 |
| state 损坏 | 尝试从 EventLog 重建；失败则 Blocked |
| EventLog 缺失 | 使用 snapshot 但标记证据不足 |
| Git 有用户修改 | 不覆盖；显示冲突文件 |
| memory 与代码冲突 | 使用代码和测试，memory 标记 stale |
| 没有 Ready Task | 展示 blocker 和解除条件 |
| 多个 Ready Task | Scheduler 返回排序和原因 |

### 5.10 验收

- 用户输入“继续”后可以稳定选择同一个项目。
- state 和 EventLog 冲突时不会静默继续。
- stale index 不会作为可靠上下文。
- dirty files 不会被覆盖。
- Scheduler 能解释为什么选择下一项任务。

参考机制：

- GitNexus 的 index commit staleness。
- memorix 的 current/suspect/stale/unbound。
- OpenHands 的显式 sandbox recovery。

---

## 6. F02：新建项目与需求捕获

### 6.1 用户目标

用户提供想法、PRD、Issue 或一段自然语言后，Kiana 创建一个可执行项目，而不是只生成一份计划文本。

### 6.2 产品入口

```text
kiana project create
kiana project create --from prd.md
kiana project import-issue <url-or-file>
```

自然语言：

```text
我要做一个个人项目操作系统
把这份 PRD 变成项目
从这个 issue 开始做
```

### 6.3 产品输出

- Project。
- Goal。
- Success Criteria。
- Constraints。
- NOT_BUILDING。
- Workstreams。
- Milestones。
- 初始 Task Cards。
- 初始 Verification Strategy。
- 需要用户决定的问题。

### 6.4 内部执行链

```text
Input
  -> RequirementIngestor
  -> GoalExtractor
  -> ScopeClassifier
  -> ConstraintExtractor
  -> ProjectTemplateSelector
  -> WbsBuilder
  -> AcceptanceDesigner
  -> ProjectRepository
  -> BoardProjection
```

### 6.5 关键逻辑

RequirementIngestor 先区分：

- Feature。
- Bug。
- Refactor。
- Research。
- Product project。
- EDA project。

GoalExtractor 生成：

- 用户希望达成的结果。
- 可验证成功条件。
- 不应自动扩展的范围。

WbsBuilder 只拆到可理解的 Milestone，不直接生成几百个微任务。

Task Cards 在用户开始执行或 Milestone 进入 Ready 时再细化。

### 6.6 决策规则

需要问用户：

- 两条路线会导致不同产品。
- 成本、平台或技术栈会改变架构。
- 需求中包含不可逆硬件或发布动作。
- 成功标准无法验证。

可以采用默认：

- 文档位置。
- 低风险命名。
- 可随时更改的展示格式。

### 6.7 状态机

```text
DRAFT
  -> CAPTURING
  -> NEEDS_DECISION
  -> PLANNING
  -> READY

CAPTURING
  -> REJECTED_INVALID_SCOPE
  -> SPLIT_REQUIRED
```

### 6.8 验收

- Project 必须有 Goal、Success Criteria 和 NOT_BUILDING。
- Milestone 必须可验证。
- 不能把“完善系统”作为可执行 Task。
- 需求不清时不能直接进入代码修改。

---

## 7. F03：WBS、里程碑、任务和看板

### 7.1 用户目标

用户可以看到：

- 项目为什么做。
- 现在在哪个阶段。
- 哪些任务可执行。
- 哪些任务被阻塞。
- 哪些任务可以并行。
- 哪些完成状态有证据。

### 7.2 产品视图

Kiana 提供五种投影：

- WBS：目标和结构。
- Kanban：执行状态。
- Dependency：依赖图。
- Risk：风险和 blocker。
- Release：交付 readiness。

### 7.3 领域结构

```text
Project
  -> Goal
  -> Workstream
  -> Milestone
  -> Task
  -> WorkPacket
```

Task 是项目管理单位。

WorkPacket 是一次执行单位。

一个 Task 可以有多个 WorkPacket，但一个 WorkPacket 只能属于一个 Task。

### 7.4 Task 状态

```text
BACKLOG
  -> SPEC
  -> READY
  -> IN_PROGRESS
  -> REVIEW
  -> DONE

READY / IN_PROGRESS / REVIEW
  -> BLOCKED

BLOCKED
  -> READY
  -> CANCELLED
```

### 7.5 Ready 判定

Task 只有满足以下条件才能进入 Ready：

- 所有依赖完成。
- 目标和范围明确。
- 允许修改文件已知。
- 验收标准可执行。
- 验证命令存在。
- 不存在未解决审批。
- 不存在路径冲突。

### 7.6 Board 实现链

```text
Task Events
  -> TaskRepository
  -> DependencyGraph
  -> Scheduler
  -> RiskCalculator
  -> BoardProjection
  -> CLI / TUI / App
```

Board 是事件和领域状态的投影，不是唯一事实源。

### 7.7 查询能力

必须支持：

- `next_task`
- `blocked_tasks`
- `ready_parallel`
- `missing_evidence`
- `stale_tasks`
- `decision_needed`
- `release_blockers`

每个查询必须返回排序原因。

### 7.8 验收

- Done Task 必须引用 Evidence。
- Blocked Task 必须有 blocker reason 和解除条件。
- Ready Task 必须有 verification。
- 同一 Task 不得同时处于多个执行状态。

---

## 8. F04：上下文、代码理解和长期记忆

### 8.1 用户目标

用户不需要重复告诉 Kiana：

- 项目结构。
- 当前技术栈。
- 关键约束。
- 已经验证的历史结论。
- 某个 symbol 的影响范围。

系统也不能因为旧记忆错误地修改当前代码。

### 8.2 上下文组成

ContextPack 包含：

- 用户本轮目标。
- 项目 Goal 和 Constraints。
- 当前 Task/WorkPacket。
- 相关文件片段。
- 相关 symbol 和调用者。
- 相关测试。
- 当前 Git 状态。
- 已验证记忆。
- 风险和审批要求。

### 8.3 内部执行链

```text
Task Scope
  -> RepoIndexer
  -> SymbolExtractor
  -> DependencyResolver
  -> ContextSearch
  -> MemoryRetriever
  -> FreshnessEvaluator
  -> ContextRanker
  -> ContextPack
```

### 8.4 Repo Map

Repo Map 不只是文件列表。

每个节点至少包含：

- 文件路径。
- language。
- symbol。
- definition/reference。
- imports/dependencies。
- tests。
- content hash。
- index commit。

排序信号：

- 用户明确提到。
- Task allowed files。
- symbol 依赖。
- 测试关系。
- 最近变更。
- 结构中心度。
- token 预算。

### 8.5 Memory 类型

- Observation：直接观察事实。
- Decision：用户或系统已确认的决定。
- Reasoning：当时的推理过程。
- Verification：命令和测试结果。
- Preference：用户长期偏好。
- Warning：已知风险和失败模式。

### 8.6 Memory 写入规则

允许写入长期 Memory：

- 用户明确决策。
- 重复出现的稳定偏好。
- 有命令证据的项目事实。
- 已验证的故障原因。

不允许直接写入：

- 临时猜测。
- 未验证的 TODO。
- Agent 自己的完成声明。
- 已过期的代码路径。

### 8.7 新鲜度和置信度

置信度：

- `EXTRACTED`
- `INFERRED`
- `AMBIGUOUS`

新鲜度：

- `current`
- `suspect`
- `stale`
- `unbound`

高风险修改只允许使用 `EXTRACTED + current` 作为直接依据。

### 8.8 验收

- 当前代码和 memory 冲突时使用当前代码。
- ContextPack 能说明每条内容来自哪里。
- 代码索引过期时必须提示刷新。
- ContextPack 必须受 token 预算限制。

参考机制：

- aider 的 RepoMap 排序。
- Continue 的 signature map 和 token 裁剪。
- GitNexus 的 impact/staleness。
- graphify 的置信度等级。
- MemPalace 的时态事实。

---

## 9. F05：单任务 Coding Agent 执行

### 9.1 用户目标

用户给出一个明确任务后，Kiana 可以：

- 理解相关代码。
- 修改正确文件。
- 运行验证。
- 修复失败。
- 输出可检查的结果。

### 9.2 产品入口

```text
kiana task run <task_id>
kiana -p "修复 session resume 的重复事件问题"
```

### 9.3 执行前条件

- Task 已 Ready。
- Scope 已明确。
- ContextPack 新鲜。
- allowed files 已定义。
- verification 已冻结。
- policy 允许执行。

### 9.4 内部执行链

```text
Task
  -> WorkPacketBuilder
  -> ContextAssembler
  -> CheckpointService
  -> AgentRunner
  -> ToolExecutionPipeline
  -> ResultCollector
  -> VerificationService
  -> ReviewService
  -> TaskStateMachine
```

### 9.5 WorkPacket

必须包含：

- Goal。
- In scope。
- NOT_BUILDING。
- Read first。
- Allowed files。
- Forbidden files。
- Consumes。
- Produces。
- Test plan。
- Frozen verification。
- Rollback。
- Risk。
- Result contract。

### 9.6 工具执行链

```text
Tool Request
  -> PreToolUse Hook
  -> Exec Policy
  -> Sandbox Policy
  -> Approval
  -> Input Validation
  -> Execution
  -> Changed File Capture
  -> PostToolUse Hook
  -> Runtime Event
```

任何工具拒绝都必须终止当前调用，不允许 fallback 成假成功。

### 9.7 代码修改流程

1. 读取目标文件和测试。
2. 创建 checkpoint。
3. 编写失败测试或最小复现。
4. 确认失败原因。
5. 修改最小代码范围。
6. 运行目标测试。
7. 运行相关回归。
8. 审计 changed files。
9. 生成 ResultPacket。

### 9.8 状态机

```text
READY
  -> PREPARING
  -> EXECUTING
  -> VERIFYING
  -> REVIEWING
  -> DONE

EXECUTING / VERIFYING / REVIEWING
  -> FIXING
  -> BLOCKED
  -> ROLLED_BACK
```

### 9.9 验收

- 只修改 allowed files。
- 用户已有修改未被覆盖。
- 行为变化有测试或可复查验证。
- ResultPacket 包含 changed files 和命令结果。
- 验证失败不能进入 Done。

---

## 10. F06：参考仓扫描与差距修补

### 10.1 用户目标

用户可以要求：

```text
扫描 Codex、Cline 和 Claude Code 的 permission lifecycle，
对比 Kiana 当前实现，证明真实缺口并完成最小修补。
```

### 10.2 产品入口

现有入口：

```bash
kiana --agent reference-repairer -p "<目标>"
```

Agent 定义：

```text
.kiana/agents/reference-repairer.md
```

未来命令入口：

```text
kiana reference scan <capability>
kiana reference gap <capability>
kiana reference repair <gap_id>
```

### 10.3 三阶段模型

```text
SCAN
  -> GAP
  -> REPAIR
```

SCAN 只读。

GAP 只生成差距证据。

REPAIR 才能修改 Kiana。

### 10.4 SCAN 阶段

输入：

- 目标能力。
- 指定 reference 仓库。
- Kiana 候选模块。

输出 `RepoScanContext` 和 FindingPackets。

每个 Finding 必须包含：

- reference commit。
- source path 和 line。
- test path 和 line。
- 机制说明。
- 用户价值。
- 置信度。
- 候选 Kiana 落点。

### 10.5 GAP 阶段

差距成立需要同时证明：

- reference 有可验证机制。
- Kiana 当前没有等价行为。
- 差距影响真实用户流程。
- 有最小复现或测试可以证明。

差距不能仅来自：

- README 口号。
- TODO。
- demo。
- 未调用代码。
- 旧 roadmap。

### 10.6 REPAIR 阶段

```text
Verified Gap
  -> Impact Analysis
  -> WorkPacket
  -> Approval
  -> Checkpoint
  -> Minimal Patch
  -> Frozen Verification
  -> Independent Review
  -> ResultPacket
```

### 10.7 只读和写入边界

- `reference/**` 永远只读。
- 扫描 Agent 不能修改 Kiana。
- 主 Repair Agent 是唯一写入者。
- 多个 Repair Agent 不能并行修改同一主仓。
- 合并冲突视为拆分失败。

### 10.8 差距置信度

- `EXTRACTED`：源码和测试直接证明。
- `INFERRED`：多个机制组合推断。
- `AMBIGUOUS`：证据不足。

只有 `EXTRACTED` 可以直接进入自动修补候选。

### 10.9 修补前审批

以下修改必须用户批准：

- public schema。
- protocol。
- remote transport。
- auth、permission、policy、hook。
- plugin trust。
- release、license、signing。
- sandbox 关闭。
- destructive shell。
- 用户已修改的同一区域。

### 10.10 验收

- Agent 能被 `kiana agents --json` 发现。
- reference 仓没有产生修改。
- 每个差距有源码和测试路径。
- 修补发生在隔离 worktree。
- 每次只修一个独立问题。
- 更新 feature matrix 前必须有验证证据。

参考机制：

- Claude Code 的 tool permission pipeline。
- Codex 的 approval/sandbox/exec policy 分层。
- Cline 的 checkpoint 和 team child session。
- Architect Loop 的 frozen checks 和 postflight。
- Superpowers 的 fresh worker 和逐任务 review。

---

## 11. F07：有界并行和 Worker 调度

### 11.1 用户目标

用户可以同时推进多个任务，但不会因为多个 Agent 修改同一资源而破坏项目。

### 11.2 并行前提

两个 WorkPacket 可以并行必须同时满足：

- 所有依赖完成。
- allowed files 不重叠。
- 不共享 lockfile、schema、migration、generated output 或运行服务。
- 不存在 produces/consumes 依赖。
- verification 可以独立运行。
- policy 允许。

### 11.3 内部执行链

```text
Ready Tasks
  -> DependencyResolver
  -> ConflictAnalyzer
  -> WavePlanner
  -> WorkerLauncher
  -> WorkerMonitor
  -> ResultCollector
  -> Postflight
  -> SerialIntegrator
```

### 11.4 Wave

Wave 是一组可以同时执行的 WorkPacket。

每个 Worker：

- 独立 worktree。
- 独立 ContextPack。
- 独立 ResultPacket。
- 独立退出状态。

共享：

- 只读 Project Goal。
- 只读冻结计划。
- 只读 verification profile。

### 11.5 单写者

以下状态只能由 Orchestrator 写：

- WorkflowRun 状态。
- Task 最终状态。
- 合并状态。
- 共享 Evidence Ledger。
- Release readiness。

Worker 只能写自己的工作目录和 ResultPacket。

### 11.6 Postflight

每个 Worker 结束后检查：

- exit status。
- ResultPacket。
- changed files。
- forbidden files。
- verification。
- branch/worktree 状态。
- merge conflict。

缺失任何一项都不能集成。

### 11.7 集成

Worker 可以并行执行，但合并必须串行：

1. 选择一个已通过门禁的结果。
2. 应用或合并。
3. 运行受影响测试。
4. 更新共享状态。
5. 再处理下一个结果。

### 11.8 验收

- 并行 Worker 不共享写集合。
- 合并冲突不会临场硬解。
- Worker 超时可以终止并 fresh respawn。
- 每次合并后重新验证。

---

## 12. F08：测试、验证、审查和证据

### 12.1 用户目标

用户能够判断：

- 功能是否真的工作。
- 哪些命令运行过。
- 哪些测试失败。
- 哪些风险被接受。
- 为什么任务被标记完成。

### 12.2 Verification Profile

根据任务选择验证层级：

| Profile | 内容 |
| --- | --- |
| Document | 结构、链接、术语、一致性 |
| Local Code | format、targeted test、diff check |
| Cross Module | dependent tests、schema、integration |
| Release | workspace test、build、smoke、package |
| Security | policy、secret、dependency、sandbox |
| EDA | ERC/DFM/BOM/bring-up checklist |

### 12.3 验证链

```text
ResultPacket
  -> Structural Checks
  -> Targeted Tests
  -> Dependency Regression
  -> Acceptance Criteria
  -> Scope Audit
  -> Review
  -> Evidence Ledger
```

### 12.4 Evidence

Evidence 至少包含：

- type。
- command。
- exit code。
- captured time。
- source file。
- related Task。
- result。
- hash 或 artifact path。

### 12.5 Review

Review 分为：

- Product：是否解决用户问题。
- Architecture：是否破坏边界。
- Code：实现质量。
- Security：权限和风险。
- QA：测试覆盖。
- Scope：是否越界。

Review Finding 不能直接被忽略，必须：

- FIX。
- ACCEPT_RISK。
- OUT_OF_SCOPE。
- BLOCKED。

### 12.6 验收

- Done 必须引用 VerificationPacket。
- Verification command 和结果可复查。
- Reviewer 读取 WorkPacket、ResultPacket 和完整 diff。
- 实现 Worker 不能删除冻结检查。

---

## 13. F09：Checkpoint、失败恢复和回滚

### 13.1 用户目标

任何 Agent 修改都能回答：

- 修改前是什么状态。
- 修改了哪些文件。
- 能否撤销。
- 失败后从哪里继续。

### 13.2 Checkpoint

修补前保存：

- HEAD。
- staged patch。
- unstaged patch。
- untracked files。
- Task/Workflow ID。
- allowed files。
- 时间和 Agent ID。

### 13.3 恢复类型

- Retry：同一 WorkPacket 再尝试。
- Replan：重新拆任务。
- Rollback：恢复 checkpoint。
- Resume：从持久状态继续。
- Fork：保留当前结果，创建新分支路线。

### 13.4 失败分类

| 类型 | 行为 |
| --- | --- |
| Transient | 有界重试 |
| Test Failure | 进入 Fix Loop |
| Scope Conflict | Blocked 或重新拆分 |
| Permission Denied | 等待批准或降级 |
| State Corrupt | Event replay / repair |
| Worker Lost | fresh respawn |
| Reference Ambiguous | 不修补 |

### 13.5 Sandbox 生命周期

```text
CREATED
  -> STARTING
  -> RUNNING
  -> PAUSED
  -> RESUMING
  -> RUNNING
  -> STOPPED
  -> DELETED
```

恢复必须由用户动作或 Orchestrator 明确触发，不能在后台静默恢复失效 Worker。

### 13.6 验收

- checkpoint 不修改用户工作树。
- rollback 后能够验证恢复结果。
- Worker 丢失不会自动复用污染上下文。
- 同类失败最多有限重试。

---

## 14. F10：项目报告和交付

### 14.1 用户目标

用户可以生成：

- 当前进度。
- 当前问题。
- 下阶段计划。
- 技术审计。
- Release readiness。
- 老师或管理者汇报。

### 14.2 数据来源

报告只能使用：

- Project/Workflow 状态。
- Task 状态。
- Evidence Ledger。
- Verification。
- Review Findings。
- Decision 和 Approval。
- live Git/command 状态。

旧 memory 只能作为线索。

### 14.3 报告类型

- Progress Report。
- Blocker Report。
- Audit Report。
- Release Report。
- Handoff Report。
- EDA Review Report。

### 14.4 内部执行链

```text
Report Request
  -> ReportScopeResolver
  -> EvidenceQuery
  -> StateProjection
  -> ConsistencyCheck
  -> AudienceFormatter
  -> Report
```

### 14.5 验收

- 计划不能被写成进展。
- 未验证项标为 Unknown 或 Blocked。
- 报告结论可以追溯到 Evidence。
- 面向非技术听众时不堆砌内部对象名。

---

## 15. F11：工具、插件、Skill、MCP 和权限

### 15.1 用户目标

用户可以扩展 Kiana，但第三方能力不能绕过项目权限。

### 15.2 扩展类型

- Tool。
- Agent。
- Skill。
- Command。
- Hook。
- MCP Server。
- Plugin。
- Rule Pack。

### 15.3 Trust 来源

- Built-in。
- User。
- Project。
- Local。
- Plugin。
- Managed organization policy。

项目未被信任时：

- 不加载 project agents。
- 不加载 project hooks。
- 不加载 project MCP。
- 不允许 mutating tools。

### 15.4 权限决策

```text
Managed Deny
  > User / Session Deny
  > Managed Ask
  > Managed Allow
  > Normal Allow
  > Normal Ask
  > Permission Mode
```

### 15.5 Approval Scope

Approval 必须绑定：

- operation。
- tool。
- path 或 target。
- risk。
- 有效期。
- Workflow/Task。

不能用一次“允许写文件”授权后续任意网络、发布或 policy 修改。

### 15.6 Agent 定义

项目 Agent 需要声明：

- name。
- description。
- tools。
- disallowed tools。
- memory scope。
- permission mode。
- max turns。
- isolation。
- background。

### 15.7 验收

- untrusted project agent 不可加载。
- plugin/agent 来源可见。
- dangerous shell 进入 ask/deny。
- tool result 包含 changed files 和错误对象。
- Hook 不能静默升级权限。

---

## 16. F12：EDA 项目工作台

### 16.1 产品定位

EDA 不是独立的第二套 Kiana。

它复用：

- Project。
- Workflow。
- Task。
- WorkPacket。
- Evidence。
- Verification。
- Review。
- Approval。

只增加硬件领域对象和规则。

### 16.2 产品功能

- 硬件需求捕获。
- 原理图审查。
- 电源树审查。
- 接口和电平审查。
- BOM 风险。
- PCB/DFM 审查。
- Gerber/CPL/BOM 完整性。
- Bring-up 计划。
- 打样和下单审批。

### 16.3 内部执行链

```text
EDA Project
  -> Artifact Ingest
  -> Domain Rule Selection
  -> Schematic / BOM / DFM Analyzers
  -> Risk Findings
  -> Hardware Review
  -> Approval
  -> Bring-up Plan
```

### 16.4 Artifact

- Schematic。
- PCB。
- BOM。
- Gerber。
- CPL。
- Datasheet。
- Requirement。
- Test Record。

每个 Artifact 需要：

- path。
- format。
- version/hash。
- source。
- captured time。
- validation status。

### 16.5 风险分类

- Electrical。
- Power。
- Interface。
- Component availability。
- Lifecycle。
- DFM。
- Safety。
- Bring-up。

### 16.6 高风险动作

必须人工批准：

- 自动替换器件。
- 修改电源架构。
- 输出生产文件。
- 下单。
- 产生费用。
- 绕过 ERC/DFM。

### 16.7 P0 边界

第一阶段：

- 审查。
- 风险清单。
- BOM 分析。
- Bring-up 计划。

不做：

- 自动画原理图。
- 自动布线。
- 自动下单。
- 代替工程师最终电气签字。

---

## 17. Intent Router 实现逻辑

Router 决定使用哪个产品功能，不直接执行任务。

### 17.1 输入信号

- 显式命令。
- 用户自然语言。
- 当前项目状态。
- 风险。
- 预计修改范围。
- 验证需求。
- Ready Task 数量。
- EDA Artifact。

### 17.2 决策顺序

```text
1. 解析显式命令。
2. 分类用户目标。
3. 判断是否需要项目状态。
4. 判断是否需要修改。
5. 判断风险和审批。
6. 判断是否存在多个安全并行任务。
7. 选择功能入口。
8. 输出选择原因。
```

### 17.3 路由结果

- Answer。
- Quick Action。
- Task Execute。
- Project Manage。
- Reference Repair。
- Swarm。
- Audit。
- Report。
- EDA。
- Approval Required。

Router 选择不能绕过 Policy。

---

## 18. Scheduler 实现逻辑

Scheduler 选择下一项 Task 或下一组 Wave。

### 18.1 排序信号

- 用户优先级。
- Critical path。
- Blocker unblock value。
- Risk reduction。
- Evidence value。
- Verification readiness。
- Path conflict。
- Estimated size。
- Stale risk。

### 18.2 输出

```text
selected:
parallel_candidates:
skipped:
reasons:
requires_approval:
```

### 18.3 约束

- 不选择依赖未完成的任务。
- 不选择没有验证策略的任务。
- 不将两个共享可变资源的任务放进同一 Wave。
- 不允许 Worker 自己改变调度结果。

---

## 19. 事件和一致性

### 19.1 EventLog

EventLog 是 append-only。

核心事件：

- ProjectCreated。
- WorkflowCreated。
- TaskCreated。
- TaskReady。
- TaskStarted。
- ToolRequested。
- ToolApproved。
- ToolCompleted。
- EvidenceRecorded。
- VerificationPassed。
- VerificationFailed。
- ReviewCompleted。
- TaskBlocked。
- TaskCompleted。
- WorkerStarted。
- WorkerStopped。
- CheckpointCreated。
- RollbackCompleted。

### 19.2 Materialized State

`state.json`、Board、Dashboard 都是 EventLog 的投影。

冲突时：

1. 检查 EventLog。
2. 重建 state。
3. 生成 consistency finding。
4. 无法修复时 Blocked。

### 19.3 幂等

每个命令和事件需要：

- command_id。
- event_id。
- workflow_id。
- task_id。
- actor_id。

重复投递不能重复执行不可逆动作。

---

## 20. P0 纵向交付切片

P0 不按 crate 或对象拆，而按完整用户功能交付。

### Milestone 1：项目空间与继续

交付：

- 创建/列出/打开项目。
- Workflow state 和 EventLog。
- `project status`。
- `project continue`。
- Git dirty 和 stale context 检查。

验收：

- 中断后可以继续。
- 不能覆盖用户修改。
- 能解释下一步。

### Milestone 2：Task 和单任务执行

交付：

- Task Card。
- Ready 判定。
- WorkPacket。
- Checkpoint。
- Tool execution。
- ResultPacket。

验收：

- 一个真实 bugfix 可以完成并验证。
- 失败可以 Blocked 或 rollback。

### Milestone 3：Evidence 和 Verification

交付：

- Verification Profile。
- Evidence Ledger。
- Review Finding。
- Done Gate。

验收：

- 没有 Evidence 不能 Done。
- 报告能引用验证结果。

### Milestone 4：Reference Repair

交付：

- `reference-repairer` Agent。
- SCAN/GAP/REPAIR 协议。
- confidence 和 stale。
- Worktree 修补。
- reference 只读门禁。

验收：

- 能扫描指定 reference。
- 能证明一个真实差距。
- 能完成一个最小修补。
- reference 仓无修改。

### Milestone 5：Project Board

交付：

- WBS。
- Kanban。
- blocker。
- next task。
- progress report。

验收：

- 用户能看到真实项目状态。
- Board 与 EventLog 一致。

### Milestone 6：Bounded Swarm

交付：

- Wave Planner。
- path ownership。
- Worker worktree。
- Postflight。
- Serial integration。

验收：

- 两个独立任务可以并行。
- 冲突任务不会被同时派发。

---

## 21. 产品完成标准

### 21.1 功能完成

一个功能完成需要：

- 用户入口可用。
- 主流程可用。
- 失败分支可见。
- 状态可恢复。
- 权限边界明确。
- 验收命令通过。

### 21.2 项目系统完成

- “继续”不丢目标。
- Project Board 可恢复。
- Task 能完整闭环。
- Evidence 能证明 Done。
- Agent 不覆盖用户修改。
- Reference Repair 能安全学习本地参考仓。

### 21.3 商用准备

- 安装和配置可验证。
- 权限和 trust fail-closed。
- Plugin/MCP/Agent 来源可见。
- Release proof 可复查。
- 外部 blocker 和本地 blocker 分离。
- 不依赖口头完成声明。

---

## 22. 参考机制映射

| 产品功能 | 参考机制 |
| --- | --- |
| 项目恢复 | OpenHands sandbox recovery、GitNexus staleness |
| 工具权限 | Claude Code toolExecution、Codex approval/sandbox/exec policy |
| Checkpoint | Cline checkpoint restore、Continue review worktree |
| Repo Map | aider RepoMap、Continue signatures、GitNexus graph |
| Memory Freshness | memorix code binding、MemPalace temporal facts |
| 并行执行 | Architect Loop frontier/postflight、Superpowers fresh worker |
| Workflow | Ruflo pause/resume、GSD waves、Planning With Files persistence |
| 验证交付 | gstack gates、Superpowers verification、aider lint/test loop |
| Reference Repair | 本文 F06 和 `.kiana/agents/reference-repairer.md` |

参考仓库提供机制证据，不自动成为 Kiana 的功能承诺。
