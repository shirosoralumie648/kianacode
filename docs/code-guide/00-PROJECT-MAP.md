# KianaCode 项目地图

本文是源码导读，不是能力状态账本。它按当前 workspace、crate 依赖和入口调用整理代码位置；功能的证明等级、当前限制和近期验证结果以 [CURRENT_STATUS.md](../../CURRENT_STATUS.md) 为准。`docs/` 中的规范不能替代源码和测试回执。

## KianaCode 是什么

KianaCode 是一个本地优先的 Rust coding-agent 运行时。用户可以从命令行、文件夹工作台或本机 Web 界面提交任务。运行时把请求交给统一的 `DaemonHost` 和 `ControlPlane`；模型只提出工具调用，控制面审核后才把能力交给 Broker 执行。运行事件可以写入 EventStore，再投影成收据。

从用户入口看，当前手册列出的实际操作包括：信任项目后以 `read-only` 或 `workspace-write` 运行任务；使用 cassette/fake-script 作本地模型输入；发起规划会并生成 packet；由新 Builder session 消费 packet；查询 Receipt；运行 loopback Web 工作台；配置 stdio MCP。写入仍受项目 trust、sandbox、角色、路径与控制面检查约束。这里描述的是已有用户工作流，不表示每一种 provider 或每个 edge case 都完成了同一级别的验证。

源码中存在 provider gateway、脚本模型和多种入口，但“存在实现”不等于“已证明可用于生产”。[USER.md](../../USER.md) 当前将整体验证上限写为 `local_behavior`：受信本地仓库与 cassette/fake-script 可复现；live provider、生产安装和签名包不在已完成证明之列。daemon 源码能选择 provider gateway，因此可以说“有该接线路径”，不能据此说 live 服务已经验证。细节以当前状态账本为准。

## 功能分层

```text
入口（CLI / Workbench / Web）
  → 协议请求与本地传输
  → DaemonHost 装配的 ControlPlane
  → 身份、信任、策略、Gate、审批与生命周期
  → Runner 产生模型输出和能力请求
  → Broker 按已授权能力选择 adapter 执行
  → EventStore 记录事件，ControlPlane 生成查询视图和 Receipt
```

这些 crate 不能简单合成一个“Agent 模块”：domain/protocol 定义各层交换的数据，ports 定义可替换实现的边界，daemon 负责把实现接起来，core 持有授权和生命周期规则，runner 只负责模型循环，broker 才调用工具 adapter。这样的分工让 Web、CLI 和工作台共享同一份控制面状态，也让运行时测试可以替换模型、存储和能力实现。

## Workspace crate 地图

以下分类以当前源码依赖及已核对的入口为依据。标记为“主路径实调用”的 crate 有清楚的生产调用边；“辅助实调用”表示主路径装配或入口功能确实使用它，但不代表其中所有模块都已接入；“入口兼容面”表示仍被入口依赖或路由，但不属于规范 Harness 工具执行链；“待确认”表示 workspace 成员本身不足以证明生产调用。

| Crate | 源码位置 | 代码在系统中的位置与当前分类 |
|---|---|---|
| `kiana-entrypoints` | `kiana-entrypoints/src/bin/kiana.rs`、`kiana-entrypoints/src/cli.rs`、`workbench.rs`、`web.rs` | **主路径实调用，但入口内仍有兼容分支。** `kiana run` 由 `run_main` 转到 `harness_run`；工作台/Web 走 Host。`runner.rs::run_assistant_turn*` 仅测试编译；同文件还有 CLI 可达的 resident teammate/poll loop，需单独追它的职责，不能与普通 `kiana run` 混为一谈。CLI 的旧命令和 MCP/remote 辅助路由也仍在源码中。 |
| `kiana-client` | `kiana-client/src/lib.rs` | **主路径实调用。** 入口使用 `KianaClient` 把请求封装为协议 envelope，再通过 `LocalDaemonTransport` 送到 daemon；它负责传输适配，不决定是否授权。 |
| `kiana-protocol` | `kiana-protocol/src/lib.rs` | **主路径实调用。** 定义版本化请求/响应 envelope、请求体、运行 ID 和命令载荷，连接 client 与 daemon。 |
| `kiana-daemon` | `kiana-daemon/src/lib.rs`、`model_client.rs`、`storage.rs` 等 | **主路径实调用。** `DaemonHost` 是本地组合根，构造 core、runner、broker、存储与 adapter，并接收 client 请求。它装配依赖，不应被理解为授权规则本身。 |
| `kiana-core` | `kiana-core/src/lib.rs`、`lifecycle.rs`、`dispatch.rs`、`approvals.rs`、`events.rs`、`receipts.rs` | **主路径实调用。** `ControlPlane` 持有请求生命周期、授权判断、审批、派发、事件和收据查询等控制逻辑；具体实现分散在私有模块中。 |
| `kiana-domain` | `kiana-domain/src/lib.rs` 及其模块 | **主路径实调用。** 定义稳定 ID、角色、请求上下文、能力请求/结果、运行状态和事件等跨 crate 值对象及校验规则。底层类型被 core、protocol、ports 等共享。 |
| `kiana-ports` | `kiana-ports/src/lib.rs` | **主路径实调用。** 定义 `RunnerPort`、`CapabilityBrokerPort`、`EventStorePort` 等 trait。core 依赖这些接口，让 daemon 注入真实或测试实现。 |
| `kiana-runner-protocol` | `kiana-runner-protocol/src/lib.rs` | **主路径实调用。** 定义 runner 命令与事件协议，连接 core 生命周期调度和 Harness，不负责批准或执行工具。 |
| `kiana-runner` | `kiana-runner/src/lib.rs`、`harness.rs`、`model.rs`、`tools.rs` | **主路径实调用。** `KianaHarness` 运行模型循环、维护当前进程内的会话状态，并将模型工具调用转换为 `CapabilityRequest`。它不直接执行工具。 |
| `kiana-policy` | `kiana-policy/src/lib.rs` | **主路径实调用。** 按服务端上下文和能力申请给出 Allow/Ask/Deny 类策略结论；它不是 handler，也不执行副作用。 |
| `kiana-gates` | `kiana-gates/src/lib.rs` | **主路径实调用。** 把策略结论收敛为控制面后续可采取的 Gate 状态；职责是映射和收紧，不调用工具。 |
| `kiana-capability-broker` | `kiana-capability-broker/src/lib.rs` | **主路径实调用。** 接收 core 包装好的已授权请求，按能力类型和精确 operation 查找 handler，再调用 adapter。未注册操作不会模糊回退。 |
| `kiana-eventlog` | `kiana-eventlog/src/lib.rs`、`jsonl.rs` 等 | **主路径实调用。** 提供内存和 JSONL 等 EventStore 实现；daemon 负责选择与装配，core 通过 `EventStorePort` 读写运行事实。具体 durability 仍按测试回执判断。 |
| `kiana-provider` | `kiana-provider/src/lib.rs` 及 provider adapter 模块 | **装配路径已存在，live 证明待查账本。** daemon 的 `model_client` 在未选 cassette/fake 时可构造 `ProviderGateway`。配置与 HTTP 客户端代码存在，不代表对外模型服务已获 live 验证。 |
| `kiana-workflow` | `kiana-workflow/src/lib.rs` | **core 辅助实调用。** core 的 automation 路径调用其 workflow 规划函数；crate 本身不运行模型、不审批、不写事件。daemon 中另有 `WorkflowQueueService`，源码存在显式 start API，但默认 Host 的生产构造路径未发现调用它的证据，不能据此说队列服务已自动启动。 |
| `kiana-query` | `kiana-query/src/lib.rs` | **辅助实调用。** daemon 的 context command handler 经 Core/Broker 使用它；entrypoints 还有 DirectConnect 的只读 context 查询路由。它不是 `kiana-runner/src/tools.rs` 中五个模型工具之一，不能说模型能直接调用所有 query API。 |
| `kiana-skills` | `kiana-skills/src/lib.rs` | **辅助实调用。** daemon 和入口使用其技能发现/加载相关能力；项目级资源是否能加载还受项目路径和信任检查约束。不要只凭 crate 存在断言每种 Skill 来源都启用。 |
| `kiana-services` | `kiana-services/src/lib.rs` | **辅助/兼容调用混合。** 被 daemon、entrypoints、skills 和旧 commands 引用。它含有服务实现，但每个 service 是否走当前 Broker 路径要从其 handler 注册和调用点确认。 |
| `kiana-types` | `kiana-types/src/lib.rs` | **辅助实调用与兼容类型并存。** daemon/query/skills 等仍引用其中的类型或本地配置 helper；它不是新能力的默认归属层。查看具体类型消费者后再判断是否属于当前路径。 |
| `kiana-bootstrap` | `kiana-bootstrap/src/lib.rs` | **入口依赖存在，普通 Harness 调用未证实。** manifest 显示 entrypoints 和旧 commands 依赖它；在本次确认的普通 `kiana run` 请求链中没有它的直接调用。要理解进程启动时再从 `kiana-entrypoints/src/init.rs` 搜实际消费者。 |
| `kiana-commands` | `kiana-commands/src/lib.rs` | **入口兼容面。** 被 entrypoints 使用，内部还依赖旧 tools/services/tasks 等。它可以服务于兼容命令，但 `kiana run` 的模型工具申请走 Harness → ControlPlane → Broker。新 CompanyOS 能力不能因为这个 crate 被引用就注册到旧命令总线。 |
| `kiana-tools` | `kiana-tools/src/lib.rs` | **入口兼容面。** entrypoints/commands 仍有依赖关系；其旧工具注册表不等于 Harness 的模型可见工具面。当前 Harness 工具定义看 `kiana-runner/src/tools.rs`。 |
| `kiana-tasks` | `kiana-tasks/src/lib.rs` | **旧命令依赖。** 被 `kiana-commands` 引用；没有证据表明它是当前 Harness run 的任务生命周期实现。 |
| `kiana-modifiers` | `kiana-modifiers/src/lib.rs` | **旧命令依赖。** 被 `kiana-commands` 引用，当前主执行脊柱没有依赖它的直接证据。 |
| `kiana-constants` | `kiana-constants/src/lib.rs` | **间接辅助依赖。** 被 `kiana-services` 引用；应根据具体常量消费者阅读，不能仅凭 crate 名归为运行时权威。 |
| `kiana-bridge` | `kiana-bridge/src/lib.rs` | **兼容/集成面。** entrypoints 依赖它，桥接层还依赖 `kiana-remote` 与 `kiana-types`；它不替代本地 `DaemonHost` 主路径。 |
| `kiana-remote` | `kiana-remote/src/lib.rs` | **兼容/远程集成面。** 被 bridge 和 entrypoints 引用；远程功能证明范围以 [CURRENT_STATUS.md](../../CURRENT_STATUS.md) 为准。不能把 workspace 依赖当成远程执行已启用的证据。 |
| `kiana-chrome-mcp` | `kiana-chrome-mcp/src/lib.rs`、`src/main.rs` | **独立/可选 MCP 面。** 有自己的 crate target；默认 CLI 主路径是否注册其中 handler 需看 feature 与入口调用。它不是五个固定 Harness 工具之一。 |
| `kiana-computer-mcp` | `kiana-computer-mcp/src/lib.rs` | **feature-gated 扩展面。** entrypoints 通过 `native-computer-use` feature 才开启其 native feature；不能据此声称默认构建具有桌面控制能力。 |
| `kiana-screens` | `kiana-screens/src/lib.rs` | **TUI/屏幕辅助面。** entrypoints 仍包含入口依赖；`kiana tui` 的产品状态按 [USER.md](../../USER.md) 说明，不能混同于当前工作台主路径。 |
| `kiana-url-handler` | `kiana-url-handler/src/lib.rs`、`src/main.rs` | **专用入口辅助。** 有独立库与二进制 target，并被 entrypoints 依赖；它处理 URL/唤起场景，不是 Agent 执行层。 |
| `kiana-quality` | `kiana-quality/src/lib.rs` | **独立质量/评估面，生产调用待确认。** 它依赖 domain，但从当前主路径依赖图未发现 daemon/core/entrypoints 对它的直接依赖。需阅读其 binary、脚本和测试后再判断实际使用方式。 |
| `kiana-capability-governance-supervisor` | `kiana-capability-governance-supervisor/src/lib.rs`、`src/main.rs` | **独立 supervisor target，主路径调用待确认。** 它没有通过 workspace 内 crate 依赖进入 daemon 的证据；其二进制的运行方式应单独查看。 |

### 不在 workspace 的目录

根 `Cargo.toml` 明确排除 `legacy/*`。该目录中的旧 TUI、屏幕捕获、输入、协调器和 UI crate 不参与当前 workspace 构建；它们是历史源码，不是运行时调用链。`reference/` 也不是 workspace 成员，只能作为审计资料。

测试实现通常位于 crate 的 `tests/` 或 `#[cfg(test)]` 模块中，且可能通过 `[dev-dependencies]` 获得生产 crate 不依赖的能力。例如 daemon 的集成测试可以注入 fake provider 或内存 EventStore；测试存在只能说明该测试路径被构造，不能直接提升整个产品能力的证明等级。

## 已确认的入口和主执行路径

当前源码中最容易复现的一条是 `kiana run -- ...`：

1. `kiana-entrypoints/src/bin/kiana.rs::main` 初始化进程并调用 `cli::main`。
2. `kiana-entrypoints/src/cli.rs::main` 识别 `run` 子命令，交给私有函数 `run_main`。
3. `run_main` 解析参数、检查互斥选项与 prompt，创建 session ID，再调用 `harness_run::run_envelope`。
4. `kiana-entrypoints/src/harness_run.rs` 创建本地 `DaemonHost`、client 和 `RequestMetadata`，然后通过 `KianaClient::run` 发出请求。
5. `LocalDaemonTransport` 将 versioned `RequestEnvelope` 交给 `DaemonHost::handle`。host 再把请求转交 `ControlPlane`。
6. core 经 `RunnerPort` 启动 `kiana-runner::KianaHarness`；daemon 提供的 `ModelClient` 根据配置选择脚本模型、fake 模型、provider gateway 或显式不可用模型。
7. Harness 发出模型请求并解析 `ModelOutput`。工具调用先经过 `kiana-runner/src/tools.rs` 校验并转换为 `CapabilityRequest`，再作为 runner event 返回 core。
8. core 执行身份、项目、策略、Gate、审批及执行许可检查。通过后将 `AuthorizedCapabilityRequest` 交给 `CapabilityBrokerPort`。
9. Broker 精确选择已注册 handler 执行，并把 `CapabilityResult` 返回 core；core 再把结果送回 Runner，供同一次模型循环继续使用。
10. 事件通过 EventStore 记录；`--receipt <run_id>` 经 `harness_run::receipt_envelope` 和 core 的 receipt 查询读回运行视图。

每一步的函数、数据和未证明事项见 [01-EXECUTION-FLOW.md](01-EXECUTION-FLOW.md)。该路径解释的是源码调用关系；究竟哪些 provider、文件操作和恢复行为已有 local/durable/live 证据，仍须看状态账本中的精确回执。


此外，context query 有两类源码路径：daemon 的 context 命令会经 Core/Broker 到 `kiana-query`；entrypoints 的 DirectConnect context API 是独立的只读路由。旧命令/工具 crate 也仍被 entrypoints 引用，所以不要把“普通 run 共用一条控制面”扩大成“CLI 内每条历史命令都已走同一条路径”。

当前工作树并非干净基线。只读追踪时，daemon-host 集成用例 `run_brokers_kiana_harness_tools` 实际触达 shell handler 后返回 `ResultUnknown`（错误含 `shell_exec_failed: Invalid argument (os error 22)`）；memory-write 与审批续跑的现有集成用例得到 `result_unknown:port_failed:result_unknown:result_delivery_stop_mismatch`，不符合各自期望的终态。这些回执不能当作完整 happy-path 通过证据，也不据此修改业务逻辑；下一轮注释应在进度文件保留它们，等相关 owner 处理后再复核。
