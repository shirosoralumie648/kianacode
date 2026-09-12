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



#### UI-00 · 建立入口基线与验收矩阵　⏳

- 依赖：无。代码/文档：`docs/module-map.md`、`CURRENT_STATUS.md`、`kiana-entrypoints` 与 `contrib/desktop` 测试清单。
- 步骤：从新的 source snapshot 开始，记录 CLI/Workbench/Web/Desktop 的启动、session attach、run、approval、cancel、resume、receipt、export 路径；把既有测试按 deny/happy/reconnect/recovery/physical 分类，标出 baseline failure。
- 先拒绝：未授权 workspace、错误 token/origin、未知命令、过期 cursor/revision、非 owner action、响应丢失和旧 epoch 都要有明确预期。
- 成功/回归：画出每个入口到 `DaemonHost` 的调用图；现有通过测试逐项绑定到合同，未覆盖处生成 RED 测试名，不把 `0 tests` 记为通过。
- 完成产物：`UI-00` 基线表、覆盖矩阵、稳定快照 hash、并行 agent 交接说明；不修改 feature status。

<a id="step-ui-01"></a>



#### UI-01 · 定义 versioned UI protocol DTO　⏳

- 依赖：UI-00。代码：`kiana-protocol` 新增 snapshot/feed/action/capability/error/artifact DTO，保留旧 wire envelope 的兼容解码。
- 步骤：定义 schema/version、instance/epoch/cursor/revision、Session/Run/Turn/Item、HumanActionCard、ReceiptRef、UiNotice 和 limitations；未知字段可保留或安全丢弃并报告版本。
- 先拒绝：缺字段、重复 ID、非法 sequence、跨 instance、过大 payload、无 digest 的文件审批和伪造 actor/scope 必须结构化拒绝。
- 成功/回归：对 JSON/MessagePack（若使用）做 round-trip、旧 envelope 兼容、schema mismatch 和大小边界测试；生成 protocol fixture。
- 完成产物：协议类型、schema 文档/fixture、稳定错误码表及迁移说明。

<a id="step-ui-02"></a>



#### UI-02 · 统一错误、能力和 surface handshake　⏳

- 依赖：UI-01。代码：`kiana-protocol`/`kiana-client` 的 `StableError`、`UiCapability`、initialize/health handshake。
- 步骤：将 Input/PolicyDenied/ApprovalRequired/Conflict/Capacity/Unavailable/Cancelled/Failed/Unknown/Persistence 映射到机器码、用户文案、retry disposition 和 remediation；能力按 principal、workspace、surface、feature version 求交集。
- 先拒绝：未知错误码、能力越权、把 `Unknown`/`PolicyDenied` 映射成成功或可自动重试；health 不泄漏路径/token/内部异常。
- 成功/回归：每个 surface 生成相同 deny matrix；能力缺失时按钮/CLI action 不出现，直接构造请求仍被服务端拒绝。
- 完成产物：错误目录、能力 handshake fixture、文案与 exit-code 映射表。

<a id="step-ui-03"></a>



#### UI-03 · 本地实例身份、发现和 transport　⏳

- 依赖：UI-01/02。代码：`kiana-daemon`、`kiana-client` 的 in-process/Unix socket/named pipe adapter 与 instance lock。
- 步骤：为 daemon 生成 instance ID、authority epoch、socket/pipe 权限和 ready record；CLI/Workbench 优先复用已有实例，安全地启动/连接并探测协议版本。
- 先拒绝：旧 socket、PID 重用、不同 workspace、未知 peer、权限过宽的 socket、重复实例和 epoch 不一致必须失败关闭。
- 成功/回归：启动、attach、daemon 重启、并发客户端、半开连接、协议降级和清理锁文件测试；transport 只传请求，不执行副作用。
- 完成产物：发现记录格式、transport contract、instance/epoch fixture 和诊断命令。

<a id="step-ui-04"></a>



#### UI-04 · 动作 CAS、idempotency 与响应丢失　⏳

- 依赖：UI-01–03；串接 Event/Receipt §23。代码：ControlPlane action facade、command journal、payload digest。
- 步骤：为每个 action 绑定 command ID、idempotency key、target、expected epoch/cursor/revision、owner/scope、deadline；持久化 Accepted/Applied/Rejected/Unknown 及原始 digest。
- 先拒绝：重复 key 不得重复 effect；digest 不同、owner 不同、revision 过期、取消中、旧 epoch、超时和无 permit 必须拒绝或标 Unknown。
- 成功/回归：模拟 ACK 丢失、进程崩溃、重复请求、并发 CAS 和新旧 payload；query original 能收敛到同一结果，禁止换 ID 自动重做。
- 完成产物：action journal/adapter、重投策略、effect-count fixture 和 durable proof 记录。

<a id="step-ui-05"></a>



#### UI-05 · 原子 snapshot projector 与分页　⏳

- 依赖：UI-01–04、Event/Receipt §23。代码：`kiana-daemon` projector/query facade。
- 步骤：从 EventStore 单一 cursor 生成 `UiSnapshot`；为 session/run/action/artifact/receipt 提供稳定排序、分页 cursor、retention 和 limitations；保存 projector health/lag。
- 先拒绝：非法 page cursor、越权 session、被 retention 保护的待办被淘汰、projector lag 被伪装成空列表、跨 cursor 拼接的 snapshot。
- 成功/回归：新进程重建、重复读取、分页边界、空集合、projector 故障和恢复测试；snapshot cursor 可回放且不改变事实。
- 完成产物：snapshot contract、projector tests、分页/保留策略和 read-model rebuild 命令。

<a id="step-ui-06"></a>



#### UI-06 · feed cursor、gap、replay 与背压　⏳

- 依赖：UI-05。代码：daemon feed facade，复用 `RunStreamEnvelope` 但补齐 instance/epoch/schema/gap 语义。
- 步骤：定义 snapshot boundary、feed sequence、Last-Event-ID/after cursor、replay window、heartbeat、慢消费者上限；区分 duplicate、gap、old epoch、terminal、unknown。
- 先拒绝：序列跳跃、错误 epoch、过期 replay、无界队列、慢客户端阻塞事实提交、terminal 后伪造 delta。
- 成功/回归：断线重连、重复事件、跨页、慢消费者、服务器重启、terminal retention 和 gap→rehydrate；sequence 由服务端决定，不能按 timestamp 排序。
- 完成产物：feed state machine、fault fixture、背压指标和 replay 文档。

<a id="step-ui-07"></a>



#### UI-07 · typed client query/feed/action API　⏳

- 依赖：UI-02–06。代码：`kiana-client` 拆出 QueryClient、FeedClient、ActionClient、ArtifactClient，统一 request ID/deadline/cancel。
- 步骤：提供 initialize、snapshot、history、command status、subscribe/resume、artifact page、submit/cancel/continue；listener 可释放并隔离迟到回调。
- 先拒绝：未初始化、跨 workspace target、未知 schema、重复 listener、错误 command 重投、取消后继续回调到旧 controller。
- 成功/回归：typed mock transport 覆盖 timeout、late response、reconnect、gap、Unknown query 和 listener cleanup；不把 HTTP 200 直接当 Applied。
- 完成产物：client traits、mock transport、请求生命周期图和调用迁移清单。

<a id="step-ui-08"></a>



#### UI-08 · 共享 reducer/entity store　⏳

- 依赖：UI-05–07。代码：`kiana-entrypoints` 或共享 adapter 的纯函数 reducer/store。
- 步骤：按 entity ID/revision 合并 snapshot/feed；分别存 run、connection、submission、draft、inbox、artifact viewer；固定 optimistic 状态只能短暂存在且可回滚。
- 先拒绝：旧 revision 覆盖新值、跨 session 串数据、event duplicate 产生两条 item、cache eviction 丢 pending/unknown、乐观成功掩盖服务端拒绝。
- 成功/回归：乱序/重复/gap/epoch reset、tab isolation、hydrate/dehydrate、内存上限和 reducer purity 测试。
- 完成产物：共享 store、状态图、fixture reducer trace；presenter 不再自建事实状态。

<a id="step-ui-09"></a>



#### UI-09 · schema 资产、生成和兼容门　⏳

- 依赖：UI-01/07/08。代码：schema/fixture 目录、生成脚本、版本检查 CI。
- 步骤：决定 JSON Schema/TypeScript/Rust 生成边界；为每个 DTO 固定 examples、unknown-field、max-size 和 deprecated 字段；生成静态客户端类型。
- 先拒绝：schema 与 Rust 类型漂移、同版本破坏性改动、生成文件未更新、示例包含 secret/真实路径。
- 成功/回归：schema diff、跨语言 round-trip、旧客户端读取新服务端、服务端拒绝新客户端不可理解命令。
- 完成产物：schema lock、生成校验命令、兼容矩阵和脱敏 fixtures。

### 27.2 CLI 与 Workbench

<a id="step-ui-10"></a>



#### UI-10 · CLI 命令和输出归一化　⏳

- 依赖：UI-07–09。代码：`kiana-entrypoints/src/cli.rs`（可按用户本节约定拆分）、command registry/dispatcher。
- 步骤：统一 `run/status/events/approve/deny/cancel/resume/receipt/export/session` 的参数、workspace/session 解析和 command ID；每个命令只调用 client facade。
- 先拒绝：缺 workspace、未知 session、没有 TTY 却请求交互、不可恢复命令使用隐式 retry、输出混入 token/secret。
- 成功/回归：旧 CLI 参数兼容或给迁移错误；命令到 protocol 的映射表覆盖 JSON、TTY、静默模式。
- 完成产物：CLI command contract、参数错误目录、帮助文本和迁移 fixture。

<a id="step-ui-11"></a>



#### UI-11 · CLI JSON/TTY/exit code presenter　⏳

- 依赖：UI-02/10。代码：CLI presenter、稳定 exit code 和 stderr/stdout 分流。
- 步骤：JSON 输出只写 schema DTO；TTY 输出可读但不成为事实；错误/警告/receipt 分流；Unknown、Cancelled、PolicyDenied 各有稳定 code。
- 先拒绝：JSON 中混 ANSI、stdout 打日志破坏管道、Unknown 返回 0、结构化 error 被字符串吞掉、超长 artifact 无界打印。
- 成功/回归：pipe/no-TTY、locale、SIGINT、broken pipe、分页、输出上限和 shell exit-code 测试；相同 action 在 Web/CLI 结果一致。
- 完成产物：golden outputs、exit-code 表和 `--json/--quiet` 文档。

<a id="step-ui-12"></a>



#### UI-12 · TTY 输入状态机　⏳

- 依赖：UI-08/10。代码：Workbench/CLI input layer；可参考 Pi 的跨 chunk parser 和 Codex composer。
- 步骤：解析 Unicode、IME、bracketed paste、历史、逐行/多行、resize、EOF、SIGINT；输入草稿与已提交 command 分离。
- 先拒绝：半截 escape sequence 执行命令、粘贴内容触发 shell、取消键改写已提交 action、超长输入内存失控。
- 成功/回归：PTY 分块注入、中文/组合字符、粘贴、窗口 resize、历史回退、Ctrl-C/Ctrl-D 和无 TTY 模式。
- 完成产物：输入状态图、PTY fixture、边界上限和取消语义。

<a id="step-ui-13"></a>



#### UI-13 · Workbench 时间线与结果渲染　⏳

- 依赖：UI-06/08/12。代码：`workbench_chat.rs`、`stream_render.rs` 及 presenter。
- 步骤：将 Delta/Terminal/Usage/ToolCall/Approval/Artifact/Error 映射为稳定 Item；实现折叠、展开、加载、未知和限制标记；过滤 ANSI/OSC 和不可信 markdown。
- 先拒绝：按到达时间覆盖事实、tool result 伪装 assistant text、错误被清屏、无限 delta/JSON/diff 渲染。
- 成功/回归：乱序/重复/gap、超长输出、二进制 MIME、terminal、approval 和 terminal 后 event；渲染性能与内存有界。
- 完成产物：Workbench render snapshots、item kind 表和结果截断策略。

<a id="step-ui-14"></a>



#### UI-14 · Workbench controller 与命令面板　⏳

- 依赖：UI-07/08/10–13。代码：controller、keymap、session switcher、command palette。
- 步骤：实现 open/attach/new/continue/status/run/cancel/resume/receipt；controller 只把用户意图翻译成 UiAction，提交状态与 run 状态分离。
- 先拒绝：切换 session 误提交旧草稿、重复快捷键产生两个 command、关闭窗口自动 cancel/resume、无 capability 显示 action。
- 成功/回归：多 session、重复 submit、断线、响应丢失、stale card、SIGTERM 和重启后恢复测试。
- 完成产物：controller state machine、keymap 文档、action audit trace。

<a id="step-ui-15"></a>



#### UI-15 · Workbench inbox、Diff 和 Receipt　⏳

- 依赖：UI-05/08/13/14、Event/Receipt §23。代码：inbox panel、artifact viewer、receipt view。
- 步骤：展示 reason/scope/expiry/fields/allowed decisions；Diff 由服务端 Artifact ref 提供并校验 digest/revision；Receipt 显示 files/cost/unknown/limitations/provenance。
- 先拒绝：过期 approval、revision mismatch、客户端自行改审批 payload、artifact digest 错、ResultUnknown 画成绿色完成。
- 成功/回归：approve/deny/edit/restore、部分文件、分页 artifact、撤销、receipt 重算和终态回放。
- 完成产物：Workbench UX fixture、receipt golden、审阅错误恢复说明。

### 27.3 Web、SSE 和多标签页

<a id="step-ui-16"></a>



#### UI-16 · Web 路由、来源校验和最小健康信息　⏳

- 依赖：UI-03/07/11。代码：`kiana-entrypoints/src/web.rs`（可拆分 router/auth/handlers）。
- 步骤：按 health/state/sessions/events/run/cancel/trust/sandbox/session/receipt/approval/resume/command 分类；统一 loopback Host/Origin/token 校验和请求大小/速率上限。
- 先拒绝：缺 token 的 state/events/action、任意 Origin、路径穿越、跨 workspace session、health 泄漏绝对路径或内部错误；不新增浏览器直连执行器。
- 成功/回归：每路由 deny-first、OPTIONS/GET/POST 方法、错误码、token rotation、过大 body、并发 tab 和日志脱敏。
- 完成产物：route/auth matrix、安全 header/CSP 草案、迁移旧 endpoint 的兼容表。

<a id="step-ui-17"></a>



#### UI-17 · Web snapshot hydrate、历史和分页　⏳

- 依赖：UI-05/08/16。代码：Web bootstrap、session/history/artifact query。
- 步骤：首屏先 hydrate snapshot，再开启 feed；历史按 server cursor 分页，缓存带 instance/epoch/schema；loading、empty、partial、limited 状态明确。
- 先拒绝：仅依赖 localStorage 状态、把空响应当无 session、跨 tab 复用 owner 数据、分页 cursor 重放旧 action。
- 成功/回归：首次打开/刷新/旧缓存/分页边界/离线/恢复/多 session；重新 hydrate 后 reducer 与 CLI status 一致。
- 完成产物：hydrate trace、分页 fixture、缓存淘汰和可见限制说明。

<a id="step-ui-18"></a>



#### UI-18 · Web SSE reconnect、Last-Event-ID 和 gap　⏳

- 依赖：UI-06/16/17。代码：SSE handler、EventSource wrapper、feed reducer。
- 步骤：发出 `id`/event/schema/epoch，读取 Last-Event-ID；心跳、退避、最大重连、terminal/gap 事件；连接 listener 在 resume 前安装。
- 先拒绝：重复/跳号静默接受、旧 epoch 继续渲染、事件流无限 buffer、SSE token 出现在历史/日志、断线自动重投副作用 command。
- 成功/回归：网络切断、代理重连、重复 Last-Event-ID、server restart、gap→snapshot、慢 tab、terminal retention。
- 完成产物：浏览器/HTTP fixture、reconnect state machine、可观测重连指标。

<a id="step-ui-19"></a>



#### UI-19 · Web session ownership 与多 tab 并发　⏳

- 依赖：UI-04/08/16–18。代码：session lease/tab identity、action coordinator。
- 步骤：每 tab 独立 draft/submission；服务端按 principal/session owner 授权；同一 action 的重复提交显示原 command 结果；跨 tab 通过 feed 观察而非共享可变状态。
- 先拒绝：tab A 操作 tab B 私有 session、stale card 通过、重复 click 产生两个 effect、关闭 tab 触发 cancel。
- 成功/回归：双 tab approve/cancel/edit、刷新、浏览器睡眠、竞态 CAS、token 轮换和 owner 退出。
- 完成产物：多 tab e2e、tab/session 关系图和冲突文案。

<a id="step-ui-20"></a>



#### UI-20 · Web 时间线组件迁移　⏳

- 依赖：UI-08/13/17/18。代码：从当前 inline HTML 渐进抽出 Thread/Turn/Item、status bar、feed indicator 组件；可采用 React/TS/Vite，但先保持静态构建可运行。
- 步骤：按 server item kind 渲染 delta/tool/approval/error/unknown；虚拟化和窗口化受有界 cursor 控制；错误、partial、replay、loading 可感知。
- 先拒绝：前端根据文本猜 item kind、innerHTML 直插不可信内容、虚拟化丢 pending/unknown、render key 使用数组 index。
- 成功/回归：旧页面功能 parity、长时间流、刷新重连、XSS payload、深链接和窄屏；组件只依赖 shared store/client。
- 完成产物：组件目录、迁移开关、浏览器截图/golden 和 bundle size 基线。

<a id="step-ui-21"></a>



#### UI-21 · Web Human Inbox 与审批动作卡　⏳

- 依赖：UI-02/04/05/15/20。代码：approval/inbox cards、form schema、action submitter。
- 步骤：表单字段、范围、理由、expiry、allowed decision 由 HumanActionCard 驱动；危险动作默认折叠并显示 effect scope；提交后显示 command status。
- 先拒绝：客户端添加隐藏字段扩大 scope、审批过期仍可 click、deny 需要重新执行 tool、Unknown 自动显示 retry。
- 成功/回归：approve/deny/edit/restore/continue、必填/非法值、过期/撤销/CAS conflict、重复 click 和响应丢失。
- 完成产物：inbox schema renderer、deny/happy fixtures、审计事件映射。

<a id="step-ui-22"></a>



#### UI-22 · Web artifact/diff/receipt detail　⏳

- 依赖：UI-05/15/20/21。代码：artifact API、diff viewer、receipt page。
- 步骤：按 ref 分页拉取文本/patch/metadata，校验 digest、MIME、revision、size；展示 server stats、file status、provenance、limitations 和 unknown。
- 先拒绝：任意 URL fetch、SVG/HTML/ANSI 注入、digest mismatch 继续预览、客户端 diff 代替授权载荷、私有 artifact 出现在列表。
- 成功/回归：大文件/二进制/截断/坏 digest/过期 ref、部分文件审批、receipt 从新进程重算。
- 完成产物：artifact viewer、content policy、diff/receipt golden 与大小上限。

<a id="step-ui-23"></a>



#### UI-23 · Web 可访问性、焦点和内容安全　⏳

- 依赖：UI-16–22。代码：CSS/DOM/ARIA/CSP、keyboard focus manager、sanitizer。
- 步骤：定义 landmark/live region、焦点回收、键盘顺序、缩放/对比度/减少动画；严格 CSP、无 eval/inline script（迁移期有例外要登记）、sanitize markdown/HTML。
- 先拒绝：仅用 aria 属性宣称达标、键盘无法处理 approval/cancel、焦点跳出 modal、用户/工具输出执行脚本、CSP 报错被吞掉。
- 成功/回归：键盘/读屏/200% zoom/窄屏、恶意 markdown/URL/OSC、CSP violation、prefers-reduced-motion。
- 完成产物：WCAG 2.2 检查表、axe/手工证据、CSP report 和无障碍回归截图。

### 27.4 Electron/Desktop 与 IDE

<a id="step-ui-24"></a>



#### UI-24 · Electron IPC sender、导航和新窗口 allowlist　⏳

- 依赖：UI-02/03/16/23。代码：`contrib/desktop/main.js`、`preload.js`、IPC handlers。
- 步骤：为每个 IPC 校验 `event.senderFrame`、origin、channel、workspace binding；`will-navigate`/`setWindowOpenHandler` 只允许受信 loopback URL 和显式外部浏览器打开。
- 先拒绝：任意 renderer 调 `workspace:*`、URL 伪造 workspace/token、未知 channel、导航到 file/javascript/data、不受控新窗口。
- 成功/回归：合法/伪造 sender、窗口重定向、open external、preload contextIsolation/nodeIntegration、dev/prod URL 差异。
- 完成产物：IPC allowlist、sender test、CSP/BrowserWindow 安全配置审计。

<a id="step-ui-25"></a>



#### UI-25 · Desktop readiness、attach 和 worker 生命周期　⏳

- 依赖：UI-03/24。代码：worker discovery/readiness、process group stop、single-instance lock。
- 步骤：ready record 使用结构化 stdout/sidecar，不从任意 stderr URL 猜地址；Electron attach 到正确 instance/workspace；启动失败、崩溃、升级和 stop 有状态。
- 先拒绝：PID 重用、旧 ready URL、错误 workspace、kill 单进程留下子进程、worker 未 ready 就发 action、关闭自动 resume。
- 成功/回归：cold start/attach/restart/crash/SIGTERM/子进程、端口占用、重复窗口、升级兼容和 process-group cleanup。
- 完成产物：桌面生命周期图、readiness fixture、物理进程 e2e 与诊断日志。

<a id="step-ui-26"></a>



#### UI-26 · Desktop workspace、托盘、通知与关闭策略　⏳

- 依赖：UI-08/19/24/25。代码：workspace switcher、tray/menu、notification bridge、close confirmation。
- 步骤：workspace open/new/continue 只发显式 command；通知仅由服务端终态/待办事实触发；关闭提示 pending/unknown，不把窗口状态等同 run 状态。
- 先拒绝：旧 workspace 数据串入新窗口、通知泄漏私有内容、close 隐式 cancel/resume/trust、托盘调用宽权限 API。
- 成功/回归：多窗口、托盘隐藏/恢复、OS notification permission、未保存草稿、pending approval、unknown invocation 和 reopen。
- 完成产物：desktop state machine、通知脱敏策略、关闭/托盘 e2e。

<a id="step-ui-27"></a>



#### UI-27 · Desktop 安全持久化和 detach　⏳

- 依赖：UI-04/25/26。代码：layout/session reference store、safe detach/reattach。
- 步骤：只持久化布局、草稿策略、instance/session 引用和最后已知 cursor；敏感 token 使用受控存储；detach 后重新 handshake，不自动执行。
- 先拒绝：把 access token 写明文配置、从 layout 恢复即 launch/resume/approve、旧 epoch 的 cursor 直接提交、私有 artifact 缓存无 TTL。
- 成功/回归：重启/升级/损坏 store/权限不足/多用户、旧实例、token rotation、detach/reattach。
- 完成产物：持久化 schema、迁移/清理策略、恢复证据和限制清单。

<a id="step-ui-28"></a>



#### UI-28 · 共享静态资产、版本和生产打包　⏳

- 依赖：UI-20/23/24–27。代码：Web build、asset manifest、Electron packaging、CSP hash。
- 步骤：固定 schema/client/ui 版本和 asset hash；开发/生产 URL、base path、cache headers、source map 和 license；桌面包内不带 secret。
- 先拒绝：旧 bundle 连接新 protocol 无能力协商、缓存旧 JS 绕过 CSP、构建把 `.env`/token 打入产物、未签名/未校验资源运行。
- 成功/回归：clean build、离线 bundle、hash mismatch、upgrade/rollback、Electron package 和 loopback deployment smoke。
- 完成产物：可复现 build manifest、打包检查、CSP hash/asset integrity 证据。

<a id="step-ui-29"></a>



#### UI-29 · ACP/IDE session adapter　⏳

- 依赖：UI-01/02/07/18/21/25。代码：`kiana-entrypoints` 或独立 adapter 的 initialize/session/new/resume/prompt/update/permission/cancel。
- 步骤：将 ACP turn/update 映射到同一 UiSnapshot/UiFeed/UiAction；连接 handler 在 resume/replay 前安装；明确 v1/v2 capability 和 session ownership。
- 先拒绝：未知协议版本、外部 session 越权、permission 过期、cancel 抢先显示为最终成功、host tool 直接执行、update 乱序。
- 成功/回归：fake ACP peer 的 initialize/new/resume/prompt/permission/cancel、disconnect/replay/gap、旧版本协商和 late update。
- 完成产物：ACP mapping table、fixture peer、版本/错误转换表；live IDE 只在 opt-in 卡验证。

<a id="step-ui-30"></a>



#### UI-30 · IDE editor/terminal capability boundary　⏳

- 依赖：UI-04/07/29、Capability 专项。代码：editor/terminal adapter 的 capability request/permit bridge。
- 步骤：将打开文件、读取、patch preview、apply、终端输出映射为受控 Artifact/UiAction；editor host 只提供显示/输入，真正 effect 回 ControlPlane。
- 先拒绝：任意路径、隐式保存、编辑器扩展绕过 permit、终端 spawn、旧 revision apply、remote workspace 混入本地 scope。
- 成功/回归：只读/写入/拒绝/过期 permit、外部修改、diff digest、terminal cancel/unknown、host 重启。
- 完成产物：IDE capability matrix、fake editor fixture、越权/TOCTOU 证据。

### 27.5 跨入口一致性、恢复和质量门

<a id="step-ui-31"></a>



#### UI-31 · CLI/Workbench/Web/Desktop 行为 parity　⏳

- 依赖：UI-10–30 中各入口可运行。代码：共享 operation matrix、cross-surface harness。
- 步骤：用同一 fixture 依次执行 open/status/run/approval/cancel/resume/receipt/export；比较 protocol action、最终 cursor/revision、error code 和 Receipt，而不是比较文案。
- 先拒绝：某入口绕过 capability、用不同 retry policy、把断线/关闭改写成业务状态、结果字段缺失或敏感字段增多。
- 成功/回归：同一 command ID 在各 surface 查询结果一致；surface-specific presenter 只改变显示和输入方式。
- 完成产物：parity matrix、跨入口 trace diff、差异解释和未支持能力清单。

<a id="step-ui-32"></a>



#### UI-32 · deny-first 安全路径集成测试　⏳

- 依赖：UI-04/16/19/21/23/24/30/31。代码：entrypoint/daemon/core integration tests。
- 步骤：覆盖未授权、越权 session、错误 token/origin/sender、过期 approval、stale cursor/revision、digest mismatch、scope widening、cancel race、unknown effect、注入。
- 先拒绝：每个拒绝必须无 effect、无错误副作用，或明确记录 Unknown/incident；测试检查 effect count、journal 和 audit event。
- 成功/回归：拒绝原因稳定、UI 可恢复、CLI exit code 正确、Web/desktop 不泄漏；再执行对应 happy path。
- 完成产物：deny matrix、effect-count fixtures、审计/receipt 断言。

<a id="step-ui-33"></a>



#### UI-33 · reconnect/replay/gap/crash recovery e2e　⏳

- 依赖：UI-05–08、UI-18、UI-25、Event/Receipt §23。代码：daemon restart + browser/PTY/Desktop/ACP harness。
- 步骤：在 snapshot、feed、action accepted、artifact fetch、cancel 和 terminal 各阶段注入断网/kill/延迟/重复；重启后查询原 command、重建 projection、fence 旧 epoch。
- 先拒绝：丢响应产生第二 effect、gap 静默继续、terminal 重复、旧 worker 仍写入、恢复自动 approve/resume/retry。
- 成功/回归：状态最终收敛到 Applied/Rejected/Cancelled/Failed/Unknown；Unknown 有显式下一步和 limitation。
- 完成产物：故障时间线、重启 fixture、recovery Receipt 和未证明的跨进程边界。

<a id="step-ui-34"></a>



#### UI-34 · 性能、资源上限和可访问性验收　⏳

- 依赖：UI-13/20/23/28/33。代码：metrics、bounded queue/cache/render budget、accessibility runner。
- 步骤：为 feed、snapshot、artifact、DOM、TUI buffer、IPC payload 定义上限和退化行为；测首屏、长 stream、100+ session、超长 diff、慢磁盘、低带宽。
- 先拒绝：无界内存、慢消费者拖住 EventStore、丢弃 pending/unknown、超限静默截断、焦点/读屏回归。
- 成功/回归：压力下仍有可见 Degraded/limited 状态和可恢复 action；记录 p50/p95、RSS、queue depth、bundle size。
- 完成产物：性能基线、资源预算、无障碍人工/工具报告。

<a id="step-ui-35"></a>



#### UI-35 · 旧 Web/CLI 迁移与兼容收口　⏳

- 依赖：UI-10–23、UI-31。代码：旧 inline Web、旧 endpoint、旧 CLI 参数的 adapter/feature flag。
- 步骤：逐路由/命令迁移到 typed client；保留必要兼容窗口和明确 deprecation；双读比较 projection，不双写事实；删除旧路径前记录使用/测试证据。
- 先拒绝：兼容层重新执行副作用、旧参数扩大 scope、双写造成两个 command、旧缓存污染新 epoch/schema。
- 成功/回归：新旧入口同一 fixture parity；禁用 flag 后旧请求返回可操作迁移错误；不存在第二执行循环。
- 完成产物：迁移表、flag 生命周期、兼容窗口和删除前 checklist。

<a id="step-ui-36"></a>



#### UI-36 · 生产构建、安装和发布前 smoke　⏳

- 依赖：UI-28/34/35。代码：Web static build、Electron artifact、CLI distribution scripts。
- 步骤：执行 clean/offline 构建、版本/asset/schema 检查、loopback launch、桌面 attach、CLI pipe、Workbench smoke；确保日志和包不含 secret。
- 先拒绝：构建引用工作树绝对路径、未锁依赖、旧 protocol、Electron 远程导航、安装后默认 trust/resume。
- 成功/回归：空目录安装、升级/回滚、无网络、权限不足、端口占用、daemon crash；每个失败有诊断和退出码。
- 完成产物：构建 manifest、安装 smoke 回执、artifact hash 和已知限制。

<a id="step-ui-37"></a>



#### UI-37 · 用户文档、模块图和操作 runbook　⏳

- 依赖：UI-31–36。代码/文档：`module-map.md`、`USER.md`、入口帮助、`docs/company-os-ui-ux.md`。
- 步骤：更新真实调用关系、命令/错误/状态/恢复流程、token/来源边界、CLI/Workbench/Web/Desktop 差异；将目标与已实现明确分开。
- 先拒绝：把类型写成 implemented、把一次本地通过写成 durable/live、把 reference 行为写成 Kiana 事实、遗漏限制。
- 成功/回归：从干净 checkout 按文档启动并完成最小 deny/happy/reconnect；链接、编号和命令检查通过。
- 完成产物：用户 runbook、module-map 入口图、迁移 FAQ 和限制/证据索引。

<a id="step-ui-38"></a>



#### UI-38 · 协议/入口 conformance 集成门　⏳

- 依赖：UI-01–37。代码：统一 conformance runner，覆盖 Rust client、CLI、Web、Desktop、ACP fake peer。
- 步骤：对每个 surface 执行 schema decode、capability、snapshot/feed cursor、action disposition、error/retry、artifact digest、receipt reference 检查；生成可比较 JSON trace。
- 先拒绝：未声明 schema/epoch、重复/跳号、错误 retry、unknown 被吞、敏感字段泄漏、surface 特例绕过协议。
- 成功/回归：同一 fixture 的事实 trace 一致，差异仅在 presenter；失败输出最小复现 cassette。
- 完成产物：conformance suite、fixture registry、CI gate 和差异报告格式。

<a id="step-ui-39"></a>



#### UI-39 · live ACP/IDE opt-in 验证　⏳

- 依赖：UI-29/30/33/38。代码/fixture：显式 opt-in 的本地 ACP/IDE 连接脚本。
- 步骤：固定协议版本、workspace、能力和脱敏数据，验证 initialize/session/prompt/update/permission/cancel/reconnect；记录外部 host 版本。
- 先拒绝：未 opt-in 的网络/远端 provider、host 直接写文件/执行终端、live 结果替代 deny fixture、不可重现环境被写成 durable。
- 成功/回归：live 只提升到声明的 `live` 上限；断开、旧版本、permission timeout 和 host 重启仍回到本地 recovery 语义。
- 完成产物：live cassette、host/version/environment、上限与限制；无 live 环境时保持 ⏳。

<a id="step-ui-40"></a>



#### UI-40 · 发布门与证据收口　⏳

- 依赖：UI-32–39、Event/Receipt §23。代码/脚本：聚焦 cargo tests、entrypoint tests、desktop tests、smoke 和文档校验。
- 步骤：先运行 deny/recovery，再 happy/parity/performance；按改动 crate 串行 daemon/control-plane 测试；保存命令 argv、匹配测试数、环境、fixture digest 和 exit code。
- 先拒绝：基线失败混入本次结果、`0 tests`、忽略/放宽断言、网络偶然可用、一次人工观察冒充 durable/physical。
- 成功/回归：必要 checks、`git diff --check`、schema/link/id 唯一性、release smoke 全部有回执；更新状态仅覆盖真实行为。
- 完成产物：UI evidence bundle、failure classification、CURRENT_STATUS 候选证据块（待 reviewer 采纳）。

<a id="step-ui-41"></a>



#### UI-41 · 交接、审查和后续缺口　⏳

- 依赖：UI-40。代码/文档：roadmap、module-map、CURRENT_STATUS、实现 PR/工作树说明。
- 步骤：列出完成/部分/延期/不支持的 UI 卡，绑定 P/P2/P4、Event/Receipt、Capability、Harness 单元；指出跨进程、物理 PTY、live provider、规模和浏览器兼容上限。
- 先拒绝：没有 reviewer、没有 source snapshot、把 WIP agent 修改视为验收、发现规范与源码冲突却改规范消除冲突。
- 成功/回归：独立 reviewer 能从 evidence bundle 重跑关键拒绝与恢复路径；未完成项有下一步和阻塞事实，不用“整体完成”覆盖细节。
- 完成产物：交接清单、审查意见、最终证据索引和下一批明确任务。

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
