# 源码阅读路线

这份路线按“先看一项真实功能，再逐层下钻”的顺序组织。每完成一站，都应能解释一个可观察行为；遇到实现与状态账本的差异，先记录差异，不从规范目标反推实现。

## 阅读前：Rust 常见写法

以下概念会在多处出现，先记住它们在 Kiana 的实际作用，不需要逐个 crate 重学一遍：

| 写法 | 在本仓库里怎么读 |
|---|---|
| `Result<T, E>` | 函数可能失败。`Ok(T)` 是结果，`Err(E)` 是明确错误；入口可用 `anyhow` 补上下文，库通常保留结构化错误。不要把拒绝改写成普通成功值。 |
| `Option<T>` | 值可能不存在，例如没有 run ID、没有审批或没有可选存储。`None` 本身不一定是错误，需看调用方如何处理。 |
| `trait` / `impl Trait for Type` | trait 是一组行为约定，impl 是一个具体实现。Core 常依赖 `RunnerPort`、`EventStorePort` 等 trait，由 Daemon 注入实现，方便替换 adapter 和测试夹具。 |
| `dyn Trait` | trait object：运行时持有“任意实现该接口的对象”，不要求编译时知道具体类型。常与 `Arc<dyn Trait>` 配合用于长期共享的 adapter。 |
| `Arc<T>` | 原子引用计数的共享所有权。多个组件可持有同一个对象；它不自动保护对象内部可变数据。 |
| `Mutex<T>` / `RwLock<T>` | 在线程之间保护可变数据。同步版本不应跨 `.await` 长时间持有；Tokio 异步锁用于需要异步等待的场景。具体选择需看锁保护的临界区。 |
| `async fn` / `.await` | I/O 或并发工作可以挂起。读调用链时沿 `await` 看它等待的端口，以及失败如何返回。 |
| `&T` / `&mut T` / move | 借用只临时访问值；可变借用允许修改；按值传递通常会转移所有权。Rust 用编译期规则限制悬垂引用和数据竞争。先看参数签名就能判断函数是否接管或修改数据。 |
| 生命周期参数，如 `<'a>` | 引用有效期之间的约束。多数业务代码可先把它读作“这个返回的引用不能比输入数据活得更久”。 |
| 泛型，如 `T: Port` | 编译期参数，要求 T 满足指定接口；与 `dyn Port` 不同，泛型通常会为具体类型生成实现。 |

## 第一站：知道产品现状与入口

先读 [用户手册](../../USER.md) 的运行命令和诚实边界，再看 [项目地图](00-PROJECT-MAP.md)。然后打开 `kiana-entrypoints/src/bin/kiana.rs` 和 `kiana-entrypoints/src/cli.rs`。

- 阅读前：了解 argv、Rust `main` 和 `Result` 的含义。
- 重点：`main`、`cli::main`、`run_main`；不要一次性通读冻结的大型 `cli.rs`，用 `rg -n` 找到函数和调用点，再读相邻逻辑区。
- 阅读后应能回答：`kiana run` 从哪里分发？prompt、role、sandbox 在哪里校验？哪些选项互斥？CLI 返回非 Completed 时怎么显示错误？
- 下一步：打开 `kiana-entrypoints/src/harness_run.rs`，看入口如何构造请求。

## 第二站：看请求如何跨 crate

按顺序读 `kiana-entrypoints/src/harness_run.rs`、`kiana-client/src/lib.rs`、`kiana-protocol/src/lib.rs` 的 `RequestEnvelope` / `RequestBody`，再看 `kiana-ports/src/lib.rs` 中相应端口。

- 阅读前：记住入口参数不是授权凭据；协议对象是层间传递的数据。
- 重点：`run_envelope`、`client_on_host`、`LocalDaemonTransport::send`、`KianaClient::run`、`RequestEnvelope::run`。
- 阅读后应能回答：谁创建 session 和 request metadata？请求在哪一步变成版本化 envelope？transport 是进程内还是网络？协议 schema 校验在哪一层执行？
- 下一步：沿 `LocalDaemonTransport::send` 进入 `DaemonHost::handle`，因为它是入口到服务端逻辑的边界。

## 第三站：看 DaemonHost 如何装配程序

先读 `kiana-daemon/src/lib.rs` 顶部文件导读和 `DaemonHost` 字段，再跟 `DaemonHost::local`、构造函数和 `handle`。接着只挑 `model_client.rs`、`storage.rs`、`harness_capabilities.rs`、`harness_mcp.rs`、`harness_memory.rs` 中与当前请求有关的路径。

- 阅读前：理解 trait object、`Arc`、组合根（负责把依赖装配到一起的入口）。
- 重点：`DaemonHost` 持有哪些依赖、权限由谁判断、model client 如何选择、哪些 handler 注册进 broker、EventStore 如何被选中。
- 阅读后应能回答：CLI 与 Workbench 是否创建同一类 Host？哪些依赖由 Host 构造？哪些项目资源需先检查 trust？Host 自己是否执行模型循环？
- 下一步：查看 `kiana-core/src/lifecycle.rs`，读真正的请求生命周期。

## 第四站：读 ControlPlane 的一次运行

先从 `kiana-core/src/lib.rs` 的 `ControlPlane` 字段看依赖，再按顺序读 `lifecycle.rs`、`authority.rs`、`approvals.rs`、`dispatch.rs`、`events.rs`、`receipts.rs`。不要试图从 crate 文件名猜状态变化；跟着调用点读事件写入和错误分支。

- 阅读前：了解 RequestContext、CapabilityRequest、Run/Turn/Invocation 的基本区别。
- 重点：`start_run_with_history`、身份和信任上下文、policy → gate → approval、`dispatch_authorized`、`deliver_capability_result`、`record_event`、`read_receipt`。
- 阅读后应能回答：Run ID/Turn ID 由谁确定？`Allow` 为什么还不表示已执行？审批如何绑定特定请求？结果未知时为何不能盲目重试？receipt 如何从事件构造？
- 下一步：读 `kiana-runner/src/harness.rs`，看 Runner 如何向 ControlPlane 提交能力申请。

## 第五站：看模型循环和工具请求

先读 `kiana-runner/src/lib.rs` 的模块地图，再读 `model.rs`、`harness.rs` 的运行状态与 `dispatch` / step / `invoke_model` 逻辑，最后读 `tools.rs`。`harness.rs` 很大，先用 `rg -n` 定位函数，按状态创建、step、模型调用、工具回包、取消几段分开读。

- 阅读前：理解 `ModelClient` 是 trait、`ModelRequest` 是模型输入、`ModelOutput` 是规范化后的输出。
- 重点：消息如何组成、五个工具 schema 如何暴露、输出如何校验、工具调用如何变成 `CapabilityRequested`、工具结果如何回灌消息、什么情况产生 Completed/Failed/Cancelled。
- 阅读后应能回答：Harness 能不能直接执行 shell？消息历史保存在何处？预算和取消在哪些边界检查？返回纯文本和返回 tool call 有什么不同？
- 下一步：跟 `RunnerEvent::CapabilityRequested` 回到 core，再读 Broker。

## 第六站：看工具如何获批并执行

读 `kiana-policy/src/lib.rs`、`kiana-gates/src/lib.rs`，然后回到 core 的审批/dispatch 模块，最后读 `kiana-capability-broker/src/lib.rs` 和 daemon 中某个具体 handler。

- 阅读前：分清“模型想调用”“策略允许”“有待批准”“Broker 执行”四个阶段。
- 重点：结构化拒绝原因、Gate 单调收紧、审批绑定、Broker 的精确键 `(CapabilityKind, operation)`、handler 注册、执行前许可、handler 的副作用边界。
- 阅读后应能回答：一个未知工具名在哪里被拒？策略允许后为什么仍可能不能执行？实际文件/进程调用在哪个 handler？失败结果如何回到 Runner？
- 下一步：读 EventStore 端口和 JSONL adapter，核对事实记录来自哪里。

## 第七站：分清事件、历史和收据

先读 `kiana-ports/src/lib.rs` 的 `EventStorePort`，再读 `kiana-eventlog/src/lib.rs`、`jsonl.rs`，最后读 `kiana-core/src/events.rs` 和 `receipts.rs`。

- 阅读前：理解 append-only（追加写）与投影（从事实构造读视图）的区别。
- 重点：事件如何追加和读取、JSONL 如何处理并发/恢复、Receipt 读取时如何校验所有者和状态、哪些字段只是展示。
- 阅读后应能回答：EventLog 和 transcript 有何区别？重启后 receipt 与 continue 的能力是否相同？Receipt 能证明什么、不能证明什么？
- 下一步：阅读 memory/context 和 extension 模块，理解模型上下文如何获得，而不把上下文来源当授权来源。

## 第八站：模型上下文与外部适配器

根据兴趣选择一条纵向路径：provider 看 `kiana-daemon/src/model_client.rs` → `kiana-provider`；记忆和检索看 `harness_memory.rs` → `memory_retrieval.rs` / `kiana-query`；MCP 看 `harness_mcp.rs` → `mcp_stdio.rs` / `mcp_http.rs`；技能看 `harness_skills.rs` → `kiana-skills` / `extensions.rs`。

- 阅读前：先区分“给模型看的上下文/工具说明”和“ControlPlane 的授权事实”。
- 重点：配置从哪里来、项目级资源何时信任、adapter 如何映射输入输出、错误/unsupported 分支如何表现。
- 阅读后应能回答：provider gateway 是否处于当前证明范围？HTTP MCP 是否支持？memory ACL 由谁应用？项目 Skill 如何避免未信任加载？
- 下一步：读多角色请求和 workflow，了解如何复用同一执行脊柱。

## 第九站：读多角色工作与 Workflow

从 `cli.rs::run_main` 的 `--symposium` / `--packet` / `--review` / `--close` 分支开始，接着读 `harness_run.rs` 对应 envelope，再追 `DaemonHost::handle`、core symposium/collaboration 模块、work packet 和 `kiana-workflow`。

- 阅读前：掌握 Run、RoleSpec、WorkPacket、独立 Reviewer 的区别。
- 重点：各步骤是否生成新 session、正式交接对象是什么、角色是否经过同一 ModelClient/Runner、哪一层记录产物和决策。
- 阅读后应能回答：Builder 的输入是 transcript 还是 packet？谁能决定状态转移？Workflow crate 是否真的执行外部副作用？
- 下一步：如需继续，再读 `kiana-core/src/swarm_*` 与 daemon workflow service；只有确认生产调用后再把某模块列入主路径。

## 第十站：兼容、可选与测试专用代码

最后读 `kiana-commands`、`kiana-tools`、`kiana-bridge`、`kiana-remote`、`kiana-screens`、feature-gated MCP crate 和 `legacy/`。同时看 crate manifest 的 `features`、`dev-dependencies`，并搜索实际消费者。

- 阅读前：记住“workspace member”只说明被 Cargo 纳入，不等于当前产品入口在用。
- 重点：入口路由是否到达它、是否仅由测试依赖、feature 默认值、`legacy/` 是否被根 workspace 排除。
- 阅读后应能回答：该模块属于当前 `kiana run` 主路径、额外入口、测试辅助还是历史兼容？它的能力是否有对应回执？
- 下一步：把尚未确认的调用关系写入 `COMMENT_PROGRESS.md`，不要凭名称升级分类。

## 建议的学习闭环

读完一站后，用一个具体问题复述代码，例如“一个模型返回 `shell` 调用时，为什么命令不会在 Harness 里直接运行？”然后用源码符号核对答案。若答案只能引用规范文档或注释而找不到调用点，应标成待验证，并继续搜索生产调用者和拒绝测试。
