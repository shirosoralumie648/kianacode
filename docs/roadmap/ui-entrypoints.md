# Roadmap 专项：UI / Entrypoints 专项

> 返回 [Kiana 执行路线图](../roadmap.md) 的总图与当前窗口。本文保留原专项编号、状态、依赖、验收口径和证据限制；专项步骤完成不会自动改变 P 阶段状态。

## 26. UI / Entrypoints 专项：用户入口实际设计与处理流程（2026-09-12 追加）

> 本节对应 [module-map.md](../module-map.md) 第 9 模块，执行卡为 `UI-00`–`UI-41`。它是面向实施 agent 的设计合同，不是完成声明；所有 UI 卡初始状态均为 `⏳`。源码中已经存在的 `UiSnapshot`、SSE、Workbench 和 Electron WIP 必须重新按本节验收，不能因为有类型、按钮或历史测试就提升 feature status / proof level。
>
> 本专项按用户指示处理旧 roadmap 的限制性文字：如果旧限制与本节的入口重构、拆分 `cli.rs`/Web、增加本地 transport 或 IDE adapter 冲突，以本节为准。该约定只改变设计空间，不授权提交、发布、真实外部操作，也不改变 `CURRENT_STATUS.md` 的事实权威。

### 26.1 目标、边界和设计判断

用户入口的完整链路是：**选择工作区和身份 → 获取能力/权限快照 → 提交命令 → 观察同一事实投影 → 处理人工待办 → 检查文件和 Receipt → 明确取消、恢复或收尾**。CLI、终端 Workbench、loopback Web、Electron 和后续 IDE adapter 都是同一 `DaemonHost → ControlPlane` 的客户端，不得另起模型循环、权限判断或副作用执行器。

本次调研的详细来源、reference 全量盘点和源码缺口见 [ui-entrypoints-design-research.md](../ui-entrypoints-design-research.md)。重点吸收 Codex 的 Turn/Item 投影和输入状态机、OpenCode 的 bootstrap/reducer/有界淘汰、OpenHands 的事件去重和连接隔离、Cline/Continue 的 typed bridge、Roo 的服务端 Diff、Goose/Emdash 的 session 生命周期、Pi 的跨 chunk 输入解析、Crush 的有界结果渲染、Herdr/Orca 的布局与 PTY 恢复；这些项目只提供机制对照，不能替代 Kiana 的授权或持久事实。

五条不可变判断：

1. **Daemon 生成读模型。** UI 只保存草稿、连接状态和可丢弃的游标；Session/Run/Turn/Invocation/Approval/Artifact/Receipt 的状态从服务端快照和事件投影读取。
2. **四类水位分开。** EventLog 的事实 cursor、UI feed sequence、aggregate/object revision、ControlPlane authority epoch 各司其职；token 更新不能无故使审批过期，重启 epoch 也不能被普通 reconnect 忽略。
3. **接收与完成分开。** 每个有副作用的命令有稳定 `command_id`/`idempotency_key`/payload digest；响应丢失时查询原命令，不能换 ID 重投。
4. **三种状态分开。** `run_status`、`connection_status`、`submission_status` 分别表示业务、连接和本地提交；断线不等于 Run Failed，关窗不等于 Cancelled，未知工具结果必须显式显示。
5. **界面只展示可执行动作。** 动作卡由服务端能力和前置条件生成，客户端只能提出请求；ControlPlane 在提交和执行两个边界再次核验 owner、scope、revision、epoch、approval 和 permit。

### 26.2 目标分层与唯一执行脊柱

```text
CLI / Workbench / Web / Electron / IDE adapter
                    │ versioned command + query + feed client
                    ▼
                DaemonHost (唯一组合根)
                    │ local transport / in-process adapter
                    ▼
              ControlPlane + EventStore
                    │ authorize → transition batch → project
        ┌───────────┼────────────┬──────────────┐
        ▼           ▼            ▼              ▼
   snapshot      feed       action result    receipt/artifact
        │           │            │              │
  shared reducer  cursor     ack/unknown    immutable refs
        └───────────┴────────────┴──────────────┘
                    ▼
       surface-specific presenter and input state
```

分层职责如下：

| 层 | 责任 | 禁止事项 |
|---|---|---|
| `kiana-protocol` | versioned query/feed/action/receipt DTO、错误码、能力协商 | 不把 HTML 字段或 TUI 文本当稳定协议；不隐藏未知字段 |
| `kiana-client` | 请求关联、超时、游标、重连、动作提交和响应丢失查询 | 不执行 shell/MCP/文件写入；不自行决定 retry 是否安全 |
| `kiana-daemon` | 本地身份、session ownership、snapshot projector、feed 和 action facade | 不让入口绕过 ControlPlane；不把内存 bus 当 durable idempotency |
| `kiana-entrypoints` | CLI/Workbench/Web 路由、presenter、输入和可访问性 | 不复制状态机、策略表或第二个模型 loop |
| `contrib/desktop` | 进程发现、窗口、窄 IPC、worker 生命周期和安全边界 | renderer 不 spawn；不通过 URL/IPC 隐式 trust、resume 或 approve |
| IDE adapter | ACP/编辑器消息映射、session attach、permission、cancel | 不把编辑器 host 权限当 Kiana permit；不创建独立 run store |

### 26.3 统一合同

UI 合同分成四个可独立演进的版本：

```text
UiSnapshot.schema_version       // 一次可渲染的原子读模型
UiFeed.schema_version           // 增量事件与 gap/replay 语义
UiAction.schema_version         // 人工命令与 CAS/idempotency
UiCapability.schema_version     // 当前身份和 surface 可用功能
```

推荐的最小形状（真实字段落在 `kiana-protocol`，这里是设计约束）：

```text
UiSnapshot {
  schema, instance_id, authority_epoch, snapshot_cursor,
  generated_at, principal, workspace, capabilities,
  sessions: [SessionSummary], active_session_id?,
  runs: [RunSummary], pending_actions: [HumanActionCard],
  artifacts: [ArtifactSummary], notices: [UiNotice],
  next_page?, limitations: [EvidenceLimitation]
}

UiFeedEnvelope {
  schema, instance_id, authority_epoch, feed_sequence,
  snapshot_cursor, event_id, aggregate, object_revision, event
}

UiAction {
  schema, command_id, idempotency_key, target_id,
  expected_epoch, expected_cursor, expected_revision?,
  payload, payload_digest, submitted_by, deadline?
}

UiActionResult {
  command_id, disposition: Accepted|Applied|Rejected|Unknown,
  resulting_cursor?, resulting_revision?, receipt_ref?,
  error?: StableError, retry: QueryOriginal|SafeRetry|DoNotRetry
}
```

`UiSnapshot` 必须来自同一事实切点；feed 先发 snapshot boundary 再发事件。客户端 reducer 只接受相同 `instance_id`、兼容 schema、连续 sequence 或明确 replay/gap 结果。`Unknown` 不是 `Rejected`，也不能被 presenter 画成成功。所有服务端给出的 Diff、Usage、ToolCall、approval 和 receipt 都携带稳定 item/artifact ID、revision、digest、MIME/大小和 provenance；前端折叠、排序或分页不改变审批载荷。

### 26.4 端到端处理流程

#### 26.4.1 打开、快照、恢复 feed

1. 入口解析 workspace/session 参数，建立本地 transport；仅验证来源和 token，不以 URL 或缓存状态授予 trust。
2. 客户端 `initialize`，协商 schema/capabilities/instance identity；服务端返回当前 authority epoch 和最小错误信息。
3. 客户端请求 snapshot（可带 `session_id`、分页 cursor 和已知 feed cursor），服务端从 EventStore/projector 生成原子读模型。
4. 客户端安装 feed listener **后**发送 resume/replay 请求；这保证重放事件不会跑到 listener 之前。旧 epoch 或不可重放 cursor 返回 gap，客户端丢弃本地增量并重新 hydrate。
5. reducer 按 `event_id`/sequence/revision 去重，标记 `connection_status=Degraded` 直到 snapshot/feed 边界一致；待办、未知结果和限制不得被缓存淘汰。

#### 26.4.2 提交、响应丢失和重投

1. presenter 根据 snapshot/capabilities 生成输入约束和 `UiAction`，本地显示 `submission_status=Preparing`。
2. client 发送稳定 `command_id`、idempotency key、payload digest 和 expected epoch/cursor/revision；入口不直接调用 runner/broker。
3. ControlPlane 原子验证 owner、scope、policy、approval、budget、lease、CAS 和撤销状态，再追加事实或返回结构化拒绝。
4. `Accepted` 只表示命令已入账；`Applied` 需有 resulting cursor/receipt。HTTP/IPC 连接在此时断开则标为 `Unknown`，保留原 command ID。
5. 客户端先 query original command；服务端返回已应用结果、拒绝或仍未知。只有明确 `SafeRetry` 才能原 ID 重投；新 ID 只用于用户明确的新意图。

#### 26.4.3 人工待办、文件审阅和 Receipt

1. `HumanActionCard` 包含 action kind、target、reason、scope、expires_at、required fields、display refs、expected revision 和 allowed decisions。
2. UI 先展示服务端摘要和可验证 Diff；正文/大输出按 Artifact ref 分页获取并检查 digest，不能用客户端重新 diff 代替。
3. 提交 approve/deny/edit/restore/continue 时携带原卡的 expected values；过期、撤销、revision mismatch 必须回显新的 snapshot 和冲突位置。
4. 业务完成后从 EventLog + Artifact refs 生成 Receipt；Receipt 页面同时显示 `Unknown`、未验证项、成本、文件版本和 limitations。

#### 26.4.4 Cancel、重启和恢复

1. 用户取消先创建稳定 cancel command；UI 立即显示 `Cancelling`，不把本地按键当作已经停止。
2. ControlPlane 追加 cancel intent，runner/broker 处理 stop permit；每个 Invocation 产生 stop/effect 证据。
3. 服务端终态为 Cancelled、Completed、Failed 或 ResultUnknown；连接断开或 worker 被杀只产生 recovery notice。
4. Daemon 重启后重建 projector、fence 旧 lease/epoch、恢复 pending/unknown 索引；默认展示 `Paused/Needs review`，不自动 resume/approve/retry。
5. 只有显式 continue/resume 命令并通过新一轮 authority/approval 检查，才创建后续 invocation。

### 26.5 各入口的产品处理

| 入口 | 首屏和主交互 | 必须证明 |
|---|---|---|
| CLI | `run/status/events/approve/cancel/resume/receipt`；TTY、JSON、静默管道三种 presenter | 稳定 exit code、机器可读错误、无 TTY 时不打印装饰；断线可用 command query 收敛 |
| Workbench | 会话列表、聊天/事件时间线、人工 inbox、Diff/Receipt、命令面板 | 键盘编辑/粘贴/取消/resize 不丢字节；审批和未知结果可追踪；不阻塞 daemon |
| Web | snapshot hydrate、SSE feed、分页历史、审批卡、artifact viewer、状态/设置 | loopback 来源/token、tab 隔离、Last-Event-ID/gap、XSS/CSP、焦点和读屏、断线恢复 |
| Electron | 安全壳承载 Web、workspace attach/new/continue、托盘/通知 | IPC sender/origin、导航/新窗 allowlist、worker process group、关闭不隐式 resume/trust |
| IDE/ACP | initialize/session/new/resume/prompt/update/permission/cancel 映射 | 版本协商、session update 顺序、permission 过期、cancel 与 Kiana 终态对齐；host 编辑能力仍过 ControlPlane |

禁止把上述入口差异实现成不同业务语义：颜色、快捷键和 JSON 字段可以不同，命令 identity、权限结果、cursor、revision、Receipt 和限制必须相同。

### 26.6 非目标和风险边界

- SSE/WebSocket/IPC 只是传输；它们不提供事实落盘、命令幂等或 effect exactly-once。持久性必须由 EventStore/ControlPlane 证明。
- React/TypeScript/Vite、桌面壳或 ACP 是可替换 adapter 选择，不是当前仓库已经采用的事实；迁移必须先保留可运行的 CLI/Workbench。
- 浏览器缓存、Electron store、TUI 内存和 transcript 都是展示缓存；cache miss、schema/epoch 不符或 digest 错误必须回源并可见。
- 参考项目的自动重试、宿主 shell、宽松 session fallback、盲目工具重放和“测试字符串存在”不作为 Kiana 验收标准。

## 27. UI / Entrypoints 详细实施 steps（UI-00–UI-41）

每张卡都以“先拒绝、再成功”为顺序，依赖项未满足时只能产出 fixture 或 RED 测试，不能临时绕过 `DaemonHost`。实现中可以拆分文件、调整框架和增加 adapter，但不得改变 §26 的命令 identity、事实来源和 proof-level 规则。

### 27.1 基线和协议地基

<a id="step-ui-00"></a>



#### UI-00 · 建立入口基线与验收矩阵　✅

- 依赖：无。代码/文档：`docs/module-map.md`、`CURRENT_STATUS.md`、`kiana-entrypoints` 与 `contrib/desktop` 测试清单。
- 步骤：从新的 source snapshot 开始，记录 CLI/Workbench/Web/Desktop 的启动、session attach、run、approval、cancel、resume、receipt、export 路径；把既有测试按 deny/happy/reconnect/recovery/physical 分类，标出 baseline failure。
- 先拒绝：未授权 workspace、错误 token/origin、未知命令、过期 cursor/revision、非 owner action、响应丢失和旧 epoch 都要有明确预期。
- 成功/回归：画出每个入口到 `DaemonHost` 的调用图；现有通过测试逐项绑定到合同，未覆盖处生成 RED 测试名，不把 `0 tests` 记为通过。
- 完成产物：`UI-00` 基线表、覆盖矩阵、稳定快照 hash、并行 agent 交接说明；不修改 feature status。

<a id="step-ui-01"></a>



#### UI-01 · 定义 versioned UI protocol DTO　✅

- 依赖：UI-00。当前 source slice 与 CI-only 证据见 [`ui-protocol-baseline.md`](ui-protocol-baseline.md)。代码：`kiana-protocol` 新增 snapshot/feed/action/capability/error/artifact DTO，保留旧 wire envelope 的兼容解码。
- 步骤：定义 schema/version、instance/epoch/cursor/revision、Session/Run/Turn/Item、HumanActionCard、ReceiptRef、UiNotice 和 limitations；未知字段可保留或安全丢弃并报告版本。
- 先拒绝：缺字段、重复 ID、非法 sequence、跨 instance、过大 payload、无 digest 的文件审批和伪造 actor/scope 必须结构化拒绝。
- 成功/回归：对 JSON/MessagePack（若使用）做 round-trip、旧 envelope 兼容、schema mismatch 和大小边界测试；生成 protocol fixture。
- 完成产物：协议类型、schema 文档/fixture、稳定错误码表及迁移说明。

<a id="step-ui-02"></a>



#### UI-02 · 统一错误、能力和 surface handshake　✅

- 依赖：UI-01。当前 source slice 与 CI-only 证据见 [`ui-handshake-baseline.md`](ui-handshake-baseline.md)。代码：`kiana-protocol`/`kiana-client` 的 `StableError`、`UiCapability`、initialize/health handshake。
- 步骤：将 Input/PolicyDenied/ApprovalRequired/Conflict/Capacity/Unavailable/Cancelled/Failed/Unknown/Persistence 映射到机器码、用户文案、retry disposition 和 remediation；能力按 principal、workspace、surface、feature version 求交集。
- 先拒绝：未知错误码、能力越权、把 `Unknown`/`PolicyDenied` 映射成成功或可自动重试；health 不泄漏路径/token/内部异常。
- 成功/回归：每个 surface 生成相同 deny matrix；能力缺失时按钮/CLI action 不出现，直接构造请求仍被服务端拒绝。
- 完成产物：错误目录、能力 handshake fixture、文案与 exit-code 映射表。

<a id="step-ui-03"></a>



#### UI-03 · 本地实例身份、发现和 transport　✅

- 依赖：UI-01/02。当前 source slice 与 CI-only 证据见 [`ui-instance-baseline.md`](ui-instance-baseline.md)。代码：`kiana-daemon`、`kiana-client` 的 in-process/Unix socket/named pipe adapter 与 instance lock。
- 步骤：为 daemon 生成 instance ID、authority epoch、socket/pipe 权限和 ready record；CLI/Workbench 优先复用已有实例，安全地启动/连接并探测协议版本。
- 先拒绝：旧 socket、PID 重用、不同 workspace、未知 peer、权限过宽的 socket、重复实例和 epoch 不一致必须失败关闭。
- 成功/回归：启动、attach、daemon 重启、并发客户端、半开连接、协议降级和清理锁文件测试；transport 只传请求，不执行副作用。
- 完成产物：发现记录格式、transport contract、instance/epoch fixture 和诊断命令。

<a id="step-ui-04"></a>



#### UI-04 · 动作 CAS、idempotency 与响应丢失　✅

- 依赖：UI-01–03；串接 Event/Receipt §23。当前 source slice 与 CI-only 证据见 [`ui04-action-cas-baseline.md`](ui04-action-cas-baseline.md)。代码：ControlPlane action facade、command journal、payload digest。
- 步骤：为每个 action 绑定 command ID、idempotency key、target、expected epoch/cursor/revision、owner/scope、deadline；持久化 Accepted/Applied/Rejected/Unknown 及原始 digest；Applied 必须绑定合法 receipt digest，其他状态拒绝 receipt。
- 先拒绝：重复 key 不得重复 effect；digest 不同、owner 不同、revision 过期、取消中、旧 epoch、超时和无 permit 必须拒绝或标 Unknown。
- 成功/回归：模拟 ACK 丢失、进程崩溃、重复请求、并发 CAS 和新旧 payload；query original 能收敛到同一结果，禁止换 ID 自动重做。
- 完成产物：action journal/adapter、重投策略、effect-count fixture 和 durable proof 记录。源码与 GitHub-only 验证已接入；当前证明等级为 `source`，CI 结果未等待。

<a id="step-ui-05"></a>



#### UI-05 · 原子 snapshot projector 与分页　✅

- 依赖：UI-01–04、Event/Receipt §23。代码：`kiana-daemon` projector/query facade。
- 步骤：从 EventStore 单一 cursor 生成 `UiSnapshot`；为 session/run/action/artifact/receipt 提供稳定排序、分页 cursor、retention 和 limitations；保存 projector health/lag。
- 先拒绝：非法 page cursor、越权 session、被 retention 保护的待办被淘汰、projector lag 被伪装成空列表、跨 cursor 拼接的 snapshot。
- 成功/回归：新进程重建、重复读取、分页边界、空集合、projector 故障和恢复测试；snapshot cursor 可回放且不改变事实。
- 完成产物：snapshot contract、projector tests、分页/保留策略和 read-model rebuild 命令。

已接入 `kiana-domain/src/ui_snapshot.rs` 的原子 projector、`kiana-daemon` 的三条只读 facade
（`ui_snapshot_page`、`ui_snapshot_projector_health`、`rebuild_ui_snapshot_projection`）与完整分页/保留/
限制合同：`UiSnapshotPage.limitations` 由本次切片边界产生（`ui_snapshot_page_partial`、
`ui_snapshot_retention_floor_applied`）；`UiSnapshotProjectorStatus`/`UiSnapshotProjectorHealth` 显式区分
lag 与未知进度并给出 `lag` 数值，绝不把投影落后渲染成空列表；`UiSnapshotRebuild` 是 read-model rebuild
命令，页与 health 来自同一 `persisted_events()` 读取且 health 必须 `UpToDate`。domain 8 条、真实
`DaemonHost` 5 条与 core 两条守卫夹具覆盖空集合、重复读取/游标回放、分页与游标边界、health 三态、
故障恢复和「新宿主实例重开同一账本」重建同摘要；手动 lane 为
[`.github/workflows/ui05-snapshot-projector.yml`](../../.github/workflows/ui05-snapshot-projector.yml)，
自动 lane 仍是统一 `ci.yml` 的 domain/core 分片与 `kiana-daemon` 整 crate 分片。源码与 GitHub-only
验证已接入；当前证明等级为 `source`，CI 结果未等待。

<a id="step-ui-06"></a>



#### UI-06 · feed cursor、gap、replay 与背压　✅

- 依赖：UI-05。代码：daemon feed facade，复用 `RunStreamEnvelope` 但补齐 instance/epoch/schema/gap 语义。
- 步骤：定义 snapshot boundary、feed sequence、Last-Event-ID/after cursor、replay window、heartbeat、慢消费者上限；区分 duplicate、gap、old epoch、terminal、unknown。
- 先拒绝：序列跳跃、错误 epoch、过期 replay、无界队列、慢客户端阻塞事实提交、terminal 后伪造 delta。
- 成功/回归：断线重连、重复事件、跨页、慢消费者、服务器重启、terminal retention 和 gap→rehydrate；gap 后 boundary 必须使用服务端当前 cursor，sequence 由服务端决定，不能按 timestamp 排序。
- 完成产物：feed state machine、fault fixture、背压指标和 [replay 基线](ui06-feed-replay-baseline.md)。完成切片补齐：`UiFeedGapReason::TerminalRetention`
  由真实淘汰路径产生（instance/epoch 已匹配的 cursor 领先一个 `current_sequence == 0` 的已被有界表淘汰 channel 时报保留期丢失，
  `sequence_ahead` 只在服务端确实持有更高位置时报；两者都 `snapshot_required` 且后续 boundary 一律用服务端当前 cursor）；
  `NotificationStreamBridge::ingest` 区分 `Replayed`（已看过位置不重复投递）、`Terminal`（回放的 terminal 仍是终态事实）与新增的
  `NotificationStreamDisposition::Unknown`（`UiFeedFrameKind::Unknown` 的 cursor 前进但 `snapshot_required`，不当 delta 渲染）；
  新增 `kiana-daemon/tests/ui06_feed_replay.rs` 5 条真实 `KianaHarness` run + `DaemonHost` 夹具
  （`ui06_feed_fault_plan_binds_the_bounded_budget_and_the_deny_matrix`、
  `live_feed_keeps_the_boundary_the_sequence_and_the_retained_terminal`、
  `gap_frames_use_the_server_cursor_and_hydrate_through_the_snapshot_page`、
  `a_stalled_consumer_gaps_but_never_blocks_committed_facts`、
  `restarted_host_requires_hydration_and_keeps_the_same_ledger`）与声明式 fault plan
  [`kiana-daemon/tests/fixtures/ui06-feed-replay.json`](../../kiana-daemon/tests/fixtures/ui06-feed-replay.json)（256/128 预算 + 七类 gap 原因 + required/denied）；
  `run_stream.rs` 新增 `resumed_feed_replays_nothing_when_the_cursor_is_already_current`、
  `retired_feed_window_reports_terminal_retention_and_requires_snapshot`、
  `stalled_consumer_keeps_the_queue_and_history_bounded`，`nm13_notification_stream.rs` 新增
  `terminal_retention_and_unknown_frames_never_collapse_into_a_delta`；core 守卫新增
  `ui06_feed_fault_matrix_stays_wired_to_the_lane_and_the_auto_shards` 绑定夹具名、预算常量、七类 gap 原因、lane 与分片。
  手动 lane 为 [`.github/workflows/ui06-feed-replay.yml`](../../.github/workflows/ui06-feed-replay.yml)，自动 lane 仍是统一 `ci.yml` 的
  core 分片与 `kiana-daemon`/`kiana-protocol` 整 crate 分片。源码与 GitHub-only 验证已接入；当前证明等级为 `source`，CI 结果未等待。

<a id="step-ui-07"></a>



#### UI-07 · typed client query/feed/action API　✅

- 依赖：UI-02–06。代码：`kiana-client` 拆出 QueryClient、FeedClient、ActionClient、ArtifactClient，统一 request ID/deadline/cancel。
- 步骤：提供 initialize、snapshot、history、command status、subscribe/resume、artifact page、submit/cancel/continue；listener 可释放并隔离迟到回调。
- 先拒绝：未初始化、跨 workspace target、未知 schema、重复 listener、错误 command 重投、取消后继续回调到旧 controller。
- 成功/回归：typed mock transport 覆盖 timeout、late response、reconnect、gap、Unknown query 和 listener cleanup；不把 HTTP 200 直接当 Applied。
- 完成产物：client traits、mock transport、请求生命周期图和调用迁移清单。完成切片补齐：`ListenerRecord` 现在同时绑定 listener
  generation 与创建它的 submission cancellation token（`dispatch`/`resume` 对已释放 generation 或已取消 token 一律
  `ListenerInactive`，callback 不再被调用）；11 个 operation 常量与三个扩展 schema 常量从 `kiana-client` 导出；
  `kiana-client/tests/ui07_typed_clients.rs` 换成脚本化 mock transport（记录每个 envelope、request ID 与 cancel hook），
  17 条先拒绝夹具覆盖未初始化/跨 workspace/未知 schema/重复 listener/取消与过期请求/迟到响应与 cancel hook/
  listener 代际退休/重连与服务端 cursor/gap 必须交还 presenter/Unknown 原 key 对账/Accepted 不升级 Applied/
  cancel 只发 command identity；声明式矩阵
  [`kiana-client/tests/fixtures/ui07-typed-clients.json`](../../kiana-client/tests/fixtures/ui07-typed-clients.json)
  固定 operation/facade/预算/required/denied；core 守卫新增
  `ui07_typed_client_lifecycle_stays_wired_to_the_lane_and_the_auto_shards` 绑定夹具名、预算常量、lane 与自动分片。
  手动 lane 为 [`.github/workflows/ui07-typed-client.yml`](../../.github/workflows/ui07-typed-client.yml)，自动 lane 仍是统一
  `ci.yml` 的 `kiana-client`/`kiana-daemon` 整 crate 分片与 `kiana-core-s6/6` 的 `ui07_typed_client_guard`。
  源码与 GitHub-only 验证已接入；当前证明等级为 `source`，CI 结果未等待。

<a id="step-ui-08"></a>



#### UI-08 · 共享 reducer/entity store　✅

- 依赖：UI-05–07。代码：`kiana-entrypoints` 或共享 adapter 的纯函数 reducer/store。
- 步骤：按 entity ID/revision 合并 snapshot/feed；分别存 run、connection、submission、draft、inbox、artifact viewer；固定 optimistic 状态只能短暂存在且可回滚。
- 先拒绝：旧 revision 覆盖新值、跨 session 串数据、event duplicate 产生两条 item、cache eviction 丢 pending/unknown、乐观成功掩盖服务端拒绝。
- 成功/回归：乱序/重复/gap/epoch reset、tab isolation、hydrate/dehydrate、内存上限和 reducer purity 测试。
- 完成产物：共享 store、状态图、fixture reducer trace；presenter 不再自建事实状态。

`kiana-client::UiEntityStore` 是纯函数 reducer + 有界 store（`kiana-client/src/ui_store.rs`），
`reduce` 不修改输入、`apply` 只提交 reducer 输出。完成切片（2026-10-17）修掉四类真实缺口并补齐
拒绝面：位置 0 的初始 boundary 变成合法起点（此前 daemon 的 live feed 第一帧会被当重复帧拒绝）；
缺 `entity_kind` 的 server 帧一律 `ui_feed_entity_kind_missing` 拒绝（不再默认猜成 run，raw
run-stream 帧因此进不了实体面）；draft 命名空间只属于本地输入层（feed → `ui_feed_draft_kind_forbidden`，
伪造快照 → `ui_store_snapshot_draft_entity`）；pending/unknown 实体遇到更高 revision 的权威 delta
不再静默保留本地值，而是 `SnapshotRequired` + `UiStoreGapReason::PendingConflict`（帧不记为已应用、
位置不回退）；新 authority epoch 的 boundary 丢弃旧权威实体、保留本地 pending 并要求 snapshot；
`Rejected` 结算不再删除自己从未 stage 过的权威实体；protocol snapshot 合并到 protected 实体时刷新
rollback baseline（回滚到服务端最新已知值）。`entities_of_kind`/`count_of_kind` 让 presenter 按命名空间
读共享 store 而不是自建副本。验收夹具：`kiana-client/tests/ui08_reducer_store.rs` 17 条先拒绝优先
（新增 10 条，含 `live_feed_starts_at_the_initial_boundary_and_markers_never_regress`、
`replayed_terminal_after_the_boundary_is_not_a_second_item`、
`epoch_reset_drops_authoritative_state_and_keeps_local_pending`、
`generic_feed_requires_an_explicit_kind_and_refuses_the_draft_namespace`、
`pending_conflict_never_lets_an_optimistic_value_hide_a_newer_revision`、
`snapshot_merge_refreshes_the_rollback_baseline_instead_of_a_stale_value`、
`hydrate_refuses_a_forged_draft_entity`、`a_stale_rejection_never_deletes_an_authoritative_entity`、
`kind_scoped_views_keep_the_six_namespaces_separate`、`ui08_reducer_plan_and_trace_replay_deterministically`），
声明式 plan + 31 步 reducer trace
[`kiana-client/tests/fixtures/ui08-reducer-store.json`](../../kiana-client/tests/fixtures/ui08-reducer-store.json)
（kind/lifecycle/gap reason/预算/夹具名/拒绝清单 + 两次重放同摘要）；
真实组合根夹具 `kiana-daemon/tests/ui08_entity_store.rs` 3 条（raw run-stream 帧必须被拒绝且零事实、
tab/cross-session 隔离、真实 gap 只围栏不伪装）；
`kiana-core/tests/ui08_reducer_store_guard.rs` 扩为 2 条守卫（marker/负断言 + plan/trace/夹具/lane/分片
绑定）；`kiana-entrypoints/tests/ui08_store_boundary_guard.rs` 拒绝入口再声明一套 store。
手动 lane 为 [`.github/workflows/ui08-reducer-store.yml`](../../.github/workflows/ui08-reducer-store.yml)，
自动 lane 仍是统一 `ci.yml` 的 `kiana-core-s6/6` 与 `kiana-client`/`kiana-daemon`/`kiana-entrypoints`
整 crate 分片。源码与 GitHub-only 验证已接入；当前证明等级为 `source`，CI 结果未等待。

<a id="step-ui-09"></a>



#### UI-09 · schema 资产、生成和兼容门　✅

- 依赖：UI-01/07/08。代码：schema/fixture 目录、生成脚本、版本检查 CI。
- 步骤：决定 JSON Schema/TypeScript/Rust 生成边界；为每个 DTO 固定 examples、unknown-field、max-size 和 deprecated 字段；生成静态客户端类型。
- 先拒绝：schema 与 Rust 类型漂移、同版本破坏性改动、生成文件未更新、示例包含 secret/真实路径。
- 成功/回归：schema diff、跨语言 round-trip、旧客户端读取新服务端、服务端拒绝新客户端不可理解命令。
- 完成产物：schema lock、生成校验命令、兼容矩阵和脱敏 fixtures。JSON Schema/Rust catalog 源码与 GitHub-only 验证已接入；当前证明等级为 `source`，CI 结果未等待。

`docs/schemas/ui/schema-lock.v1.json` 锁定七个 UI DTO 边界（handshake request/response、snapshot、
feed frame、action、action result、entity-store snapshot）及其 Rust owner、strict unknown-field
策略、byte 上限、脱敏 examples/unknown-field fixtures 与 canonical SHA-256；
`compatibility-matrix.v1.json` 固定同版本 frozen-shape、unknown major `reject_before_decode`、
unknown command `reject_with_schema_unsupported` 与四条负例绑定。2026-10-17 收口切片补齐三处
缺口：①资产覆盖门——目录里未登记的 `*.schema.json`、`examples/*.json`、`negative/*.json` 一律
`unlocked_schema_file`/`unreferenced_example_fixture`/`unreferenced_negative_fixture` 拒绝；
②`deprecated_fields` 与 schema 自身 `"deprecated": true` 标记逐字绑定，且每项必须是
`field`/`since`/`remove_in` 评审记录（`contract_deprecated_fields_unbound`/
`contract_deprecated_field_unreviewed`）；③跨语言 round-trip 的另一半——
`kiana-client/examples/ui09_schema_emit.rs` 把七个 fixture 经 Rust wire type 解码再编码成
`kiana.ui09-emitted.v1` 报告，`validate-ui09-schemas.py --emitted` 再对 canonical JSON Schema
校验（漂移 `emitted_schema_mismatch`、集合缺失 `emitted_contract_set_mismatch`）。

验收：`kiana-client/tests/ui09_schema_catalog.rs`（3 条生成 catalog 夹具）、
`kiana-client/tests/ui09_schema_round_trip.rs` 4 条
（`ui09_locked_examples_round_trip_through_the_rust_wire_types`、
`ui09_unknown_field_examples_are_rejected_by_strict_rust_decode`、
`ui09_version_and_command_gates_fail_closed_before_decode`、
`ui09_lock_contracts_are_registered_and_negative_fixtures_stay_refused`）、发射门
`kiana-client/examples/ui09_schema_emit.rs` + `python3 scripts/validate-ui09-schemas.py --emitted`、
source guard `kiana-core/tests/ui09_schema_guard.rs`（2 条：数据面纯净负断言 + 新 gate/example/lane
绑定），手动 lane 为 [`.github/workflows/ui09-schema-contract.yml`](../../.github/workflows/ui09-schema-contract.yml)
（`workflow_dispatch`-only），自动覆盖仍是统一 `ci.yml` 的 `kiana-client`/`kiana-protocol` 整 crate
分片与 `kiana-core-s6/6` 的 `ui09_schema_guard`。证明等级为 `source`，CI 结果未等待；浏览器侧
TypeScript 生成、live 跨版本互操作、durable schema 迁移与 physical 证据仍开放。

### 27.2 CLI 与 Workbench

<a id="step-ui-10"></a>



#### UI-10 · CLI 命令和输出归一化　✅

- 依赖：UI-07–09。代码：`kiana-entrypoints/src/cli.rs`（可按用户本节约定拆分）、command registry/dispatcher。
- 步骤：统一 `run/status/events/approve/deny/cancel/resume/receipt/export/session` 的参数、workspace/session 解析和 command ID；每个命令只调用 client facade。
- 先拒绝：缺 workspace、未知 session、没有 TTY 却请求交互、不可恢复命令使用隐式 retry、输出混入 token/secret。
- 成功/回归：旧 CLI 参数兼容或给迁移错误；命令到 protocol 的映射表覆盖 JSON、TTY、静默模式。
- 完成产物：CLI command contract、参数错误目录、帮助文本和迁移 fixture。
- 已接入 `kiana-client` 的 versioned `CliInvocation`/`CliOutput` contract、十个 canonical
  command IDs、legacy aliases、workspace/session/TTY/retry/redaction bounds，以及 GitHub-only
  fixtures/source guard；见 [UI-10 baseline](ui10-cli-contract-baseline.md)。证明等级为
  `source`，CI 结果未等待。

2026-10-17 收口切片把四个纯表补到同一 client 边界（`kiana-client/src/cli_contract.rs`）：
`CLI_COMMAND_SPECS` 为十个命令各一行（CLI token、wire name、摘要、usage、primary typed
operation、只读 follow-up operation、facade、session/retry/idempotency 策略、有界参数、支持的
输出模式）；`CliArgumentError` 给出 27 个稳定码与各自 remediation；`CliSessionRef` +
`resolve_session` 以服务端派生的 session 快照拒绝未知 id（`cli_session_unknown`），`latest`
仍交服务端解析；`CLI_LEGACY_FLAGS`/`normalize_legacy_args` 单向迁移旧参数
（`--session-id`→`--session`、`-c/--continue/-r`→`--resume`、`-p`→`--prompt`、`--print` 丢弃；
`--sdk-url`/`--input-format`/`--output-format`/`--debug-file`/`--record-only`/
`--replay-user-messages`/`--include-partial-messages`/`--fork-session` 一律
`cli_legacy_flag_retired`），argv 与 migration notice 都有上限，`--` 之后原样透传。命令参数按名
有界并先拒绝缺必填/未知键/类型或 enum 不合法/超限（`cli_argument_missing`/`cli_argument_unknown`/
`cli_argument_invalid`/`cli_argument_oversized`）；`quiet` 只允许 `run`/`status`/`cancel`/`resume`，
载荷即结果的命令（events/approve/deny/receipt/export/session）返回
`cli_output_mode_unsupported`。`cli_help_text()` 由同一批表确定性渲染，fixture 的 `help_shape`
钉住形状。验收：`kiana-client/tests/ui10_cli_contract.rs` 12 条（新增
`command_specs_match_the_declared_contract_fixture`、
`help_text_covers_every_command_and_output_mode`、`argument_errors_follow_the_stable_catalogue`、
`per_command_arguments_fail_closed_before_dispatch`、`output_mode_support_is_enforced_per_command`、
`session_resolution_refuses_unknown_ids_from_the_server_snapshot`、
`legacy_args_rewrite_or_fail_closed_per_the_migration_table`、
`legacy_normalization_bounds_and_passthrough_are_enforced`、
`typed_validation_maps_to_the_same_codes_as_the_string_api`）+ 声明式 fixture
[`kiana-client/tests/fixtures/ui10-cli-contract.json`](../../kiana-client/tests/fixtures/ui10-cli-contract.json)
（10 命令 / 27 错误码 / 14 legacy flag / 12 argv 行 / 预算 / help shape / required / denied /
limitations）；`kiana-entrypoints/tests/ui10_cli_boundary_guard.rs` 2 条守卫（client 边界无执行
权限 + fixture/lane/分片/baseline 绑定）。手动 lane 为
[`.github/workflows/ui10-cli-contract.yml`](../../.github/workflows/ui10-cli-contract.yml)
（`workflow_dispatch`-only、`permissions: contents: read`、fetch + fmt 门 + 两个 target +
`--no-run`），自动 lane 仍是统一 `ci.yml` 的 `kiana-client`/`kiana-entrypoints` 整 crate 分片。
证明等级为 `source`，CI 结果未等待；冻结的 `kiana-entrypoints/src/cli.rs` 迁移、真实 PTY/管道
字节级输出与 live/physical 证据仍开放。

<a id="step-ui-11"></a>



#### UI-11 · CLI JSON/TTY/exit code presenter　✅

- 依赖：UI-02/10。代码：CLI presenter、稳定 exit code 和 stderr/stdout 分流。
- 步骤：JSON 输出只写 schema DTO；TTY 输出可读但不成为事实；错误/警告/receipt 分流；Unknown、Cancelled、PolicyDenied 各有稳定 code。
- 先拒绝：JSON 中混 ANSI、stdout 打日志破坏管道、Unknown 返回 0、结构化 error 被字符串吞掉、超长 artifact 无界打印。
- 成功/回归：pipe/no-TTY、locale、SIGINT、broken pipe、分页、输出上限和 shell exit-code 测试；相同 action 在 Web/CLI 结果一致。
- 完成产物：golden outputs、exit-code 表和 `--json/--quiet` 文档。

已接入 `kiana-client::present_cli_output`、稳定 lifecycle/signal exit-code 表、JSON/TTY/quiet
stdout/stderr 分流、locale/pagination/no-TTY fences、warning/error bounds，以及 GitHub-only
deny-first fixtures/source guard；见 [UI-11 baseline](ui11-cli-presenter-baseline.md)。证明等级为
`source`，CI 结果未等待。

2026-10-17 收口切片把「纯 presenter + 6 条夹具 + 单一 source guard」补成卡片退出条件收口。
`CLI_EXIT_CODE_TABLE` 成为唯一退出码表（11 行：0/1/2/3/4/5/6/7/8/130/141，各带 wire name 与
meaning），与 `CliExitCode::ALL`/`wire_name`/`meaning`/`parse_name` 以及
`CapabilityErrorCode::policy().cli_exit` 双向绑定，`from_capability_error` 公开但数值与原私有实现
逐一对应；`CliSignal::parse` 归一 SIGINT/SIGPIPE 别名，未知名字不发明退出类。adapter 侧新增
`CliStreamFault` + `write_cli_presentation` + `exit_code_after_write`：首个写故障即返回、不重试、
不开文件、不 spawn pager，信号类故障（EPIPE→141、interrupted→130）覆盖响应退出类，普通 io
故障绝不覆盖策略拒绝码。`CliPagerPlan` 对已限界的 TTY body 按行切页：页拼接回原文逐字节相等、
超预算单行独占一页、footer 预算先留出所以页脚恒在，`page_bytes` 下界/footer 预留/页数上限
（`cli_pager_page_bytes_invalid`/`cli_pager_too_many_pages`）一律失败关闭；pagination 不是 pager，
不读用户按键、不发终端控制序列、不重跑命令。`CliLocale::from_env_value` 让 locale 只影响标签
（乱码回落 English，绝不当 gate），`CliPresentation::stdout_is_pipe_safe` 证明 JSON 管道里混不进
日志行或 ANSI。验收：`kiana-client/tests/ui11_cli_presenter.rs` 10 条先拒绝优先夹具（退出表↔代码
双向、lifecycle 场景、信号与写流、locale、`CapabilityErrorCode::ALL` 与 `policy().cli_exit` 遍历、
逐字节 golden（TTY/quiet）+ JSON 单行 DTO 键序与 error 原样 + 分页逐页拼接、12 行拒绝码、pager
预算、writer 首个故障停止且不覆盖拒绝码、同一 `ResponseEnvelope` 经 `stable_error_from_response`
与 CLI 分类一致）+ 声明式 fixture
[`kiana-client/tests/fixtures/ui11-cli-presenter.json`](../../kiana-client/tests/fixtures/ui11-cli-presenter.json)
（11 行退出表 / 11 行 lifecycle 场景 / 2 信号 / 3 写流故障 / 7 locale / 12 golden case / 12 行拒绝
矩阵 / 11 行 Web-CLI parity / required+forbidden / limitations）；
`kiana-entrypoints/tests/ui11_cli_presenter_guard.rs` 2 条守卫（纯投影 + fixture `required`/`forbidden`
双向绑定；fixture↔夹具名↔golden 名↔手动 lane↔整 crate 分片↔USER.md↔baseline，且冻结
`kiana-entrypoints/src/cli.rs` 不得出现第二套 presenter/退出表）。手动 lane 为
[`.github/workflows/ui11-cli-presenter.yml`](../../.github/workflows/ui11-cli-presenter.yml)
（`workflow_dispatch`-only、`permissions: contents: read`、fetch + fmt 门 + 两个 target + `--no-run`），
自动 lane 仍是统一 `ci.yml` 的 `kiana-client`/`kiana-entrypoints` 整 crate 分片；`USER.md` 记录
`--json`/`--quiet` 契约与同一张退出码表。证明等级为 `source`，CI 结果未等待；真实 PTY/管道字节
证据、冻结 `cli.rs` 迁移与 durable/live/physical 证据仍开放。

<a id="step-ui-12"></a>



#### UI-12 · TTY 输入状态机　✅

- 依赖：UI-08/10。代码：Workbench/CLI input layer；可参考 Pi 的跨 chunk parser 和 Codex composer。
- 步骤：解析 Unicode、IME、bracketed paste、历史、逐行/多行、resize、EOF、SIGINT；输入草稿与已提交 command 分离。
- 先拒绝：半截 escape sequence 执行命令、粘贴内容触发 shell、取消键改写已提交 action、超长输入内存失控。
- 成功/回归：PTY 分块注入、中文/组合字符、粘贴、窗口 resize、历史回退、Ctrl-C/Ctrl-D 和无 TTY 模式。
- 完成产物：输入状态图、PTY fixture、边界上限和取消语义。

已接入有界 `PtyChunkDecoder` / `TtyInputState`：半截 UTF-8/escape 只保留 pending，bracketed
paste 作为单个文本 payload，IME candidate、历史、逐行/多行、resize、EOF、SIGINT 和
NonTty fence 均与 draft/`CommittedInput` 分离；WorkBench 事件适配仍只把提交交给既有
`WorkbenchView::interpret_line` 和 DaemonHost 路径。deny-first PTY/Unicode/paste/resize/EOF/
SIGINT/bounds fixtures、source guard 和 GitHub-only workflow 已接入；见 [UI-12 baseline](ui12-tty-input-baseline.md)。证明等级为 `source`，CI 结果未等待。

2026-10-17 收口切片补齐上一轮遗留的缺口（专属 lane 被 08552ada workflow fan-in 删除、
若干先拒绝用例只是隐含）：`kiana-entrypoints/src/tty_input.rs` 的 IME API
（`begin_ime`/`update_ime`/`commit_ime`/`cancel_ime`）现在在 `InputMode::NonTty` 下于任何
composition 状态存在之前返回 `tty_input_requires_tty`；声明式 fixture
[`kiana-entrypoints/tests/fixtures/ui12-pty-events.json`](../../kiana-entrypoints/tests/fixtures/ui12-pty-events.json)
（`kiana.ui12-pty-events.v1`）固化为六个 stream（含期望 transition 序列 changed/committed/
cancel/resized/ignored）、五条原子拒绝、预算（65536 bytes / 16384 scalars / 64 history
entries）与 required/denied/limitations 列表；`kiana-entrypoints/tests/ui12_tty_input.rs` 由
11 条扩到 19 条先拒绝优先夹具，新增
`partial_escape_sequences_never_synthesize_enter_across_chunks`、
`a_dangling_escape_across_chunks_is_rejected_and_never_submits_the_draft`、
`bracketed_paste_payload_controls_stay_data_until_the_closing_marker`、
`oversized_bracketed_paste_is_rejected_at_the_decoder_boundary_and_clears_pending`、
`scalar_safe_editing_never_splits_a_character_and_combining_marks_stay_data`、
`non_tty_mode_rejects_ime_and_resize_before_any_state_change`、
`ime_rejects_paste_enter_and_oversized_candidates_without_touching_the_draft`、
`history_navigation_is_bounded_and_editing_resets_the_cursor`；
`kiana-entrypoints/tests/ui12_tty_input_guard.rs` 新增第二条守卫
`tty_input_completion_slice_stays_wired_to_the_fixture_lane_and_shard`（fixture 预算↔源码常量、
登记夹具名↔真实测试、required marker↔baseline、拒绝码↔源码、Workbench 提交路径、手动 lane
逐字命令与整 crate 分片）。重建手动 lane
[`.github/workflows/ui12-tty-input.yml`](../../.github/workflows/ui12-tty-input.yml)
（`workflow_dispatch`-only、`permissions: contents: read`、`cargo fetch --locked` + fmt 门 +
两个 target（`--test-threads=1`）+ `--no-run`），自动 lane 仍是统一 `ci.yml` 的
`kiana-entrypoints` 整 crate 分片（`scripts/ci/test-shards.json` 未改）。证明等级为 `source`，
CI 结果未等待；paste 内 CR 仍按终端控制拒绝（不归一化为换行）、live Workbench 仍用 crossterm
自有事件流（`PtyChunkDecoder` 是确定性 source fixture）、无 OS-backed PTY/IME 与
durable/live/physical 证据仍开放。

<a id="step-ui-13"></a>



#### UI-13 · Workbench 时间线与结果渲染　✅

- 依赖：UI-06/08/12。代码：`workbench_chat.rs`、`stream_render.rs` 及 presenter。
- 步骤：将 Delta/Terminal/Usage/ToolCall/Approval/Artifact/Error 映射为稳定 Item；实现折叠、展开、加载、未知和限制标记；过滤 ANSI/OSC 和不可信 markdown。
- 先拒绝：按到达时间覆盖事实、tool result 伪装 assistant text、错误被清屏、无限 delta/JSON/diff 渲染。
- 成功/回归：乱序/重复/gap、超长输出、二进制 MIME、terminal、approval 和 terminal 后 event；渲染性能与内存有界。
- 完成产物：Workbench render snapshots、item kind 表和结果截断策略。

已接入有界 typed `TimelineRenderer`：RunStreamEnvelope 先做 run/epoch/cursor 校验，再将
Delta/Terminal/Usage/ToolCall/Approval/Artifact/Error/Unknown 投影为稳定 item；重复、乱序、
gap、terminal 后事件、foreign run 和超限内容均 fail-closed 或显式标记。ANSI/OSC/control
序列与常见 secret 值在纯文本边界处理，Workbench 只把展示投影映射为兼容 ChatMessage，不
新增执行或授权路径。deny-first fixture、source guard 和 GitHub-only workflow 已接入；见
[UI-13 baseline](ui13-workbench-render-baseline.md)。证明等级为 `source`，CI 结果未等待。

2026-10-17 收口切片补齐上一轮遗留的缺口（专属 lane 被 08552ada workflow fan-in 删除；
「按到达时间覆盖事实 / tool result 伪装 assistant / 错误被清屏 / 无限 diff / 二进制 MIME」
只有隐含覆盖）：`kiana-entrypoints/src/workbench_render.rs` 新增 `TimelineDisplayRole` +
`TimelineItemKind::display_role()` 作为所有入口唯一的展示角色权威（只有 Delta/Terminal 是
assistant，Artifact 是 changed，Usage/ToolCall/Approval/Error/Unknown/Gap/Limit 一律
system；`workbench_chat` 不再自己 match item kind）；声明式 fixture
[`kiana-entrypoints/tests/fixtures/ui13-render-snapshot.json`](../../kiana-entrypoints/tests/fixtures/ui13-render-snapshot.json)
升级为 `kiana.ui13-render-snapshot.v1`（预算 512 items / 512 KiB / 32 KiB item / 16 KiB
JSON / 16 KiB diff / 64 artifacts per response；五个先拒绝场景：到达顺序不覆盖错误、二进制
MIME 不内联、带标签的 text diff、terminal 围栏、gap 阻塞；13 个夹具名、required/denied/
limitations 列表）；Artifact 投影新增 binary MIME / `encoding: base64`/`hex` / 未声明载荷的
显式标记（绝不解码、内联或回显字节），text/diff 走独立 `MAX_TIMELINE_DIFF_BYTES` 截断且
保留 `id`/`path` 标签；`apply_event`（legacy adapter 路径）与 `apply` 一样保留 gap/terminal
围栏；`kiana-entrypoints/tests/ui13_workbench_render.rs` 5 → 13 条先拒绝优先夹具（新增
`arrival_order_and_replays_never_overwrite_the_recorded_error`、
`tool_results_usage_approvals_and_errors_are_never_assistant_text`、
`binary_and_unspecified_artifact_payloads_are_never_inlined`、
`text_diff_artifacts_are_labelled_and_truncated_by_the_declared_policy`、
`terminal_fences_every_late_event_kind`、
`gap_marker_blocks_both_typed_and_legacy_delivery_until_reset`、
`hostile_stream_keeps_items_and_bytes_bounded`、`untrusted_markdown_is_rendered_as_inert_plain_text`），
既有断言一条未放宽；`kiana-entrypoints/tests/ui13_workbench_render_guard.rs` 新增第二条
守卫 `ui13_completion_slice_stays_wired_to_the_fixture_lane_and_shard`（预算↔源码常量、
13 个夹具名↔真实测试、scenario↔夹具、required↔baseline、denied↔源码、
protocol-propagated 码↔`kiana-protocol`、append-only 负断言、
`display_role` 单一权威、手动 lane 逐字命令与 `kiana-entrypoints` 整 crate 分片）；
重建手动 `.github/workflows/ui13-workbench-render.yml`（`workflow_dispatch`-only、
`permissions: contents: read`、fetch/fmt 门 + 两个 target + `--no-run`），自动覆盖仍是统一
`ci.yml` 的 `kiana-entrypoints` 整 crate 分片（`scripts/ci/test-shards.json` 未改）。

<a id="step-ui-14"></a>



#### UI-14 · Workbench controller 与命令面板　✅

- 依赖：UI-07/08/10–13。代码：controller、keymap、session switcher、command palette。
- 步骤：实现 open/attach/new/continue/status/run/cancel/resume/receipt；controller 只把用户意图翻译成 UiAction，提交状态与 run 状态分离。
- 先拒绝：切换 session 误提交旧草稿、重复快捷键产生两个 command、关闭窗口自动 cancel/resume、无 capability 显示 action。
- 成功/回归：多 session、重复 submit、断线、响应丢失、stale card、SIGTERM 和重启后恢复测试。
- 完成产物：controller state machine、keymap 文档、action audit trace。

完成切片（2026-10-17，CI-only）把「纯 `WorkbenchController` source contract、fixture、source
guard 与已接入 workflow」补成卡片退出条件收口——`kiana-entrypoints/src/workbench_controller.rs`：

1. 交付丢失不再只有「在途」一种状态：`set_connection` 返回
   `ConnectionTransition { previous, current, in_flight }`，离开 `Attached` 时把在途提交改写成
   `SubmissionStatus::Unknown` 并把原始 `SubmissionIdentity`（command id + idempotency key）
   交回调用方，不重试、不发新命令 ID、不 cancel/resume、不改写 run 维度；重连不解锁，未对账
   前新的变更一律 `duplicate_submission`（`connection_loss_marks_the_submission_unknown_with_its_original_identity`、
   `response_loss_never_mints_a_second_command_and_survives_a_terminal_run`）；
2. `Degraded` 本地拒绝（`workbench_controller_offline`）：控制器没有发送队列，「先准备、稍后发」
   在结构上不存在；`observe_run_status` 到终态也不再清除未对账提交，因此一次响应丢失不会被
   「run 已经结束」洗掉（`submission-state-and-run-state-stay-separate`）；
3. `note_termination_signal`（`TerminationSignal::{Interrupt,Terminate}`，`sigint`/`sigterm`）
   把「进程收信号」做成一等状态：只标记结果未知，绝不合成 cancel/resume，也不等于关窗；
   `WindowClose` 仍可收尾（`sigterm-is-not-a-window-close-or-a-cancel`）；
4. `recover(RecoveryObservation)` 是跨进程恢复的唯一入口：observation 只有服务端观测到的
   workspace/principal/epoch/cursor/revision/connection/sessions/active_session_id/capabilities，
   **没有** draft/submission/in-flight action 的位置；固定顺序拒绝
   `workbench_recovery_offline`/`workbench_controller_scope_invalid`/
   `workbench_controller_cursor_invalid`/`workbench_recovery_scope_changed`/
   `workbench_recovery_epoch_mismatch`/`workbench_recovery_cursor_stale`/
   `workbench_recovery_revision_stale`/`workbench_recovery_session_changed`，再走
   `SessionSwitcher` 与 `UiCapability::validate`；成功时只带走未对账身份并**延续 idempotency
   序号**（新动作不可能与已死进程用过的 key 重名），草稿清空且 revision 前进，旧进程的草稿版本
   仍然 `stale_draft`（`restart-recovery-carries-only-server-coordinates`、
   `restart-recovery-continues-the-idempotency-sequence`）；
5. `Keymap` 除重复快捷键（`duplicate_shortcut`）外再拒绝一个命令绑两个键位
   （`duplicate_command_binding`）；`in_flight` 只保留身份（`Option<SubmissionIdentity>`），
   结构上不可重放；审计 trace 有界（128）且只从前端裁剪、只保留完整身份对
   （`action-audit-trace-is-bounded-and-append-only`）。

声明式 fixture
[`kiana-entrypoints/tests/fixtures/ui14-controller-actions.json`](../../kiana-entrypoints/tests/fixtures/ui14-controller-actions.json)
升级为 `kiana.ui14-workbench-controller.v1`（预算 256 sessions / 32 palette entries / 128 audit
entries / 64 KiB text / 256 target / 512 title / 9 commands / 8 shortcuts；九行 intent 表含
target/payload 键/mutating/clears-draft；12 个先拒绝 scenario 带期望 refusal 码；recovery 段声明
可观测字段、不得带走与必须带走的三类；21 个夹具名；required/denied/denial_templates/
propagated_denials/limitations）。`kiana-entrypoints/tests/ui14_workbench_controller.rs` 7 → 21
条先拒绝优先夹具（新增 `session_switch_cannot_submit_the_previous_draft_and_targets_the_new_session`、
`denied_intents_leave_no_action_no_sequence_and_no_audit_entry`、
`multi_session_targets_stay_bound_to_the_selected_session`、
`connection_loss_marks_the_submission_unknown_with_its_original_identity`、
`response_loss_never_mints_a_second_command_and_survives_a_terminal_run`、
`stale_card_results_are_rejected_and_never_resurrect_a_submission`、
`termination_signals_are_not_window_close_cancel_or_resume`、
`restart_recovery_refuses_stale_widened_or_foreign_coordinates`、
`restart_recovery_carries_only_server_coordinates_and_the_unreconciled_identity`、
`restart_recovery_without_a_pending_submission_starts_idle_and_allows_work`、
`hostile_control_traffic_keeps_the_controller_bounded_and_audit_append_only`、
`every_declared_intent_translates_to_its_stable_action`、
`declared_scenarios_are_exercised_by_their_named_fixtures` 等），既有断言一条未放宽；
`kiana-entrypoints/tests/ui14_workbench_controller_guard.rs` 新增第二条守卫
`ui14_completion_slice_stays_wired_to_the_fixture_lane_and_shard`（预算↔源码常量、命令/键位↔
源码、21 个夹具名↔真实测试、scenario↔夹具与 refusal 码、required↔baseline、denied↔源码、
denial_templates↔源码+夹具、protocol-propagated 边界↔`kiana-protocol`、手动 lane 逐字命令与
`kiana-entrypoints` 整 crate 分片，并负断言 `in_flight: Option<WorkbenchUiAction>` 不存在）；
重建手动 `.github/workflows/ui14-workbench-controller.yml`（`workflow_dispatch`-only、
`permissions: contents: read`、fetch/fmt 门 + 两个 target（`--test-threads=1`）+ `--no-run`），
自动覆盖仍是统一 `ci.yml` 的 `kiana-entrypoints` 整 crate 分片（`scripts/ci/test-shards.json`
未改）；无 Cargo.toml/Cargo.lock/package.json 变化、无新依赖/第二执行循环，五个模型可见工具面
不变、未触碰冻结项；本地未运行测试/build/check/clippy/smoke（只跑 `cargo fmt --all`），CI 回执
未等待，proof_level 维持 `source`；见
[UI-14 baseline](ui14-workbench-controller-baseline.md)。

<a id="step-ui-15"></a>



#### UI-15 · Workbench inbox、Diff 和 Receipt　✅

- 依赖：UI-05/08/13/14、Event/Receipt §23。代码：inbox panel、artifact viewer、receipt view。
- 步骤：展示 reason/scope/expiry/fields/allowed decisions；Diff 由服务端 Artifact ref 提供并校验 digest/revision；Receipt 显示 files/cost/unknown/limitations/provenance。
- 先拒绝：过期 approval、revision mismatch、客户端自行改审批 payload、artifact digest 错、ResultUnknown 画成绿色完成。
- 成功/回归：approve/deny/edit/restore、部分文件、分页 artifact、撤销、receipt 重算和终态回放。
- 完成产物：Workbench UX fixture、receipt golden、审阅错误恢复说明。

已接入纯 `WorkbenchInboxCard`/`WorkbenchInbox`、`ArtifactPage`/`ArtifactViewer` 与
`WorkbenchReceipt` source contract：inbox 保留服务端 reason/scope/expiry/fields/allowed
decisions/payload，过期/撤销审批、revision mismatch、客户端 payload mutation 和重复卡片
fail-closed；Diff 只接受服务端 `ArtifactRef` 页并校验 page/content digest、revision、分页
边界与完整 artifact digest；Receipt 显示 files/cost/unknown/limitations/provenance，
`ResultUnknown` 永不画成绿色完成。fixture、source guard 和 GitHub-only workflow 已接入；见
[UI-15 baseline](ui15-workbench-inbox-receipt-baseline.md)。证明等级为 `source`，CI 结果未等待。

2026-10-17 收口切片把「纯 inbox/artifact/receipt source contract + 少量夹具 + 无专属 lane」补成
卡片退出条件收口——`kiana-entrypoints/src/workbench_review.rs`：

1. **展示与拒绝同源**：新增 `WorkbenchDecision`（五个词的封闭审批词汇，`ALL`/`as_str`/`parse`）与
   `InboxCardState`（`actionable`/`expired`/`revoked`）；`WorkbenchInboxCard::display_state` 只由服务端
   facts 派生，`prepare_decision` 走同一状态机，所以画成 expired/revoked 的卡片不可能产出可执行决策；
   卡片声明任何词汇表外的 decision 一律 `inbox_allowed_decision_unsupported`；
   手工构造的 `WorkbenchInboxDecision` 现在必须自证 payload digest
   （`inbox_decision_payload_digest_mismatch`/`inbox_decision_payload_limit`），并拒绝词汇表外的
   decision 值（`inbox_decision_value_unsupported`）；
2. **inbox 面板不再只是去重**：`WorkbenchInbox::upsert` 允许服务端刷新但拒绝 expected revision 回退
   （`inbox_card_revision_stale`）与「已撤销卡片被后来的非撤销 payload 复活」
   （`inbox_card_revocation_regression`）；`revoke` 对未知 action id 报 `inbox_action_unknown` 而不是静默
   丢弃；`state_of`/`actionable` 让 presenter 只读共享投影（过期/撤销卡片仍可见但不在 actionable 列表）；
3. **Diff 只来自服务端页**：`ArtifactViewState`（`empty`/`partial`/`complete`）+ `missing_pages()` +
   `retained_bytes()`/`retained_pages()`；部分内容永远不是 artifact（`content()` 只在完整可信时返回），
   presenter 拿到的是还需向服务端请求的页号列表，而不是客户端拼出的 diff；
4. **Receipt 重算与终态回放分开**：`recompute` 现在对重建结果做完整校验，不一致片段
   （`receipt_file_revision_invalid` 等）返回错误而不是画出来；唯一替换保留回执的入口是
   `apply_server_receipt`，固定顺序拒绝 `receipt_run_mismatch`/`receipt_replay_stale`/
   `receipt_replay_conflict`（同 cursor 必须同 digest/状态/unknown 标记）与
   `receipt_terminal_regression`（终态不得回到非终态），同 cursor 的完全相同回执是幂等 `Replayed`；
   `is_terminal()` 由非终态集合的补集派生（新增状态默认终态，fail-closed）；
5. **receipt golden 与审阅错误恢复说明**：`ReceiptGolden`（`kiana.workbench-receipt-golden.v1`，文件按
   path/revision/status 排序、digest 归一化、逐字节稳定，`green`/`unknown`/limitations/provenance 全部
   可见）；`REVIEW_REFUSALS` 是唯一 presenter 可见拒绝目录（32 码 × `inbox`/`decision`/`artifact`/
   `receipt` 四阶段，每码一条有界 review_recovery 说明，且没有任何一条会代替操作者 resubmit/retry/
   approve/restore/reconcile）。

声明式 fixture `kiana.ui15-workbench-review.v1`
（[`kiana-entrypoints/tests/fixtures/ui15-workbench-review.json`](../../kiana-entrypoints/tests/fixtures/ui15-workbench-review.json)：
11 行预算、六组词汇表、14 个先拒绝优先 scenario + refusal 码、22 个夹具名、required/denied/
denial_templates/recovery/limitations、receipt golden 逐字期望值）；`kiana-entrypoints/tests/ui15_workbench_review.rs`
7 → 22 条先拒绝优先夹具（新增 `every_declared_decision_produces_its_stable_decision_intent`、
`inbox_decisions_are_restricted_to_the_declared_vocabulary`、
`hand_built_decisions_must_still_match_their_payload_digest`、
`inbox_bounds_scope_fields_decisions_and_payload_size`、
`inbox_deduplicates_cards_and_updates_are_never_a_silent_downgrade`、
`artifact_pagination_is_partial_until_every_server_page_is_verified`、
`artifact_pages_outside_the_declared_bounds_are_refused`、
`receipt_never_paints_result_unknown_as_green_completed`、
`receipt_replay_is_idempotent_and_never_regresses_a_terminal_state`、
`receipt_recompute_refuses_inconsistent_server_fragments`、
`receipt_golden_is_deterministic_and_keeps_unknown_and_limitations_visible`、
`review_refusal_catalogue_covers_every_presenter_visible_refusal`、
`review_contracts_round_trip_through_typed_input`、
`declared_scenarios_are_exercised_by_their_named_fixtures`），既有断言一条未放宽；
`kiana-entrypoints/tests/ui15_workbench_review_guard.rs` 1 → 2 条守卫，新增
`ui15_completion_slice_stays_wired_to_the_fixture_lane_and_shard`（预算↔源码常量、六组词汇表↔
enum `ALL`/`as_str`、22 个夹具名↔真实测试、scenario↔夹具与「只在目录之前的模块正文里」核对 refusal
码、denied↔`ReviewRefusal::codes()` 双向且每码都有恢复说明与被夹具断言、denial_templates↔源码字段与
组合码、required↔baseline、propagated↔`kiana-protocol`、手动 lane 逐字命令与 `kiana-entrypoints`
整 crate 分片）；重建手动 `.github/workflows/ui15-workbench-review.yml`（`workflow_dispatch`-only、
`permissions: contents: read`、fetch/fmt 门 + 两个 target（`--test-threads=1`）+ `--no-run`），
自动覆盖仍是统一 `ci.yml` 的 `kiana-entrypoints` 整 crate 分片（`scripts/ci/test-shards.json` 未改）。
证明等级为 `source`，CI 结果未等待；真实 Workbench 事件循环/渲染、durable artifact store、跨进程回执
重算、provider 成本真值与 live/physical 证据仍开放。

### 27.3 Web、SSE 和多标签页

<a id="step-ui-16"></a>



#### UI-16 · Web 路由、来源校验和最小健康信息　✅

- 依赖：UI-03/07/11。代码：`kiana-entrypoints/src/web.rs`（可拆分 router/auth/handlers）。
- 步骤：按 health/state/sessions/events/run/cancel/trust/sandbox/session/receipt/approval/resume/command 分类；统一 loopback Host/Origin/token 校验和请求大小/速率上限。
- 先拒绝：缺 token 的 state/events/action、任意 Origin、路径穿越、跨 workspace session、health 泄漏绝对路径或内部错误；不新增浏览器直连执行器。
- 成功/回归：每路由 deny-first、OPTIONS/GET/POST 方法、错误码、token rotation、过大 body、并发 tab 和日志脱敏。
- 完成产物：route/auth matrix、安全 header/CSP 草案、迁移旧 endpoint 的兼容表。

实现基线：[UI-16 Web routes, origin checks and minimal health baseline](ui16-web-routes-baseline.md)。完成切片（2026-10-17）
把该基线补成卡片退出条件：`WEB_ROUTE_MATRIX`（28 路由，新增 `POST /api/token/rotate`，class `Token`）与
Axum `.route(...)` 注册、fixture 三方双向绑定且 `OPTIONS` 从不注册；`WEB_ERROR_MATRIX` 63 码 + 机械 status 映射，
守卫从源码错误码字面量反向重扫；安全 header/CSP 草案常量化并被响应中间件使用（覆盖 405/413/429）；首版
workbench 九条旧 endpoint 的 1:1 兼容表（无别名/redirect）；每路由 `authorize_*` 先于任何 host/session/ledger
访问（`ui16_per_route_authorization_precedes_any_host_access` 逐 handler 证明）；`POST /api/token/rotate` 真正接线
（递增 `token_generation`、旧凭据立即 401、响应不回显凭据、页面从 host-only `/` 重新 hydrate）；限额先拒绝
（128 KiB body → 413、8 KiB URI → 400、编码穿越 → 400、120 req/s → 429 且保留安全 header、窗口准时恢复）；
并发 tab 只读同一 server snapshot；health 低细节 + host-only；请求/服务路径零日志且不能出现第二执行器。
新增 4 条真实 loopback HTTP 夹具（`kiana-entrypoints/src/web.rs` 的 `web::tests::ui16_*`，真实 `DaemonHost` +
raw socket）与重写的 `kiana-entrypoints/tests/ui16_web_routes.rs`/`ui16_web_routes_guard.rs`；重建手动
`.github/workflows/ui16-web-routes.yml`（`workflow_dispatch`-only、`permissions: contents: read`），自动覆盖仍是
统一 `ci.yml` 的 `kiana-entrypoints` 整 crate 分片。无 Cargo.toml/Cargo.lock/package.json 变化、无新依赖/第二执行
循环，五工具面不变；本地未运行（CI-only，回执未等待），proof 上限 `source`；浏览器矩阵、跨进程 transport、
durable rate state/token 持久化与 live/physical 证据仍开放。

<a id="step-ui-17"></a>



#### UI-17 · Web snapshot hydrate、历史和分页　🔄

- 依赖：UI-05/08/16。代码：Web bootstrap、session/history/artifact query。
- 步骤：首屏先 hydrate snapshot，再开启 feed；历史按 server cursor 分页，缓存带 instance/epoch/schema；loading、empty、partial、limited 状态明确。
- 先拒绝：仅依赖 localStorage 状态、把空响应当无 session、跨 tab 复用 owner 数据、分页 cursor 重放旧 action。
- 成功/回归：首次打开/刷新/旧缓存/分页边界/离线/恢复/多 session；重新 hydrate 后 reducer 与 CLI status 一致。
- 完成产物：hydrate trace、分页 fixture、缓存淘汰和可见限制说明。

实现基线：[UI-17 Web snapshot hydrate, history and pagination](ui17-web-hydrate-baseline.md)。

<a id="step-ui-18"></a>



#### UI-18 · Web SSE reconnect、Last-Event-ID 和 gap　🔄

- 依赖：UI-06/16/17。代码：SSE handler、EventSource wrapper、feed reducer。
- 步骤：发出 `id`/event/schema/epoch，读取 Last-Event-ID；心跳、退避、最大重连、terminal/gap 事件；连接 listener 在 resume 前安装。
- 先拒绝：重复/跳号静默接受、旧 epoch 继续渲染、事件流无限 buffer、SSE token 出现在历史/日志、断线自动重投副作用 command。
- 成功/回归：网络切断、代理重连、重复 Last-Event-ID、server restart、gap→snapshot、慢 tab、terminal retention。
- 完成产物：浏览器/HTTP fixture、reconnect state machine、可观测重连指标。

实现基线：[UI-18 Web SSE reconnect, Last-Event-ID and gap](ui18-web-sse-baseline.md)。当前
source slice 在既有 loopback `/api/events` 与 `DaemonHost` 只读展示投影上补充 server-owned
`id`/event/schema/epoch、严格 Last-Event-ID 冲突拒绝、显式 heartbeat、有限退避/最大重连和
gap→snapshot hydrate；重连路径不会提交 `/api/run`、`/api/cancel` 或其他副作用 command。

<a id="step-ui-19"></a>



#### UI-19 · Web session ownership 与多 tab 并发　🔄

- 依赖：UI-04/08/16–18。代码：session lease/tab identity、action coordinator。
- 步骤：每 tab 独立 draft/submission；服务端按 principal/session owner 授权；同一 action 的重复提交显示原 command 结果；跨 tab 通过 feed 观察而非共享可变状态。
- 先拒绝：tab A 操作 tab B 私有 session、stale card 通过、重复 click 产生两个 effect、关闭 tab 触发 cancel。
- 成功/回归：双 tab approve/cancel/edit、刷新、浏览器睡眠、竞态 CAS、token 轮换和 owner 退出。
- 完成产物：多 tab e2e、tab/session 关系图和冲突文案。

实现基线：[UI-19 Web session ownership and multi-tab baseline](ui19-session-tabs-baseline.md)。当前
source slice 在既有 loopback Web 与 `DaemonHost`/ControlPlane 路径上补充 server-owned principal/session/tab
lease、tab-local draft/submission、stale/CAS deny、重复提交原响应、跨 tab feed-only observation 与
close-without-cancel fence；GitHub-only fixture/workflow 已接入，证明等级为 `source`，CI 结果未等待。

<a id="step-ui-20"></a>



#### UI-20 · Web 时间线组件迁移　🔄

- 依赖：UI-08/13/17/18。代码：从当前 inline HTML 渐进抽出 Thread/Turn/Item、status bar、feed indicator 组件；可采用 React/TS/Vite，但先保持静态构建可运行。
- 步骤：按 server item kind 渲染 delta/tool/approval/error/unknown；虚拟化和窗口化受有界 cursor 控制；错误、partial、replay、loading 可感知。
- 先拒绝：前端根据文本猜 item kind、innerHTML 直插不可信内容、虚拟化丢 pending/unknown、render key 使用数组 index。
- 成功/回归：旧页面功能 parity、长时间流、刷新重连、XSS payload、深链接和窄屏；组件只依赖 shared store/client。
- 完成产物：组件目录、迁移开关、浏览器截图/golden 和 bundle size 基线。

实现基线：[UI-20 Web timeline component migration](ui20-web-timeline-baseline.md)。当前 source
slice 在既有 UI-17 hydrate、UI-18 SSE 和 UI-19 tab/session projection 上加入 typed client
`WebTimelineItemKind`/`WebTimelineWindow`、server-owned stable `ItemView.id`、DOM text-only
timeline renderer 与受保护 pending/unknown recent window；loading/partial/replay/offline 以
状态节点可见，unknown 不从文本猜测且要求保守刷新。GitHub-only fixture/source guard/workflow
已接入，浏览器截图/golden、真实长流/窄屏、视觉 parity、跨进程/durable replay 和 live/physical
proof 留后续。

<a id="step-ui-21"></a>



#### UI-21 · Web Human Inbox 与审批动作卡　🔄

- 依赖：UI-02/04/05/15/20。代码：approval/inbox cards、form schema、action submitter。
- 步骤：表单字段、范围、理由、expiry、allowed decision 由 HumanActionCard 驱动；危险动作默认折叠并显示 effect scope；提交后显示 command status。
- 先拒绝：客户端添加隐藏字段扩大 scope、审批过期仍可 click、deny 需要重新执行 tool、Unknown 自动显示 retry。
- 成功/回归：approve/deny/edit/restore/continue、必填/非法值、过期/撤销/CAS conflict、重复 click 和响应丢失。
- 完成产物：inbox schema renderer、deny/happy fixtures、审计事件映射。

实现基线：[UI-21 Web Human Inbox and approval action cards](ui21-web-human-inbox-baseline.md)。当前
source slice 在 UI-15 server-owned inbox/review、UI-04 action CAS 和 UI-19 tab/session ownership
上补充 `UiHumanInboxV1`/`UiHumanActionCardV1`/`UiHumanActionIntentV1`，Web 响应附带只读卡片
元数据，表单字段由卡片驱动并以稳定 idempotency key 提交 `human.resolve`。过期/撤销、旧
inbox/card revision、owner/tab mismatch、重复卡、隐藏字段/载荷 digest 变更和 Unknown 自动
重试均 fail-closed；incident/limitation/Unknown 在结果面板可见。GitHub-only fixture、source
guard 与 workflow 已接入；浏览器 E2E、durable/live/physical proof 留后续。

<a id="step-ui-22"></a>



#### UI-22 · Web artifact/diff/receipt detail　🔄

- 依赖：UI-05/15/20/21。代码：artifact API、diff viewer、receipt page。
- 步骤：按 ref 分页拉取文本/patch/metadata，校验 digest、MIME、revision、size；展示 server stats、file status、provenance、limitations 和 unknown。
- 先拒绝：任意 URL fetch、SVG/HTML/ANSI 注入、digest mismatch 继续预览、客户端 diff 代替授权载荷、私有 artifact 出现在列表。
- 成功/回归：大文件/二进制/截断/坏 digest/过期 ref、部分文件审批、receipt 从新进程重算。
- 完成产物：artifact viewer、content policy、diff/receipt golden 与大小上限。

实现基线：[UI-22 Web artifact, diff and receipt detail](ui22-web-detail-baseline.md)。当前 source
slice 在 UI-21 server-owned card/session projection 上补充 typed ArtifactRef/page/detail、server
diff statistics、receipt cross-links、bounded cursor/revision/session/tab fencing 与 text-only
detail renderer。坏 digest、stale ref、跨会话枚举、HTML/SVG/ANSI/raw path/secret、任意 URL fetch、
客户端 diff 与 Unknown 绿色成功均 fail-closed；GitHub-only fixture/source guard/workflow 已接入，
浏览器 E2E、完整 durable artifact/diff index、跨进程 receipt 重算与 live/physical proof 留后续。

<a id="step-ui-23"></a>



#### UI-23 · Web 可访问性、焦点和内容安全　🔄

- 依赖：UI-16–22。代码：CSS/DOM/ARIA/CSP、keyboard focus manager、sanitizer。
- 步骤：定义 landmark/live region、焦点回收、键盘顺序、缩放/对比度/减少动画；严格 CSP、无 eval/inline script（迁移期有例外要登记）、sanitize markdown/HTML。
- 先拒绝：仅用 aria 属性宣称达标、键盘无法处理 approval/cancel、焦点跳出 modal、用户/工具输出执行脚本、CSP 报错被吞掉。
- 成功/回归：键盘/读屏/200% zoom/窄屏、恶意 markdown/URL/OSC、CSP violation、prefers-reduced-motion。
- 完成产物：WCAG 2.2 检查表、axe/手工证据、CSP report 和无障碍回归截图。

实现基线：[`ui23-web-accessibility-baseline.md`](ui23-web-accessibility-baseline.md)。当前 source
slice 在 UI-22 typed timeline/inbox/detail/session/SSE projection 上补充 nonce CSP/安全响应头、
可见 CSP violation、text-only sanitizer、session/tab/epoch-bound focus trap、键盘/ARIA/窄屏/
高对比/减少动画回退和 deny-first fixture/source guard/workflow；浏览器 axe/读屏/真实键盘、
200% zoom 截图、跨进程 lease race、durable/live/physical proof 留后续。

### 27.4 Electron/Desktop 与 IDE

<a id="step-ui-24"></a>



#### UI-24 · Electron IPC sender、导航和新窗口 allowlist　🔄

- 依赖：UI-02/03/16/23。代码：`contrib/desktop/main.js`、`preload.js`、IPC handlers。
- 步骤：为每个 IPC 校验 `event.senderFrame`、origin、channel、workspace binding；`will-navigate`/`setWindowOpenHandler` 只允许受信 loopback URL 和显式外部浏览器打开。
- 先拒绝：任意 renderer 调 `workspace:*`、URL 伪造 workspace/token、未知 channel、导航到 file/javascript/data、不受控新窗口。
- 成功/回归：合法/伪造 sender、窗口重定向、open external、preload contextIsolation/nodeIntegration、dev/prod URL 差异。
- 完成产物：IPC allowlist、sender test、CSP/BrowserWindow 安全配置审计。

实现基线：[`ui24-electron-ipc-baseline.md`](ui24-electron-ipc-baseline.md)。当前 source slice
在 Electron 壳加入 `kiana.desktop.ipc.v1` handshake、senderFrame/origin/channel/workspace
binding/nonce 校验，四个 workspace 调用只转换为 `kiana.desktop.intent.v1` typed intent；
`will-navigate` 与 `setWindowOpenHandler` 仅接受绑定 loopback/welcome，显式安全 HTTP(S) 在
系统浏览器打开，unsafe/unknown/token-bearing URL 和不受控 sender/window 拒绝。preload
不接触 Web bearer token，BrowserWindow 固定 sandbox/contextIsolation/no-node-integration；
fixture、source guard、CI workflow 与 baseline 已加入。真实 Electron/browser/OS、durable、live
和 physical proof 留后续。

<a id="step-ui-25"></a>



#### UI-25 · Desktop readiness、attach 和 worker 生命周期　🔄

- 依赖：UI-03/24。代码：worker discovery/readiness、process group stop、single-instance lock。
- 步骤：ready record 使用结构化 stdout/sidecar，不从任意 stderr URL 猜地址；Electron attach 到正确 instance/workspace；启动失败、崩溃、升级和 stop 有状态。
- 先拒绝：PID 重用、旧 ready URL、错误 workspace、kill 单进程留下子进程、worker 未 ready 就发 action、关闭自动 resume。
- 成功/回归：cold start/attach/restart/crash/SIGTERM/子进程、端口占用、重复窗口、升级兼容和 process-group cleanup。
- 完成产物：桌面生命周期图、readiness fixture、物理进程 e2e 与诊断日志。

实现基线：[`ui25-desktop-readiness-baseline.md`](ui25-desktop-readiness-baseline.md)。当前 source slice
把 nonce-bound structured stdout、受保护的 workspace instance sidecar 和 `/api/health` identity
绑定到同一个 worker PID/workspace；侧车 lease identity 与 UI feed identity 分字段核对。Electron
启动只 attach 到自己刚启动且 sidecar、健康投影一致的 worker，停止不确认时保留 `unconfirmed`。
deny-first fixture、source guard 与 GitHub Actions 已加入；真实 Electron/OS process-tree、跨重启
durability、升级兼容和 physical process e2e 仍未证明。

<a id="step-ui-26"></a>



#### UI-26 · Desktop workspace、托盘、通知与关闭策略　🔄

- 依赖：UI-08/19/24/25。代码：workspace switcher、tray/menu、notification bridge、close confirmation。
- 步骤：workspace open/new/continue 只发显式 command；通知仅由服务端终态/待办事实触发；关闭提示 pending/unknown，不把窗口状态等同 run 状态。
- 先拒绝：旧 workspace 数据串入新窗口、通知泄漏私有内容、close 隐式 cancel/resume/trust、托盘调用宽权限 API。
- 成功/回归：多窗口、托盘隐藏/恢复、OS notification permission、未保存草稿、pending approval、unknown invocation 和 reopen。
- 完成产物：desktop state machine、通知脱敏策略、关闭/托盘 e2e。

实现基线：[`ui26-desktop-workspace-tray-baseline.md`](ui26-desktop-workspace-tray-baseline.md)。当前 source
slice 新增 bounded desktop state reducer、workspace switch 时 attention reset、server-bound
terminal/pending notification fact、workspace binding/feed cursor/replay fence 和固定脱敏通知文案；
现有 tray/workspace typed intents 继续复用 `DaemonHost`，close 只显示 pending/unknown/draft attention
并保留 Keep/Quit/Cancel，不提交隐式 cancel/resume/trust/approve。Web SSE 只提交 schema 化且不含
用户正文、路径或 token 的事实，Electron 主进程是唯一 OS Notification 适配边界。

<a id="step-ui-27"></a>



#### UI-27 · Desktop 安全持久化和 detach　🔄

- 依赖：UI-04/25/26。代码：layout/session reference store、safe detach/reattach。
- 步骤：只持久化布局、草稿策略、instance/session 引用和最后已知 cursor；敏感 token 使用受控存储；detach 后重新 handshake，不自动执行。
- 先拒绝：把 access token 写明文配置、从 layout 恢复即 launch/resume/approve、旧 epoch 的 cursor 直接提交、私有 artifact 缓存无 TTL。
- 成功/回归：重启/升级/损坏 store/权限不足/多用户、旧实例、token rotation、detach/reattach。
- 完成产物：持久化 schema、迁移/清理策略、恢复证据和限制清单。

实现基线：[`ui27-desktop-persistence-baseline.md`](ui27-desktop-persistence-baseline.md)。当前 source
slice 将旧的裸 `desktop.json` 替换为 versioned `kiana.desktop-store.v1` metadata store，严格
限制 layout、draft policy、workspace/instance/session/cursor refs；写入使用同目录临时文件、0600
权限和 rename，读取拒绝 schema/unknown field/size/symlink/digest/secret 漂移。Web 只提交 server
bound session reference，Electron 重新校验 workspace binding 后再写入引用。

detach/reattach 只保留可诊断的旧 instance/session/cursor reference；`reattachPlan` 明确要求新
handshake，并把旧 cursor 标为 discard，绝不从 layout/store 自动 launch、resume、approve 或 trust。
继续上次工作区是用户显式点击后的新 worker attach，scratch 自动 trust 仅保留给“新建项目”动作。

<a id="step-ui-28"></a>



#### UI-28 · 共享静态资产、版本和生产打包　🔄

- 依赖：UI-20/23/24–27。代码：Web build、asset manifest、Electron packaging、CSP hash。
- 步骤：固定 schema/client/ui 版本和 asset hash；开发/生产 URL、base path、cache headers、source map 和 license；桌面包内不带 secret。
- 先拒绝：旧 bundle 连接新 protocol 无能力协商、缓存旧 JS 绕过 CSP、构建把 `.env`/token 打入产物、未签名/未校验资源运行。
- 成功/回归：clean build、离线 bundle、hash mismatch、upgrade/rollback、Electron package 和 loopback deployment smoke。
- 完成产物：可复现 build manifest、打包检查、CSP hash/asset integrity 证据。

实现基线：[`ui28-desktop-assets-packaging-baseline.md`](ui28-desktop-assets-packaging-baseline.md)。当前 source
slice 新增 `contrib/desktop/asset-manifest.json` 与 `lib/asset-manifest.js`，绑定 app/protocol/UI
版本、package asset 相对路径、bytes、SHA-256、immutable/no-store cache policy、license 和
runtime-nonce CSP forbid list；`scripts/verify-desktop-assets.js` 在构建前校验 Electron `files`
allowlist、资源 hash/size、symlink/secret marker 与 manifest drift。Web 仍使用 server-generated
nonce CSP，未打开 `unsafe-inline`/`unsafe-eval`；asset verifier 不接触 DaemonHost/Broker/runner。

<a id="step-ui-29"></a>



#### UI-29 · ACP/IDE session adapter　🔄

- 依赖：UI-01/02/07/18/21/25。代码：`kiana-entrypoints` 或独立 adapter 的 initialize/session/new/resume/prompt/update/permission/cancel。
- 步骤：将 ACP turn/update 映射到同一 UiSnapshot/UiFeed/UiAction；连接 handler 在 resume/replay 前安装；明确 v1/v2 capability 和 session ownership。
- 先拒绝：未知协议版本、外部 session 越权、permission 过期、cancel 抢先显示为最终成功、host tool 直接执行、update 乱序。
- 成功/回归：fake ACP peer 的 initialize/new/resume/prompt/permission/cancel、disconnect/replay/gap、旧版本协商和 late update。
- 完成产物：ACP mapping table、fixture peer、版本/错误转换表；live IDE 只在 opt-in 卡验证。

实现基线：[`ui29-acp-session-adapter-baseline.md`](ui29-acp-session-adapter-baseline.md)。当前 source
slice 位于 `kiana-client/src/acp.rs`，支持 `acp.v1`/`acp.v2` 协商、owner/authority epoch 绑定的
session new/resume、feed handler 在 replay 前安装、prompt/permission/cancel 到现有
`UiActionV1` 的 typed intent 映射，以及 `UiSnapshotV1`/`UiFeedFrameV1`/`UiActionV1` schema 校验。
sequence gap 返回 hydrate/replay 状态，terminal 后 late update、foreign owner、过期 permission 和
旧 epoch fail-closed；adapter 不拥有 host tool、Broker、runner 或直接 effect。

<a id="step-ui-30"></a>



#### UI-30 · IDE editor/terminal capability boundary　🔄

- 依赖：UI-04/07/29、Capability 专项。代码：editor/terminal adapter 的 capability request/permit bridge。
- 步骤：将打开文件、读取、patch preview、apply、终端输出映射为受控 Artifact/UiAction；editor host 只提供显示/输入，真正 effect 回 ControlPlane。
- 先拒绝：任意路径、隐式保存、编辑器扩展绕过 permit、终端 spawn、旧 revision apply、remote workspace 混入本地 scope。
- 成功/回归：只读/写入/拒绝/过期 permit、外部修改、diff digest、terminal cancel/unknown、host 重启。
- 完成产物：IDE capability matrix、fake editor fixture、越权/TOCTOU 证据。

实现基线：[`ui30-ide-capability-baseline.md`](ui30-ide-capability-baseline.md)。当前 source slice 位于
`kiana-client/src/ide_capability.rs`：host capability 只声明 `delegated_to_kiana=true` 且
`direct_effect=false`；relative path、workspace/path digest、revision 和 permit owner/expiry/effect
在 client intent 形成前严格绑定。open/read/patch preview/terminal output 是 query intents；apply 和
terminal cancel 只有匹配的未过期 permit 才产生 `UiActionV1`，host 不读文件、不保存、不 spawn terminal。

### 27.5 跨入口一致性、恢复和质量门

<a id="step-ui-31"></a>



#### UI-31 · CLI/Workbench/Web/Desktop 行为 parity　🔄

- 依赖：UI-10–30 中各入口可运行。代码：共享 operation matrix、cross-surface harness。
- 步骤：用同一 fixture 依次执行 open/status/run/approval/cancel/resume/receipt/export；比较 protocol action、最终 cursor/revision、error code 和 Receipt，而不是比较文案。
- 先拒绝：某入口绕过 capability、用不同 retry policy、把断线/关闭改写成业务状态、结果字段缺失或敏感字段增多。
- 成功/回归：同一 command ID 在各 surface 查询结果一致；surface-specific presenter 只改变显示和输入方式。
- 完成产物：parity matrix、跨入口 trace diff、差异解释和未支持能力清单。

实现基线：[`ui31-surface-parity-baseline.md`](ui31-surface-parity-baseline.md)。当前 source slice 位于
`kiana-client/src/surface_parity.rs`，要求四个入口提交同一 command/operation/disposition/retry、
cursor/epoch、revision、receipt/error identity；surface-specific limitation 只做集合并集，不改变
服务端结果。缺 surface、重复 surface、敏感字段、字段缺失或任何 identity/retry drift 都 fail-closed，
comparator 只读，不发起 retry、resume、capability 或 effect。

<a id="step-ui-32"></a>



#### UI-32 · deny-first 安全路径集成测试　🔄

- 依赖：UI-04/16/19/21/23/24/30/31。代码：entrypoint/daemon/core integration tests。
- 步骤：覆盖未授权、越权 session、错误 token/origin/sender、过期 approval、stale cursor/revision、digest mismatch、scope widening、cancel race、unknown effect、注入。
- 先拒绝：每个拒绝必须无 effect、无错误副作用，或明确记录 Unknown/incident；测试检查 effect count、journal 和 audit event。
- 成功/回归：拒绝原因稳定、UI 可恢复、CLI exit code 正确、Web/desktop 不泄漏；再执行对应 happy path。
- 完成产物：deny matrix、effect-count fixtures、审计/receipt 断言。

实现基线：[`ui32-deny-first-baseline.md`](ui32-deny-first-baseline.md)。当前 CI-only matrix 覆盖
foreign session、token/origin/sender、expired approval、stale cursor/revision、digest/scope widening、
cancel race/Unknown 和 indirect injection；每个 deny case 要求 stable error、effect_count=0 或明确
Unknown/reconcile follow-up。source guard 绑定 Web action claim/owner/auth mutation、CLI/Workbench
cancel/parity 与 `DaemonHost`/`ControlPlane`/`EventLog`/`result_unknown` 共享脊柱，不把拒绝 fixture
改成 happy path。

<a id="step-ui-33"></a>



#### UI-33 · reconnect/replay/gap/crash recovery e2e　🔄

- 依赖：UI-05–08、UI-18、UI-25、Event/Receipt §23。代码：daemon restart + browser/PTY/Desktop/ACP harness。
- 步骤：在 snapshot、feed、action accepted、artifact fetch、cancel 和 terminal 各阶段注入断网/kill/延迟/重复；重启后查询原 command、重建 projection、fence 旧 epoch。
- 先拒绝：丢响应产生第二 effect、gap 静默继续、terminal 重复、旧 worker 仍写入、恢复自动 approve/resume/retry。
- 成功/回归：状态最终收敛到 Applied/Rejected/Cancelled/Failed/Unknown；Unknown 有显式下一步和 limitation。
- 完成产物：故障时间线、重启 fixture、recovery Receipt 和未证明的跨进程边界。

实现基线：[`ui33-recovery-baseline.md`](ui33-recovery-baseline.md)。当前 source slice 位于
`kiana-client/src/ui_recovery.rs`，将 snapshot/feed/action accepted/artifact/cancel/terminal 与
disconnect/kill/delay/duplicate/gap/old epoch 映射为只读 `RecoveryPlan`；Accepted 只 query original
command，gap/old epoch 要求 hydrate，terminal 只 replay，cancel/worker uncertain 进入 reconcile
Unknown，所有计划 `new_effect_allowed=false`。`RecoveryFence` 拒绝旧 epoch、跳号和 terminal 后迟到
更新，不隐式 retry/resume/approve。

<a id="step-ui-34"></a>



#### UI-34 · 性能、资源上限和可访问性验收　🔄

- 依赖：UI-13/20/23/28/33。代码：metrics、bounded queue/cache/render budget、accessibility runner。
- 步骤：为 feed、snapshot、artifact、DOM、TUI buffer、IPC payload 定义上限和退化行为；测首屏、长 stream、100+ session、超长 diff、慢磁盘、低带宽。
- 先拒绝：无界内存、慢消费者拖住 EventStore、丢弃 pending/unknown、超限静默截断、焦点/读屏回归。
- 成功/回归：压力下仍有可见 Degraded/limited 状态和可恢复 action；记录 p50/p95、RSS、queue depth、bundle size。
- 完成产物：性能基线、资源预算、无障碍人工/工具报告。

实现基线：[`ui34-resource-budget-baseline.md`](ui34-resource-budget-baseline.md)。当前 source slice 位于
`kiana-client/src/ui_budget.rs`，统一 Feed/Snapshot/Artifact/DOM/TTY/IPC 的 bytes/items/sessions/
queue 上限；硬 bytes/queue 超限返回 Reject，item/session 超限返回可见 Degraded，pending/unknown
保护项超过容量直接 fail-closed，禁止静默淘汰或把预算拒绝改成 cancel/retry/approval。

<a id="step-ui-35"></a>



#### UI-35 · 旧 Web/CLI 迁移与兼容收口　🔄

- 依赖：UI-10–23、UI-31。代码：旧 inline Web、旧 endpoint、旧 CLI 参数的 adapter/feature flag。
- 步骤：逐路由/命令迁移到 typed client；保留必要兼容窗口和明确 deprecation；双读比较 projection，不双写事实；删除旧路径前记录使用/测试证据。
- 先拒绝：兼容层重新执行副作用、旧参数扩大 scope、双写造成两个 command、旧缓存污染新 epoch/schema。
- 成功/回归：新旧入口同一 fixture parity；禁用 flag 后旧请求返回可操作迁移错误；不存在第二执行循环。
- 完成产物：迁移表、flag 生命周期、兼容窗口和删除前 checklist。

实现基线：[`ui35-legacy-migration-baseline.md`](ui35-legacy-migration-baseline.md)。当前 source slice 位于
`kiana-client/src/legacy_migration.rs`，将旧 Web `/api/state`、`/api/events` 和旧 CLI run/cancel/
receipt 映射到 typed snapshot/feed/action/receipt query 名称，全部标记 Deprecated、requires typed
client、writes_facts=false；未知/注入 route fail-closed。该 adapter 只返回迁移 metadata，不双写
EventLog、不自行 retry、不启动第二执行循环。

<a id="step-ui-36"></a>



#### UI-36 · 生产构建、安装和发布前 smoke　🔄

- 依赖：UI-28/34/35。代码：Web static build、Electron artifact、CLI distribution scripts。
- 步骤：执行 clean/offline 构建、版本/asset/schema 检查、loopback launch、桌面 attach、CLI pipe、Workbench smoke；确保日志和包不含 secret。
- 先拒绝：构建引用工作树绝对路径、未锁依赖、旧 protocol、Electron 远程导航、安装后默认 trust/resume。
- 成功/回归：空目录安装、升级/回滚、无网络、权限不足、端口占用、daemon crash；每个失败有诊断和退出码。
- 完成产物：构建 manifest、安装 smoke 回执、artifact hash 和已知限制。

实现基线：[`ui36-release-gate-baseline.md`](ui36-release-gate-baseline.md)。当前 source slice 新增
GitHub-only `scripts/verify-ui36-release-gate.sh`：非 CI 环境 fail-closed 为 `remote_ci_required`，
CI 才执行 desktop asset manifest、Cargo.lock/format、`cargo build --bin kiana --locked` 和 package/
script secret marker scan；脚本不安装、信任、resume、approve、启动第二 loop 或直接调用 Broker。

<a id="step-ui-37"></a>



#### UI-37 · 用户文档、模块图和操作 runbook　🔄

- 依赖：UI-31–36。代码/文档：`module-map.md`、`USER.md`、入口帮助、`docs/company-os-ui-ux.md`。
- 步骤：更新真实调用关系、命令/错误/状态/恢复流程、token/来源边界、CLI/Workbench/Web/Desktop 差异；将目标与已实现明确分开。
- 先拒绝：把类型写成 implemented、把一次本地通过写成 durable/live、把 reference 行为写成 Kiana 事实、遗漏限制。
- 成功/回归：从干净 checkout 按文档启动并完成最小 deny/happy/reconnect；链接、编号和命令检查通过。
- 完成产物：用户 runbook、module-map 入口图、迁移 FAQ 和限制/证据索引。

实现基线：[`ui37-docs-runbook-baseline.md`](ui37-docs-runbook-baseline.md)。新增
[`docs/ui-entrypoints-runbook.md`](../ui-entrypoints-runbook.md)，更新 `docs/module-map.md` 的真实
入口→client/protocol→DaemonHost→ControlPlane→Broker/EventLog/Receipt 关系，明确 CLI/TTY/Web/
Desktop/ACP-IDE 的当前 command、deny/recovery/Unknown、feature/proof 口径和限制；
`scripts/verify-ui37-docs.sh` 与 GitHub workflow 只检查链接/关键边界/overclaim，不把文档检查当成
runtime/durable/live/physical 证明。

<a id="step-ui-38"></a>



#### UI-38 · 协议/入口 conformance 集成门　🔄

- 依赖：UI-01–37。代码：统一 conformance runner，覆盖 Rust client、CLI、Web、Desktop、ACP fake peer。
- 步骤：对每个 surface 执行 schema decode、capability、snapshot/feed cursor、action disposition、error/retry、artifact digest、receipt reference 检查；生成可比较 JSON trace。
- 先拒绝：未声明 schema/epoch、重复/跳号、错误 retry、unknown 被吞、敏感字段泄漏、surface 特例绕过协议。
- 成功/回归：同一 fixture 的事实 trace 一致，差异仅在 presenter；失败输出最小复现 cassette。
- 完成产物：conformance suite、fixture registry、CI gate 和差异报告格式。

实现基线：[`ui38-conformance-baseline.md`](ui38-conformance-baseline.md)。当前 source slice 位于
`kiana-client/src/conformance.rs`，在既有四 surface parity comparator 之上统一校验 protocol/UI/
capability schema、command/cursor/feed sequence、action disposition/retry、artifact/receipt digest、
Unknown 可见性和敏感字段计数；所有失败 fail-closed，comparator 只消费 trace，不 retry、cancel、
resume、approve 或执行 capability effect。fixture 与 GitHub Actions 只在远程验证，不把 CI 未观察
结果升级为 local_behavior/durable/live/physical proof。

<a id="step-ui-39"></a>



#### UI-39 · live ACP/IDE opt-in 验证　🔄

- 依赖：UI-29/30/33/38。代码/fixture：显式 opt-in 的本地 ACP/IDE 连接脚本。
- 步骤：固定协议版本、workspace、能力和脱敏数据，验证 initialize/session/prompt/update/permission/cancel/reconnect；记录外部 host 版本。
- 先拒绝：未 opt-in 的网络/远端 provider、host 直接写文件/执行终端、live 结果替代 deny fixture、不可重现环境被写成 durable。
- 成功/回归：live 只提升到声明的 `live` 上限；断开、旧版本、permission timeout 和 host 重启仍回到本地 recovery 语义。
- 完成产物：live cassette、host/version/environment、上限与限制；无 live 环境时保持 ⏳。

实现基线：[`ui39-live-acp-baseline.md`](ui39-live-acp-baseline.md)。当前 source slice 位于
`kiana-client/src/live_acp.rs`，`LiveAcpOptIn` 强制 IDE surface、ACP v1/v2、approval ref、local
transport、redacted payload、环境/工作区 digest 和 host capability delegation；`LiveAcpSession`
只包装既有 `AcpSessionAdapter`，记录 initialize/session/prompt/update/permission/cancel/reconnect
证据，断线/gap 保持 Unknown/reconcile。它不打开 socket、不 spawn、不接 provider/Broker、不 mint
permit、不执行 host capability；`scripts/verify-ui39-live-acp-opt-in.sh` 只做本地 transport/
approval/redaction/digest preflight，也不启动 external host 或把 source/CI 结果自升为 live proof。

<a id="step-ui-40"></a>



#### UI-40 · 发布门与证据收口　🔄

- 依赖：UI-32–39、Event/Receipt §23。代码/脚本：聚焦 cargo tests、entrypoint tests、desktop tests、smoke 和文档校验。
- 步骤：先运行 deny/recovery，再 happy/parity/performance；按改动 crate 串行 daemon/control-plane 测试；保存命令 argv、匹配测试数、环境、fixture digest 和 exit code。
- 先拒绝：基线失败混入本次结果、`0 tests`、忽略/放宽断言、网络偶然可用、一次人工观察冒充 durable/physical。
- 成功/回归：必要 checks、`git diff --check`、schema/link/id 唯一性、release smoke 全部有回执；更新状态仅覆盖真实行为。
- 完成产物：UI evidence bundle、failure classification、CURRENT_STATUS 候选证据块（待 reviewer 采纳）。

实现基线：[`ui40-release-gate-baseline.md`](ui40-release-gate-baseline.md)。当前 GitHub-only
workflow 先运行 UI-32 deny、UI-33 recovery，再运行 UI-31 parity、UI-34 resource、UI-38 conformance、
UI-39 protocol/client guards 和 UI-40 typed evidence；随后复用 UI-36 release build/asset gate、
diff check 与 workspace compile。`UiEvidenceCase/Bundle` 绑定 source/fixture/environment/argv/exit/
receipt/artifact/proof/limitation/reviewer；source/CI 结果仍不能自升为 durable/live/physical。

<a id="step-ui-41"></a>



#### UI-41 · 交接、审查和后续缺口　🔄

- 依赖：UI-40。代码/文档：roadmap、module-map、CURRENT_STATUS、实现 PR/工作树说明。
- 步骤：列出完成/部分/延期/不支持的 UI 卡，绑定 P/P2/P4、Event/Receipt、Capability、Harness 单元；指出跨进程、物理 PTY、live provider、规模和浏览器兼容上限。
- 先拒绝：没有 reviewer、没有 source snapshot、把 WIP agent 修改视为验收、发现规范与源码冲突却改规范消除冲突。
- 成功/回归：独立 reviewer 能从 evidence bundle 重跑关键拒绝与恢复路径；未完成项有下一步和阻塞事实，不用“整体完成”覆盖细节。
- 完成产物：交接清单、审查意见、最终证据索引和下一批明确任务。

实现基线：[`ui41-handoff-baseline.md`](ui41-handoff-baseline.md)。当前 handoff matrix 绑定 UI-38
conformance、UI-39 opt-in、UI-40 `UiEvidenceBundle`、P/P2/P4 与 Event/Receipt/Capability/Harness
限制，逐项写出 `feature_status`、`proof_level`、source snapshot、reviewer、receipt/limitation 和
next action；UI runtime、跨进程 durable、Desktop/browser/PTY、provider/live/physical 仍保留
partial/not_supported，不用“整个项目完成”覆盖缺口。GitHub workflow 复核 UI-40 source gate 与
handoff guard，本地不运行测试。

## 28. UI / Entrypoints 执行批次、验证命令与证据规则

### 28.1 依赖批次和并行边界

```text
UI-00
  → UI-01 → UI-02 → UI-03 → UI-04 → UI-05 → UI-06 → UI-07 → UI-08 → UI-09
  → UI-10 → UI-11 → UI-12 → UI-13 → UI-14 → UI-15
  → UI-16 → UI-17 → UI-18 → UI-19 → UI-20 → UI-21 → UI-22 → UI-23
  → UI-24 → UI-25 → UI-26 → UI-27 → UI-28 → UI-29 → UI-30
  → UI-31 → UI-32 → UI-33 → UI-34 → UI-35 → UI-36 → UI-37 → UI-38
  → UI-39 → UI-40 → UI-41
```

UI-00 完成后，协议研究可以并行准备 UI-01/09 的 schema fixture；UI-05 之前不得由各入口自行拼 snapshot。UI-10–15（CLI/Workbench）与 UI-16–23（Web）在 UI-07/08 稳定后可并行实现，但共享 reducer、错误、action 和 cursor 只能由一处维护。UI-24–28（Desktop）依赖 Web 的可 attach contract；UI-29/30（IDE）可以并行写 fake peer，但不得在 UI-04 之前接入真实 effect。UI-31–34 只在各 surface 具备同一协议 trace 后开始；UI-39 必须晚于 deny/recovery 集合。任何卡失败时，取消依赖它的后续卡，保留独立研究 fixture，不以替代路径“先跑通”为完成。

### 28.2 聚焦验证命令

以下是按改动选择的命令模板；测试目标不存在时先增加 RED 测试，不能用无匹配的 `0 tests` 当通过。daemon/control-plane 相关测试串行执行。

```bash
# 协议、client、入口单元/集成
cargo test -p kiana-protocol --locked --offline
cargo test -p kiana-client --locked --offline
cargo test -p kiana-entrypoints --test cli_help --locked --offline
cargo test -p kiana-entrypoints --test cli_workbench --locked --offline -- --test-threads=1
cargo test -p kiana-entrypoints --test cli_web --locked --offline -- --test-threads=1

# daemon/core 的 cursor、action、recovery（按实际目标过滤）
cargo test -p kiana-daemon --locked --offline -- --test-threads=1
cargo test -p kiana-core --locked --offline -- --test-threads=1

# Electron/desktop
node --test contrib/desktop/tests/*.js

# 文档和静态检查
git diff --check -- docs/roadmap.md docs/ui-entrypoints-design-research.md
cargo fmt --all --check
cargo check --workspace --locked --offline
```

发布/冒烟阶段按改动选择既有 `scripts/v10-workbench-smoke.sh`、`scripts/v10-p0-closeout-smoke.sh`、`scripts/release-smoke.sh`；只有涉及产品发布或物理桌面时才执行对应脚本，不把 smoke 的单次通过升级为 durable/live/physical。需要浏览器/PTY 的 UI-23、UI-33、UI-34 应保存屏幕/日志/内存指标和实际浏览器、终端版本；无法运行时明确记录限制。

### 28.3 每张 UI 卡的证据块

```text
step: UI-xx; linked module/P/ER/CM/CAP units: ...
source_snapshot: exact commit plus relevant WIP file hashes
worktree_status: changed paths and concurrent/unrelated changes
command_argv: exact command, filter, matched test count, test-thread setting
cwd·environment: OS/kernel, Rust/Node/browser/desktop versions, feature flags,
                 transport, workspace, sandbox, quotas and provider mode
fixture·cassette: protocol/event/action/artifact/PTY/ACP fixture hash and fault point
exit_code: each command; baseline failures separated from this step
status change: feature_status before -> after for exercised behavior only
proof-level change: source/local_behavior/durable/live/physical, exactly as observed
limitations: unsupported browser/backend, projection lag, data bounds, no-live scope
reviewer: named reviewer; self-review marked as self-review
```

`feature_status` 和 `proof_level` 必须分开填写。类型、schema、静态截图只能提供 `source`；本地服务行为才可到 `local_behavior`；跨进程重启/重算和真实 host 分别需要 `durable`/`live`；PTY、安装包或真实桌面交互才可到 `physical`。Receipt 存在只证明投影生成，不证明现实文件或外部 effect 正确。

### 28.4 研究来源、边界和回填规则

实现 agent 应先阅读 [UI 调研记录](../ui-entrypoints-design-research.md)、[module-map.md](../module-map.md) 第 9 模块、`CURRENT_STATUS.md` 和对应 crate 源码；外部机制核对使用 ACP 的 session/prompt/cancel 文档、WHATWG SSE、Electron Security、VS Code Webview/UX、WCAG 2.2、Zed External Agents 及 React/Vite 构建说明。外部资料说明协议或平台行为，不是 Kiana 运行证据。

每完成一张卡，只回填它真实覆盖的 P/P2/P4、Event/Receipt、Capability、Harness 或 CompanyOS 单元；同时更新 module-map 的**实际**入口路径和本账本限制。若现状与规范冲突，记录 source snapshot、失败现象和候选修复，不改规范来迁就代码。若实施 agent 声称完成但缺少拒绝路径、原 command 查询、重启/重连、来源或 reviewer，保持 `⏳/🔄`。

### 28.5 本专项边界

```text
source_snapshot: db77c2485bcafecbb1da17ec57ee509ad2ee32b4 + shared WIP
source_capture: 2026-09-12；reference 73 个一级目录全量盘点，重点入口定向源码阅读
worktree_status: 既有多 crate、roadmap/CURRENT_STATUS 与其他 agent WIP；本专项只追加 roadmap 和研究文档
command_argv: rg/sed/python inventory、源码阅读、官方资料检索；未执行模型、provider 或 reference 安装/测试
cwd·environment: 仓库根；Linux/bash；外部资料访问日 2026-09-12；无 live IDE/桌面 host
fixture·cassette: 无产品行为 cassette；研究临时 hash 位于 /tmp，不作为仓库事实
exit_code: 文档追加和静态 diff 检查通过；产品测试由实施卡按改动执行
status change: 新增 UI-00–UI-41 设计步骤；不提升任何既有 feature status/proof level
proof-level change: none；source research/design only
limitations: 未逐行审计全部 reference；本地 reference 可能落后上游；跨进程 durable、live IDE、物理桌面、浏览器矩阵和大规模性能待对应卡实测
reviewer: Codex 自审；需实施 agent/独立 reviewer 在 UI-40/41 复核
```

---

返回：[路线图总图与当前窗口](../roadmap.md#appendix-navigation) · [文档总入口](../README.md)
