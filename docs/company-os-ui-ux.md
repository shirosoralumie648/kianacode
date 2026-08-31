# Kiana CompanyOS UI/UX 交互规范

> 文档性质：用户界面与交互规范目标（Normative Target）。
>
> 本文定义 CLI/TTY、Web Workbench、Desktop 壳和未来多项目工作台如何呈现、解释和操作 CompanyOS 状态。本文不创建第二套 Runtime，也不把 UI 状态当成事实源。
>
> 当前产品事实以 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) 和 [`../USER.md`](../USER.md) 为准。当前 Web 不宣称 token streaming，`kiana tui` 仍是 parked legacy surface。

> **本文速览（导读，非规范）**
>
> - **讲什么**：CLI/TTY、Web 工作台、Desktop 壳怎么呈现和操作系统状态——五条设计原则（状态优先于对话、解释优先于自动化…）、统一的状态与颜色表、审批卡/取消/评审/验收的交互流程、TTY 布局与斜杠命令分层、Web 事件同步与多标签页规则、无障碍要求和 UI 状态协议（UiSnapshot/UiAction）。
> - **回答的问题**："用户怎么一眼看懂 Agent 在干嘛、为什么被卡住、以及怎么安全地批准/拒绝/取消。"
> - **核心规则**：UI 只是服务端事实的投影——不能自己维护第二套状态，不能因为模型说"完成了"就显示绿色。
> - **什么时候读**：改任何用户界面（终端/网页/桌面）之前。
>
> 术语看不懂先查 [`company-os-overview.md`](company-os-overview.md) 的白话词典。

## 1. 产品目标

Kiana 的 UI 不是“聊天气泡加一堆按钮”，而是一个让人能够安全地管理工作的控制台：

```text
看懂目标
  → 看懂当前状态
  → 看懂 Agent 正在做什么
  → 看懂它为什么被阻断
  → 在需要时批准、拒绝、取消、返工或接管
  → 查看证据和结果
  → 决定下一步
```

用户不应该需要阅读 EventLog 或理解 Rust crate 才能回答：

- 这次工作属于哪个项目；
- Agent 当前在哪一步；
- 它修改了什么；
- 哪个工具正在等待；
- 是否有风险或审批；
- 取消是否真的收敛；
- 结果是成功、失败还是未知；
- 我现在需要做什么。

## 2. 设计原则

### 2.1 状态优先于对话

Conversation 是输入和解释层；Project、Workflow、Run、Invocation、Artifact、Approval、Incident 和 Receipt 才是可操作状态。

```text
Chat message       讨论，不自动产生授权
Action card        对某个正式对象的可执行操作
Timeline event     已提交事实的可读投影
Receipt            可追溯的结果报告
```

不能因为模型说“完成了”就显示绿色完成；必须等待服务端 terminal state 和 Receipt。

### 2.2 解释优先于自动化

UI 应说明：

```text
状态是什么
为什么是这个状态
谁负责
系统正在等待谁
哪些操作可用
哪些操作被禁止
如果继续/拒绝/取消会发生什么
```

### 2.3 危险动作显式、精确、可逆

Approval、外部发送、写盘、合并、删除、取消和恢复必须有明确的动作卡：

- exact target；
- payload/diff 摘要；
- 风险等级；
- 预算影响；
- 证据和来源；
- 过期时间；
- 取消或回滚方式。

不能使用含糊的“继续”“允许全部”“自动处理剩余问题”作为高风险确认文案。

### 2.4 同一事实，多种投影

```text
DaemonHost / ControlPlane / EventLog
              │
              ▼
       State + Event protocol
        ┌─────┼─────┐
        ▼     ▼     ▼
      TTY    Web  Desktop
```

不同入口可以有不同布局，但不能有不同的状态、审批或权限语义。

### 2.5 Progressive disclosure

默认显示最小必要信息；用户可以展开：

```text
摘要 → 详细状态 → 原始事件 → 完整 payload/diff → provenance
```

不要默认把所有工具 schema、完整 transcript、所有 Memory 命中和所有子 Cell 日志同时塞给用户。

## 3. 交互对象与状态显示

### 3.1 统一对象层级

```text
Organization
  → Objective / Portfolio / Program
  → Project
  → Milestone
  → WorkPacket / WorkflowInstance
  → Cell / Run
  → Invocation / Artifact / Evidence
  → Review / Acceptance / Delivery
  → Receipt / Outcome / Incident
```

UI 的导航和面包屑应保留这条关系。用户从一条 Tool Event 能够回到它所属的 Run、Packet、Project 和 Objective。

### 3.2 状态与颜色

状态颜色只作辅助，必须同时有文字、图标或 aria label：

| 状态 | 文案 | 视觉建议 | 用户下一步 |
|---|---|---|---|
| `idle` | 空闲 | 中性 | 输入目标或选择已有工作 |
| `queued` | 排队中 | 中性/等待 | 查看前方任务或取消 |
| `running` | 执行中 | 蓝色/动态 | 查看当前步骤或取消 |
| `awaiting_approval` | 等待审批 | 琥珀色/醒目 | 查看 exact payload 后批准/拒绝 |
| `waiting_input` | 等待输入 | 紫色/醒目 | 补充信息或关闭 |
| `blocked` | 被阻塞 | 琥珀色 | 查看 blocker、解决或升级 |
| `retrying` | 重试中 | 蓝色 | 查看 attempt 和退避时间 |
| `cancelling` | 正在取消 | 琥珀色 | 等待收敛，不重复点击 |
| `cancelled` | 已取消 | 灰色 | 查看是否有 partial effect |
| `failed` | 失败 | 红色 | 查看错误、重试或新建任务 |
| `result_unknown` | 结果待确认 | 红色/紫色 | 进入对账/Incident，不盲重试 |
| `ready_for_review` | 待审核 | 蓝色 | 打开 Review |
| `needs_change` | 需要修改 | 琥珀色 | 查看返工原因 |
| `completed` | 已完成 | 绿色 | 查看 Receipt 和交付 |

### 3.3 状态卡最低内容

每个 Project、Workflow、Run、Approval 和 Incident 卡片至少显示：

```text
object type + title + status
owner / waiting actor
parent project or workflow
created / updated / deadline
next action
risk / budget summary
last event time
```

详细内容从 Event/State/Artifact projection 加载，不由 UI 自己计算一个平行状态。

## 4. 信息架构

### 4.1 Web/Desktop 工作台

建议的一级导航：

```text
Workspace switcher
├── Home / Today
├── Projects
├── Inbox
│   ├── Approvals
│   ├── Reviews
│   ├── Acceptance
│   ├── Blocked work
│   └── Incidents / Reconciliation
├── Runs
├── Workflows
├── Memory / Knowledge
├── Tools / MCP
├── Artifacts / Delivery
├── Activity / Audit
└── Settings / Trust / Models
```

当前最小版本可以只实现：

```text
Workspace
├── Conversation
├── Current run status
├── Changed files / Receipt
└── Trust / Sandbox / session controls
```

不要在当前 `local_behavior` 阶段伪装出尚未有后端事实支持的 Project dashboard、实时 Swarm graph 或真实 Approval inbox。

### 4.2 Home / Today

Home 不应是一个泛化聊天框，而应显示：

- 当前工作区和信任状态；
- 正在运行的 Run；
- 需要用户处理的 Approval/Review/Acceptance；
- 被阻塞和结果未知的工作；
- 最近交付和 Receipt；
- 项目风险、预算和截止时间；
- 最近使用的 Workspace。

排序优先级：

```text
安全/结果未知
  → 需要人工决定
  → 已阻塞/即将超时
  → 正在运行
  → 最近完成
```

### 4.3 Project 页面

```text
Header: Project title / status / owner / trust / budget
Summary: Objective / success criteria / deadline / risk
Plan: milestones / dependencies / work packets
Activity: event timeline / current runs
Evidence: artifacts / tests / reviews
Decisions: approvals / changes / acceptance
Outcome: measurements / lessons
```

Project 页面不得把单个 Run 的完成状态直接渲染成 Objective Achieved；业务结果必须来自 Outcome。

### 4.4 Run 页面

Run 页面是最重要的执行观测界面：

```text
Run header
  status / run_id / owner / project / packet / elapsed / budget
Current step
  provider/model or deterministic node
  current capability/invocation
  waiting reason
Timeline
  user input / model turn / tool call / approval / result / retry
Artifacts
  diff / files / stdout / tests / snapshots
Controls
  cancel / pause / approve / deny / retry / open incident
Receipt
  final state / evidence / exceptions / next action
```

默认显示“当前正在发生什么”；原始模型内容、完整 tool input、Secret、内部 reasoning 和不必要的环境变量必须隐藏或脱敏。

## 5. 核心交互流程

### 5.1 打开工作区

```text
选择目录
  → 显示绝对路径和最近状态
  → 显示 trust/sandbox 解释
  → 用户确认工作区
  → 创建或恢复 Session
  → 显示当前状态和可用操作
```

工作区确认卡应明确：

- Kiana 将在哪个目录工作；
- 当前是否 trusted；
- sandbox profile；
- 默认会读取/写入什么；
- 哪些能力不可用；
- 是新 Session 还是 resume。

当前 Desktop welcome 页已采用“选择工作区 → 信任 → 用自然语言派活”的三步模型，见 `contrib/desktop/welcome.html:40-80`；该模型应作为后续工作台 onboarding 的基础。

### 5.2 提交任务

用户提交目标后，UI 先显示一个轻量的 dispatch summary：

```text
目标
工作区
角色
Sandbox
预计动作类型
是否需要审批
当前预算摘要
```

低风险本地请求可以直接进入 Run；多文件、外部副作用或跨部门任务应先显示 Workflow/WorkPacket summary。

不要在每个普通只读请求前打断用户；需要确认的是实际风险，不是所有模型思考。

### 5.3 Tool/Capability Approval

Approval 卡片必须包含：

```text
为什么需要
哪个 Run/Workflow 触发
哪个角色请求
能力和版本
目标资源
精确 payload 或 diff
风险等级
预算/费用影响
有效期
批准后会发生什么
拒绝后会发生什么
```

按钮文案应按风险变化：

```text
拒绝
批准这一次
批准此项目内同类请求（仅当 policy 明确允许）
```

不提供默认的“批准所有未来请求”。批准结果必须显示已消费、拒绝、过期或取消，不把按钮点击直接渲染成成功。

### 5.4 取消

取消是有状态的交互，不是立即把卡片从屏幕删除：

```text
点击取消
  → UI 显示 cancelling
  → 禁止新的 dispatch
  → 等待已有 handler/子进程收敛
  → 收到 cancelled 或 result_unknown
  → 显示实际停止证据/未确认部分
```

如果结果未知，UI 必须显示：

> 请求已停止继续推进，但系统无法确认副作用是否已经发生。请进入对账，不要直接重试。

### 5.5 Review、Acceptance 和返工

Review 页面分成三栏：

```text
左：冻结的 acceptance criteria
中：Diff / Artifact / Test evidence
右：Reviewer decision / reasons / next action
```

Reviewer 只能：

```text
通过
需要修改
拒绝
请求更多证据
```

不能在 Review 页面修改 Builder 的原始事实，也不能通过编辑 criteria 来让结果通过。

Acceptance 页面必须显示：

- 谁是决策人；
- 依据哪个 criteria snapshot；
- 哪些证据满足或不满足；
- 接受、拒绝、豁免或返工的后果；
- Delivery 是否已确认。

### 5.6 Workflow 观察

Workflow 默认显示列表和阶段，而不是复杂动画：

```text
Workflow status
  ├── completed nodes
  ├── active node
  ├── waiting/blocked node
  ├── retry count
  ├── compensation status
  └── next deterministic transition
```

只有在用户明确进入详情时才显示 DAG 图。图上的每个节点必须可以打开 NodeExecution、Run、Artifact 和 Receipt。

### 5.7 Swarm 观察

Swarm UI 不应该营造“很多 Agent 很热闹”的错觉，而应显示：

```text
Parent plan
Partition strategy
Child count / max count
Active / queued / failed / unknown children
Budget consumed
Path locks
Duplicate fingerprint suppression
Merge readiness
```

子 Cell 页面显示 typed result 和 evidence，不默认展示全量内部对话。用户应该能选择：

- 打开某个 child Run；
- 取消单个 child；
- 取消整个 parent plan；
- 查看合并冲突；
- 请求返工；
- 退休并释放资源。

### 5.8 Memory 和 Context 观察

Memory 页面分成：

```text
可见记忆
待审核记忆
来源和 provenance
作用域/ACL
过期时间
被哪些 Run 使用
```

Context Inspector 显示一次请求的摘要：

```text
system/policy sections
role prompt version
tool catalog version
project snapshot
memory hits and tokens
history/checkpoint
compaction
cache telemetry
```

默认不显示 Secret、完整隐藏 reasoning 或未经脱敏的 Provider payload。用户可以删除、撤销或纠正 Memory，但不能通过 UI 修改 EventLog 原始事实。

## 6. CLI/TTY 交互规范

### 6.1 当前产品基线

当前 Workbench 的真实交互是：

```text
transcript + input + status
Esc/Ctrl-C cancel
/trust
/sandbox
/receipt
/cancel
/quit
```

见 `kiana-entrypoints/src/workbench.rs:16-26` 和 `kiana-entrypoints/src/workbench_chat.rs:33-121`。

当前状态栏已经包含：

```text
folder / trusted / sandbox / running|idle / session
```

见 `kiana-entrypoints/src/workbench_chat.rs:92-100`。

这套状态栏应保持稳定，未来只能增加不破坏现有含义的字段。

### 6.2 推荐 TTY 布局

```text
┌─────────────────────────────────────────────┐
│ kiana  project  trusted  sandbox  status     │
├─────────────────────────────────────────────┤
│                                             │
│ transcript / action cards / changed files   │
│                                             │
├─────────────────────────────────────────────┤
│ You>                                       │
├─────────────────────────────────────────────┤
│ hint: Esc cancel · /receipt · /help         │
└─────────────────────────────────────────────┘
```

窄终端必须退化为：

```text
status line
transcript
input prompt
```

不能因为终端太窄而隐藏正在等待的 Approval、Unknown 或 cancellation state。

### 6.3 Slash command 分层

```text
导航：/help /quit /clear
工作区：/trust /sandbox
运行：/status /cancel /pause /resume
证据：/receipt /diff /tests /events
上下文：/context /memory
平台：/tools /mcp /workflow /swarm
```

只有后端真实支持的命令才能显示为可用。目标命令可以在 help 中标记为 planned，但不能执行后静默失败。

### 6.4 TTY 错误显示

错误必须包括：

```text
发生了什么
影响哪个对象
是否已经产生副作用
可以重试吗
需要用户做什么
可查看的 receipt/incident 引用
```

示例：

```text
✗ approval_expired
Run abc123 的请求没有执行；审批已过期。
没有报告外部副作用。请重新提交请求，不要复用旧 approval。
```

## 7. Web 交互规范

### 7.1 当前边界

当前 Web 是 loopback-only Workbench，使用同一 `DaemonHost`，并且不声称 token streaming。所有 UI 状态应从 run/event/receipt 协议读取，而不是从浏览器自己的 active session 推导。

### 7.2 事件同步

```text
connect
  → authenticate local instance
  → subscribe with session/run id and cursor
  → load snapshot
  → merge events after cursor
  → render state
  → require terminal event/receipt
```

断线时显示：

```text
连接已断开
最后已确认事件：seq/cursor
正在尝试恢复观察
```

不能显示“完成”直到收到 terminal event 或重新从 State/Receipt 读取到 terminal state。

### 7.3 多标签页和并发

- 每个标签页必须有显式 session/run 标识；
- 不使用全局 `active_session`；
- 一个 Session 的并发 turn 要显示 queued/rejected 语义；
- 旧标签页的响应必须被 epoch/run ID 丢弃；
- 一次 Approval 只能被一个合法 owner 消费；
- UI 乐观更新必须可被服务端事实回滚。

### 7.4 无障碍

必须支持：

- 键盘完成所有操作；
- focus trap 用于 Approval 和危险操作；
- 状态通过文字和 aria-live 宣布，不只依赖颜色；
- Diff、Timeline、Workflow table 在窄屏可滚动；
- 不闪烁、不依赖动画表达运行状态；
- 错误、Unknown 和需要人工动作有可读标题；
- 用户可以复制 run/event/receipt ID；
- 高对比度和减少动态效果设置。

## 8. Desktop 壳交互规范

Desktop 是本地承载层，不是第二个控制面：

```text
Desktop shell
  → launch / own / observe Web + DaemonHost
  → system tray / close policy
  → workspace picker
  → crash/restart notification
```

当前 welcome 页面已实现：

- Continue last workspace；
- Open existing folder；
- Create scratch project；
- trust 解释；
- close/keep background 解释。

未来补充：

- DaemonHost health；
- 当前 Run 和 pending HumanTask；
- crash/reconnect 状态；
- local token/instance ownership 状态；
- 打开 Receipt、Incident 和日志目录。

不做：

- Desktop 私有 Agent loop；
- Desktop 私有权限；
- 隐式远程同步；
- 在托盘中悄悄继续外部副作用。

## 9. 参考项目与 UI 取舍

| 参考项目 | UI/交互可吸收设计 | Kiana 调整 |
|---|---|---|
| Codex | Thread/Turn 结构、可恢复对话、interrupt/suspend | 统一映射为 Session/Turn/Run，加入 Receipt/Project |
| OpenCode | TUI 状态同步、event projector、hydration | UI 只投影 Event/State，不维护第二事实源 |
| Cline | Local Runtime Host、IDE/CLI 共用运行时、Approval UX | Approval 由 ControlPlane 提供，不由 UI 自己判定 |
| Crush | queued/active/terminal Run、cancel 状态和 terminal event | `cancelling`/`result_unknown` 必须可见 |
| Roo Code | UI timeline 与 model history 分离、写前 checkpoint、悬空 tool result 恢复 | 将 Artifact/Evidence/Invocation 做成独立可展开对象 |
| Pi | 极简聊天、append-only history、fork/resume | 借鉴轻量交互，不用 transcript 代替事件状态 |
| Aider | repo map、diff、代码工作区交互 | 显示 snapshot/base revision，避免无来源 diff |
| OpenHands | 事件桥、任务面板、工作区上下文 | 当前只做 loopback local_behavior，不宣称远程运行 |
| Agent Framework / Agno | Approval/requirement 卡、暂停后恢复 | requirement 必须持久化并绑定 owner/Invocation |
| ChatDev / MetaGPT | 阶段和角色可视化 | 显示 typed stage/artifact，不展示无界群聊噪音 |
| Archon | 工作流节点、人工 Gate、worktree 隔离 | DAG 详情必须映射到 Kiana Workflow/Event/Artifact |

明确不吸收：

- 用聊天记录替代状态；
- 用漂亮的 Agent graph 替代真实调度；
- 自动滚动掩盖审批和错误；
- 让一键“自动完成”绕过 Risk/Approval；
- 多入口维护不同 Session/Run 语义；
- 把 token streaming、远程执行或 IDE parity 写成当前完成能力。

## 10. UI 状态协议

UI 需要一个稳定的读取模型：

```text
UiSnapshot {
  instance_id
  organization_id?
  project_id?
  session_id
  run_id?
  workflow_instance_id?
  current_state
  pending_actions[]
  active_invocation?
  blockers[]
  changed_artifacts[]
  evidence_summary
  budget_summary
  last_event_cursor
  epoch
  generated_at
}
```

`UiSnapshot` 是投影，不是新的事实聚合。用户操作通过命令发送：

```text
UiAction {
  action_id
  actor/principal
  target_type
  target_id
  expected_epoch
  command
  payload
  idempotency_key
}
```

服务端返回：

```text
ActionAccepted
ActionRejected
ActionRequiresApproval
ActionConflict
StateChanged
TerminalReceiptAvailable
```

乐观 UI 必须以 `expected_epoch` 或等价版本为条件；版本冲突时重新 hydrate，不得覆盖更新的状态。

## 11. 交互错误与恢复

UI 错误分类：

```text
InputError          用户输入无效
PolicyDenied        不允许执行
ApprovalRequired    等待人类决定
Conflict             状态/版本冲突
Capacity             排队或超限
Unavailable          Provider/MCP/Store 不可用
Cancelled            已取消
Failed               明确失败
Unknown              结果无法确认
Persistence          事实未能保存
```

每个错误都必须有：

```text
human-readable title
technical code
object reference
副作用确认状态
retry/reconcile guidance
```

恢复按钮与错误类型绑定：

| 错误 | 允许操作 |
|---|---|
| InputError | 修改并重新提交 |
| PolicyDenied | 查看原因、修改范围或请求授权 |
| ApprovalRequired | 批准/拒绝/等待 |
| Conflict | 刷新状态、重新应用意图 |
| Capacity | 等待、取消或降低范围 |
| Unavailable | 重试或切换已授权 Provider |
| Cancelled | 查看部分结果、重新规划 |
| Failed | 查看证据、创建新 Attempt |
| Unknown | 对账/人工确认，禁止盲重试 |
| Persistence | 保存诊断、停止后续副作用 |

## 12. 隐私和安全 UX

- Trust 状态必须始终可见；
- Sandbox 和角色必须可见且不能由模型隐藏；
- Secret 永不进入可复制的 UI payload；
- Memory 结果显示 collection、purpose 和 provenance；
- MCP server 显示来源、版本、hash、权限和健康状态；
- 外部副作用显示账户、目标、金额/内容和最终 payload；
- 复制诊断信息时默认脱敏；
- 审计视图区分“系统记录了什么”和“现实世界是否已确认”；
- 不用深色/红色视觉制造压力来促使用户批准；
- 键盘快捷键不能绕过高风险确认。

## 13. 实施顺序

### P0：当前 Workbench 稳定性

- 固定 status line、transcript、input 和 receipt 入口；
- 统一 running/idle/blocked/failed/cancelled 文案；
- 正确处理 cancel-after-awaiting-approval；
- 错误显示对象、影响和下一步；
- CLI/Web 使用显式 session/run 目标。

### P1：可操作的 Run 和 Human Inbox

- Run detail projection；
- Approval/Review/Acceptance/Incident action cards；
- cursor/epoch hydration；
- changed files、tests、Receipt 和 evidence panels；
- reconnect、stale response 和 optimistic conflict。

### P2：项目、Workflow 和 Memory 观察

- Project/Objective 页面；
- Workflow node timeline 和 DAG detail；
- Memory/Context inspector；
- Tool/MCP catalog；
- Cost、quota、cache 和 retrieval telemetry。

### P3：Swarm 和生态体验

- parent/child Cell overview；
- partition、budget、merge conflict 和 retirement；
- Skill/Plugin/Workflow Pack management；
- Scheduler/Trigger inbox；
- local extension trust and rollback。

### P4：Desktop/IDE/Streaming 扩展

- Desktop health/reconnect；
- provider-normalized streaming；
- richer IDE integration；
- external Connector approval and reconciliation UI。

## 14. UI 完成定义

达到 `proven_local` 前，至少满足：

1. CLI/TTY、Web 和 Desktop 对同一个 Run 显示一致的 terminal state；
2. Approval、Cancel、Unknown、Failure 和 Reconnect 都能解释下一步；
3. UI 操作带 target ID、owner、expected version 和 idempotency；
4. Receipt、Artifact、Evidence、Review 和 Acceptance 可从 Run 互相定位；
5. UI 不显示服务端尚未证明的 streaming、远程、外部或现实世界成功；
6. Secret、隐藏 reasoning 和未授权 Memory 不进入可见或可复制内容；
7. 窄屏、键盘、屏幕阅读器和高对比度使用不丢失安全状态；
8. UI regression 使用 fake model/cassette 和状态协议，不依赖时间或真实 Provider。

## 15. 当前诚实描述

> **Kiana 已有基于 `DaemonHost` 的 Folder Workbench、TTY transcript/input/status、trust/sandbox 切换、cancel 和 receipt 入口，以及承载 loopback Web 的 Desktop welcome shell；统一的 Run detail、Human Inbox、Project/Workflow/Swarm 观察、Memory/Context inspector、cursor hydration、Streaming UI 和多项目运营工作台仍是目标设计。**
