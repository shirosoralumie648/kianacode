# 一次任务如何运行

本文沿着当前源码中的 `kiana run -- "..."` 说明普通任务的调用顺序。它聚焦一条容易核对的入口链，不代表所有 CLI 子命令都经过同一条函数路径。源码名和类型名可直接在链接文件中搜索；文件较大时先用下面列出的函数名定位，不需要通读整份文件。

## 一条主路径

```text
终端输入
  → entrypoints::cli::run_main
  → harness_run::run_envelope
  → stream_render::run_envelope（CLI 流式展示时；内部仍发起同一 Run）
     或直接 run_envelope_on_host（非流式展示时）
  → KianaClient::run（普通 CLI 的空历史请求；其他入口也可使用 run_with_history）
  → LocalDaemonTransport::send
  → DaemonHost::handle
  → ControlPlane::start_run_with_history
  → RunnerPort / KianaHarness
      ↔ ModelClient
      → CapabilityRequested
  → ControlPlane 检查并派发
  → CapabilityBrokerPort / handler
  → CapabilityResult 回 Runner
  → EventStore 事件 + Receipt 查询
```

同一进程中，`LocalDaemonTransport` 直接调用 `DaemonHost::handle`，没有 TCP/HTTP 请求边界。Web 和工作台的界面不同，但仍复用 `DaemonHost`；不能因为入口不同，就推断它们另有权限判断或模型循环。

## 阶段走读

### 1. 命令行参数变成一次运行请求

源码顺序：`kiana-entrypoints/src/bin/kiana.rs::main` → `kiana-entrypoints/src/cli.rs::main` → `run_main`。

`main` 先运行进程初始化，再等 `cli::main` 返回，最后运行清理 handler。`cli::main` 按子命令把 `run` 分发给 `run_main`。`run_main` 解析 `--sandbox`、`--role`、`--symposium`、`--packet`、`--continue` 等选项，检查选项是否互斥，并校验 prompt 是否为空。普通新任务分配新的 session UUID，然后进入 `harness_run::run_envelope`。

输入是 argv 字符串；输出是结构化的选项、prompt 和 session id，之后包装成协议请求。比如：

```text
kiana run --sandbox workspace-write -- "新建 NOTES.txt，内容为 hello"
```

这一步只解析用户意图，不会直接启动 shell，也不代表写入权限已经通过。

### 2. 入口组装身份元数据并发送协议 envelope

源码顺序：CLI 普通新任务进入 `harness_run::run_envelope`；该函数按 `stream` 选项进入 `stream_render::run_envelope`，或直接装配 Host 并调用 `run_envelope_on_host`。流式分支先订阅同一 Host 的运行流，再启动 `run_envelope_on_host`；后者调用 `client_on_host`，最终由 `KianaClient::run`（位于 `kiana-client/src/lib.rs`）发送普通空历史 Run。

非流式路径由 `run_envelope` 创建本地 `DaemonHost`；流式路径由 renderer 创建 Host，并把同一个 Host 交给 `run_envelope_on_host`。`run_envelope_on_host` 解析沙箱选项、通过 `client_on_host` 生成 `RequestMetadata` 和 client，再调用 `KianaClient::run`。普通 CLI 初次运行因此不会携带聊天历史。显式的 `run_with_history` 属于另一条入口；`harness_run::run_owned_harness_with_history` 和 `harness_run::run_owned_harness_with_history_and_permission_handler` 是 SDK 等调用方使用的带历史包装函数，不能把它们误认为普通 `kiana run` 的中间步骤。transport 持有 `Arc<DaemonHost>`，其 `send` 直接调用 `host.handle(request)`。

这里的 `Arc<T>` 表示多个对象共享同一个宿主实例的所有权；它不是“线程锁”。请求元数据是服务端入口构造的身份上下文，不等于授权决定。

普通 CLI 在未使用互斥子命令时默认设置 `stream=true`；`--json` 和 `--no-stream` 会关闭这条显示分支，显式 `--stream` 会打开。renderer 先订阅 Host 的运行流以免漏掉第一条增量事件，然后仍通过 `run_envelope_on_host` 和 `KianaClient::run` 发起唯一的 Run；它只读事件并显示输出，不是第二个 Agent loop。CLI 的流式呈现和模型 adapter 是否启用原生 token streaming 是两层设置；后者还受 `KIANA_STREAMING` 控制，证明边界以 [CURRENT_STATUS.md](../../CURRENT_STATUS.md) 为准。

### 3. DaemonHost 校验请求并交给 ControlPlane

源码位置：`kiana-daemon/src/lib.rs::DaemonHost::handle`。

`handle` 校验协议 schema、受保护入口字段和本地主体信息；接着根据项目根目录构造服务端 `RequestContext`，为会改变执行状态的请求同步授权快照并绑定 session assignment。`RequestBody::Run` 分支调用 `ControlPlane::start_run_with_history`。这层负责把本地环境接入控制面，不应被当成策略实现本身。

拒绝发生在模型执行之前时，请求以结构化响应返回。项目未信任、协议版本不兼容、角色或主体校验失败都不会进入成功的工具执行路径。

### 4. ControlPlane 建立 Run、Turn 并启动 Runner

源码位置：`kiana-core/src/lifecycle.rs::ControlPlane::start_run`、`start_run_with_history`、`start_run_with_id`，以及实现 `RunnerPort` 的 `kiana-runner/src/harness.rs::KianaHarness`。

`start_run_with_history` 将入口传来的历史和新 prompt 交给 `start_run_with_id`。后者确定 `RunId`、`TurnId` 和 turn 前驱关系，记录请求已接受的事件，再要求 Runner 执行。它返回控制面格式的结果；Runner 只能以 Runner command/event 协议报告进度，不能越过 core 执行能力。

核心对象通过 `Arc<dyn Trait>` 等 trait object 接收实现：`dyn Trait` 让运行时可注入任一实现该接口的 adapter，`Arc` 让多个异步组件共享它。`async fn` 表示调用可能等待模型、存储或工具 I/O；`Result<T, E>` 把成功值和可处理的失败显式传回调用者。

### 5. Daemon 为 Harness 选择模型客户端

源码位置：`kiana-daemon/src/model_client.rs::from_config`；接口和本地实现位于 `kiana-runner/src/model.rs`，请求构造及调用位于 `kiana-runner/src/harness.rs` 的 step/model invocation 逻辑。

daemon 把某个 `ModelClient` 注入 `KianaHarness`。源码可选择 `KIANA_HARNESS_SCRIPT` cassette、`fake` provider，或 `kiana_provider::ProviderGateway`；配置错误和没有可用模型时使用 `UnavailableModel`，明确返回错误。provider gateway 的源码存在，不等于 live 服务路径已经验证。

每个 Runner step 会将当前消息、固定的 `tool_schemas()` 与 sandbox 组装为 `ModelRequest`。`invoke_model` 还处理模型调用身份、预算预留、deadline、取消检查和重试判定，再调用注入的 `ModelClient`。Provider adapter（如果选中）把统一请求转成服务商请求，再把服务商响应译回内部 `ModelOutput`。不同 provider 的具体 HTTP 编码应继续读 `kiana-provider`；不要把协议转换和授权流程混为一谈。

### 6. 模型输出解析成文本或能力申请

模型调用返回 `Result<ModelOutput, String>`。Harness 检查 stop reason、整理安全的 assistant 文本，并把文本和工具调用放回当前对话消息序列。没有工具调用且没有待处理输入时，Runner 产生 Completed；截断、拒绝、传输未完成、未知 stop reason 和模型错误会进入失败事件或相应终态。

若响应包含工具调用，Harness 先用 domain 的 `validate_model_calls` 和 `plan_tool_batch` 检查结构，再对每个 `ModelToolCall` 调用 `capability_for_tool_with_request_id`。该转换只接受固定工具目录中的 `shell`、`apply_patch`、`mcp`、`memory.search`、`memory.write`，并校验参数 schema。它产生带稳定 request id 的 `CapabilityRequest`，不运行命令或写文件。

同一时刻 Runner 可能只发出队首待处理工具；core 回传结果后，Runner 再处理下一个。不能根据模型一次返回多个 tool call 就推断它们会并行执行。

### 7. ControlPlane 检查权限、审批和执行许可

源码主要位置：`kiana-core/src/dispatch.rs::ControlPlane::dispatch_authorized` 与 `deliver_capability_result`；策略和 Gate 实现在 `kiana-policy`、`kiana-gates`，审批读 `kiana-core/src/approvals.rs`。

`CapabilityRequested` 是 Runner 发出的“申请执行”事件。core 根据服务端 `RequestContext`、角色、项目、风险、sandbox、工单/路径等约束计算策略结果；Gate 只能保留或收紧该结果。需要人工批准的请求停留在 `AwaitingApproval`，入口之后以新的审批请求回到 core，校验 challenge/hash/nonce 等绑定后才可能继续。策略、审批、authority version 或调用身份检查失败都必须拒绝或停在待处理状态。

通过检查后，core 把请求包装为 `AuthorizedCapabilityRequest`，再调用 `CapabilityBrokerPort`。`Authorized` 类型说明请求已经越过上游控制面边界；它本身不是密码学证明，也不能替代 Broker 执行前的许可校验。

### 8. Broker 找到 handler 并执行能力

源码位置：`kiana-capability-broker/src/lib.rs::CapabilityBroker::execute` / `execute_cancellable`，以及 daemon 注册的具体 handler。

Broker 用 `(CapabilityKind, operation)` 精确查找已注册 handler；未知键返回 unavailable，不会调用一个更宽泛的 handler 兜底。执行前还会校验请求动作、extension admission（若适用）、连接器边界与用量/执行许可；然后调用 handler 的异步方法，并检查返回的 effect usage。真正的 shell、补丁、MCP 或 memory I/O 由被注册的具体 handler 完成。

Broker 的 `RwLock<HashMap<...>>` 保护并发注册表读取；查到 handler 后克隆 `Arc` 并释放锁，再 `await` 外部执行。这样一个慢工具不会在整个等待期间占住注册表读锁。

### 9. 工具结果回到模型，循环继续或结束

Broker 将 `CapabilityResult` 返回给 core。`deliver_capability_result` 根据运行和调用事件完成结果交付，并生成 Runner command；Harness 收到 `RunnerCommand::CapabilityResult` 后更新 pending tool 与对话上下文，然后开始下一 step。模型可能继续提出工具调用，也可能返回最终文本；预算上限、取消信号、模型拒绝、adapter 错误或状态冲突都可能结束/阻断本轮。

取消命令经同一 client、daemon 和 core 路由到 Runner 的取消状态。若系统无法证明副作用是否已经发生，不能把它伪装成普通失败后自动重试；结果未知的边界要继续看 core dispatch 的提交与恢复代码。

### 10. 运行事件、内存历史和 Receipt 是不同东西

Harness 在 `KianaHarness` 内用进程内 map 持有活跃 run、运行上下文和模型消息；这是供本次模型循环使用的历史，不是重启后可恢复聊天数据库。普通初始 `kiana run` 不携带先前历史；core 的显式 continue/new-turn 路径才会按各自实现恢复内存状态或从事件折叠模型历史。`--continue` 的限制和恢复能力以 [CURRENT_STATUS.md](../../CURRENT_STATUS.md) 为准。

控制面通过 `record_event` / `append_event` 把请求、运行、模型尝试、能力申请、执行结果和终态等记录写入注入的 `EventStorePort`。当前本地 `DaemonHost::local` 装配 `JsonlEventLog::open_default`；测试或显式注入构造可使用 `MemoryEventLog`。JSONL 实现包含 flush/sync 路径，但每项恢复与 durability 声明仍须按 EventLog 测试回执分别判断。UI transcript/run stream 是展示投影，不能替代 EventLog。

`--receipt <id>` 由 `harness_run::receipt_envelope` 重新发一个 Receipt 请求，daemon 转给 `ControlPlane::read_receipt`，后者读取已保存事件并构造收据视图。Receipt 说明系统记录了什么，不能单独证明现实世界结果正确。没有足够事件、所有者不匹配或结果未知时，core 会返回相应的阻断/未知状态，而不是造一个成功收据。

## 规划、多角色与 Workflow

`--symposium`、`--packet`、`--review`、`--close` 在 `run_main` 中是互斥分支，分别进入 `harness_run::symposium_envelope`、`spawn_envelope`、`review_envelope`、`close_envelope`。这些命令仍通过 client、protocol 和 DaemonHost 请求；Core 的 collaboration/spawn 实现会为子运行建立各自 Run/role，再复用 `start_run_with_id` 与同一 Runner/授权派发路径。现有产品约束明确 Builder 不列席规划或监控会；不要将它们解释为可以自由互发消息的群聊。

一次 `--symposium` 请求如何派生各角色 turn、何时写 packet/decision、Builder packet 是否为新 session，应按 `RequestBody::Symposium/Spawn` 在 `kiana-daemon::handle` 与 `kiana-core` 相应 symposium 模块继续追。若只检查入口分支，不能证明每个角色的完整执行和产物都成功。

`kiana-workflow` 的 `WorkflowState::transition` 是纯状态转换；core 的 automation 代码实际调用该 crate 中的规划函数。workflow 描述下一步状态/命令，副作用、授权、事件记录仍由 ControlPlane 完成。它不再开一套模型循环。

## 本轮只读验证中遇到的异常

当前共享工作树已有大量与本任务无关的未提交修改。只读研究中执行的 daemon 集成测试显示：`run_brokers_kiana_harness_tools` 能走到 shell handler，但最终得到 `ResultUnknown` / `shell_exec_failed: Invalid argument (os error 22)`；`model_written_memory_without_evidence_is_rejected_and_stays_unsearchable` 与 `wire_approval_proof_retry_resumes_original_run` 的预期也未满足，返回 `result_unknown:port_failed:result_unknown:result_delivery_stop_mismatch`。这是当前 checkout 的测试回执，不是干净基线结论；本任务只记录，不调整业务行为。

其他 stream/EventLog 测试有断言与当前事件流/JournalFrame 表示不一致的失败；相关数据是否实际生成仍需用 EventStore API 或正确解析 frame 的测试核对，不能把测试断言失败直接写成“事件未记录”。完整命令及本轮后续范围见 [COMMENT_PROGRESS.md](COMMENT_PROGRESS.md)。

## 当前需要保持的证据边界

- 源码可定位调用边，不自动证明所有分支在生产配置下成功。
- Scripted/fake provider 通过本地回执，不等于 live provider 成功。
- 单测中 handler 调用成功，不自动证明完整 `kiana run` 主路径或 restart recovery。
- EventLog 有 append/read API，不单独证明全部事件的 crash durability。
- 入口能返回 Completed 或 Receipt，不等于外部世界的结果已经被独立验证。

这些边界和最近实测命令请查看 [CURRENT_STATUS.md](../../CURRENT_STATUS.md) 及 [USER.md](../../USER.md)。
