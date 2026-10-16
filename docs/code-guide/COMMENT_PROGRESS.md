# 注释与导读进度（阶段一导读、阶段二源码注释）

## 已扫描

- workspace 的 33 个 Cargo 成员、根 manifest、主要 crate manifest 和 crate 根入口；`legacy/*` 明确不在 workspace。
- 普通 `kiana run` 从 CLI 到 client、DaemonHost、ControlPlane、KianaHarness、Broker、EventStore/Receipt 的生产源码调用边。
- Provider 选择、五个模型可见工具、context/query、workflow automation、symposium/spawn 与主要兼容 crate 的依赖边。
- 阶段二源码候选清单：[SOURCE_INVENTORY.tsv](SOURCE_INVENTORY.tsv)，逐文件记录语言类型和当前注释状态。
- 清单包含 3,053 个候选文件：2,646 个 Rust 文件、407 个 Shell/JS/Python/HTML/YAML/Makefile 源码或自动化文件；包含 tests、examples、legacy、Electron 和 GitHub Actions workflow。
- 清点时 182 个候选源码路径已有工作区改动，清单逐项标记；`.claude/helpers/` 的 42 个脚本和 `.claude/agents/` 的 1 个 YAML 是本机忽略文件，列入清单但暂不修改。`.claude-flow/config.yaml` 文件头标为生成配置，`.swarm/` 是 RuFlo 本地状态，连同 `graphify-out/`、`dist/`、依赖目录和构建产物排除。

## 已完成

- 导读：[00-PROJECT-MAP.md](00-PROJECT-MAP.md)、[01-EXECUTION-FLOW.md](01-EXECUTION-FLOW.md)、[02-READING-GUIDE.md](02-READING-GUIDE.md)。
- 源码注释：`kiana-core/src/lib.rs` 顶部模块职责；`ControlPlaneRuntimeConfig`、`SessionBinding`、`ControlPlane`、`CoreError` 的类型/字段说明。
- 源码注释：`kiana-core/src/lib.rs` 中 `PathLockLease`、`acquire_workspace_resources`、`RunTerminalScope`、`TerminalScopeGuard`、`PersistedApprovalCursor` 的资源生命周期与 Rust RAII 说明。
- 源码注释：`kiana-entrypoints/src/stream_render.rs` 已有模块位置、广播游标、终态等待和跨分片脱敏说明；这些是阶段一成果，尚未按阶段二标准逐段复核。
- 源码注释：干净的 `kiana-provider/src/lib.rs` 与 `request.rs`，补充 provider gateway 的 admission 边界、配置代次、请求编译、协议分派和图片/预算拒绝说明；`config.rs`、`resolver.rs`、`response.rs` 因已有未提交修改未触碰。
- 源码注释：`kiana-entrypoints/src/harness_run.rs` 已按本阶段标准审阅整份文件；覆盖入口命令、run/continue/cancel/approval/query/multi-role 路径、请求身份与会话上下文、WorkPacket 读取、响应/prompt 转换、sandbox/trust helper 及 5 项单元测试。模块、重要字段、非平凡函数与关键分支均已补充或审查中文注释，文件标为阶段二完成。
- 源码注释：`kiana-daemon/src/model_client.rs` 测试专用 `legacy_fixtures` 的装配、ProviderModelClient 请求/预算/重试/流式策略、事件聚合状态机、错误分类、工具/消息/响应协议转换、测试 mock 与 HTTP helper 已分批审阅。测试函数已有场景说明，但函数内部仍需按教学级标准继续检查；文件整体仍为部分完成。
- 源码注释：`kiana-core/src/lifecycle.rs` 已按本阶段标准逐函数审阅整份文件；补充 Run 身份与事件序列、角色/department 校验、蒸馏和预算分支、sandbox/run scope、模型 assignment、v1 continue 与 v2 新 turn、turn-bound input、取消 CAS/stop proof、RunnerEvent 驱动和 terminal/Receipt 的逻辑块说明。保留原有详尽模块/入口注释并为缺少说明的 helper 增加函数文档。
- 源码注释：`kiana-daemon/src/harness_capabilities.rs` 已逐函数审阅全文件；覆盖 Broker handler 注册、临时工作区写入/发布、patch 路径与数据治理校验、工作目录 containment、sandbox 环境清理、子进程取消/超时/停止证明/输出预算，以及 10 项测试和辅助构造函数。
- 源码注释：`kiana-core/src/approval_binding.rs` 已补充文件职责、值对象数据流、绑定字段含义、serde/schema/digest 校验、HumanInbox 状态转换和当前未接入生产路径的边界；记录过期分支只返回错误、`consume` 修订号饱和等实现细节。
- 源码注释：`kiana-core/src/approvals.rs` 已复核新请求授权、proof/version 决策、审批重放、Run-bound 恢复和直接执行路径；修正“唯一入口”“单次凭证”“多人审批”等过宽描述，解释 Store proof 边界、失效错误处理与策略/Gate helper。
- 源码注释：`kiana-eventlog/src/lib.rs`、`jsonl.rs`、`journal_core.rs`、`memory.rs`、`event_store_core.rs`、`kiana-domain/src/journal.rs`、`kiana-ports/src/lib.rs` 已围绕 EventStore 端口、事务/逐事件追加、流版本、JSONL 恢复与内存 adapter 补充数据流和语义说明；尚未覆盖 EventLog crate 其余子模块。
- 文档核对：`01-EXECUTION-FLOW.md` 中普通 CLI 路径已区分空历史 `KianaClient::run` 与其他历史入口；补齐 SDK 包装函数名，并修正预期流版本被误称为全局游标的注释。

## 尚未完成

- 主路径细注释：`stream_render.rs` 仍需按阶段二标准复核；`kiana-daemon/src/model_client.rs` 的复杂测试 fixture 仍待逐段细读；Provider adapter 的失败/重试/流式协议和 daemon 模型选择也需要再沿真实调用链核对。
- 当前共享工作树后续又出现一批 Git 修改：`git diff --stat -- 'kiana-*/src/**'` 显示 135 个源文件、约 14,182 行新增与 1,611 行删除。该批次横跨 Core、Daemon、Domain、Entrypoints、EventLog、Provider、Runner 等模块；注释任务只处理了其中上述文件，不能把这批代码修改视为已完成注释审阅。
- 辅助路径：`kiana-query` 的 context/memory 路径、MCP/skills 装配、`kiana-workflow` 与多角色流程仍需逐文件阅读和补充调用关系。
- 入口兼容/可选面：`kiana-entrypoints/src/runner.rs` resident teammate 分支、`kiana-commands`/`kiana-tools`/`kiana-bridge`/`kiana-remote`、feature-gated MCP、TUI 与独立 quality/supervisor target 仍未完成。
- 仍按 [SOURCE_INVENTORY.tsv](SOURCE_INVENTORY.tsv) 分模块推进；只编辑已确认源码语义的注释/文档，每批开始先看 Git，保留共享工作树里其他 Codex 的业务代码改动。

## 已确认的调用关系

- `kiana run`: `bin::main` → `cli::main` / `run_main` → `harness_run::run_envelope`（默认流式时先走 `stream_render`）→ `KianaClient` → `LocalDaemonTransport` → `DaemonHost::handle` → `ControlPlane` → `KianaHarness`。
- Harness 工具调用转换为 `CapabilityRequest`；Core 完成策略、Gate、审批/执行许可后才调用 Broker。Broker 结果经 Core 交回 Runner。
- 本地 Host 生产构造使用 `JsonlEventLog::open_default`；测试/注入构造也可使用 `MemoryEventLog`。模型由 daemon 选择 cassette、显式 fake、provider gateway 或 unavailable 实现；provider gateway 源码不等于 live 证明。
- `kiana-query` 有 daemon Core/Broker context handler，也有 entrypoints 的只读 DirectConnect 路由；它不是五个模型可见工具之一。
- `kiana-workflow` 的 planner 被 core automation 调用；daemon 的 `WorkflowQueueService` 有 start API，但本次未找到默认 Host 构造路径启动它的生产调用。
- `kiana-entrypoints/src/runner.rs::run_assistant_turn*` 仅测试编译；另一个 resident teammate/poll loop 有 CLI 调用，需独立读其运行职责。

## 待确认与本轮测试回执

- `cargo test -p kiana-daemon --test daemon_host run_brokers_kiana_harness_tools --locked --offline -- --exact --test-threads=1`：失败；触达 shell handler 后为 `ResultUnknown`，错误含 `shell_exec_failed: Invalid argument (os error 22)`。它证明链路到达 handler，不证明工具成功，也不能单据此归因于代码而非本机环境。
- `cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1 model_written_memory_without_evidence_is_rejected_and_stays_unsearchable` 与同命令过滤 `wire_approval_proof_retry_resumes_original_run`：均失败，返回 `result_unknown:port_failed:result_unknown:result_delivery_stop_mismatch`，未达到测试期望终态。只记录，不修改业务逻辑。
- `cargo test -p kiana-daemon --test daemon_host streamed_run_writes_one_run_delta_per_turn_to_the_ledger --locked --offline -- --exact --test-threads=1`：失败；测试按扁平 event JSON 读取新 JournalFrame，不能据此断言 delta 未落盘。
- `cargo test -p kiana-daemon --test daemon_host run_subscription_receives_ordered_deltas_and_terminal_response --locked --offline -- --exact --test-threads=1`：失败；先前 Completed 与有序 delta 断言通过，后续收到 Usage 而非测试预期的 Terminal，需消费完整事件序列再评估。
- 测试结果均来自只读研究代理在当前共享 dirty checkout 上执行；不是干净基线结论。其他 stream/EventLog 断言与当前事件流/JournalFrame 表示的关系仍待核对。
- `harness_run::tests::bypass_permissions_mode_is_rejected` 未清理 `KIANA_PERMISSION_PROFILE`；由于该来源优先于 `permission_mode`，若测试进程带入该变量可能在未覆盖 mode 分支时失败。本任务只记录，未改测试。
- `load_work_packet` 经 `normalize_role_path` 做词法路径检查，但本函数没有 canonicalize 最终文件路径；符号链接目标的边界需结合调用者信任和实际读取约束另行核验，本任务未改行为。

## 本批验证

- `cargo fmt --all --check`：当前全部注释批次通过，exit 0。
- 编译：`cargo check -p kiana-entrypoints --tests --locked --offline`、`cargo check -p kiana-daemon --tests --locked --offline`、`cargo check -p kiana-core --locked --offline` 均通过；有 workspace unused/dead-code/unreachable-pattern warnings，未处理。
- 聚焦测试：`cargo test -p kiana-entrypoints --lib harness_run::tests --locked --offline -- --test-threads=1` 为 5/5 通过；daemon model fixture 与 capability 测试结果见本节下方。
- `cargo fmt --all --check` 和 `cargo check -p kiana-daemon --tests --locked --offline`：harness capability 注释后通过；编译输出有 workspace warnings，未处理。
- `git diff --check` 与逐行源码差异审计通过；`lifecycle.rs`、`harness_run.rs` 和 `model_client.rs` 改动只有注释，没有非注释代码行改动。
- `cargo test -p kiana-daemon --lib legacy_fixtures::tests --locked --offline -- --test-threads=1`：当前 dirty checkout 中 19 个测试有 18 个通过；`provider_wrapper_maps_tool_calls_and_final_text` 返回 `tool_result_missing_call_id`。未在未修改基线重跑，不能判断该失败是否早已存在；本任务未修改测试或业务逻辑。
- `cargo test -p kiana-daemon --lib harness_capabilities::tests --locked --offline -- --test-threads=1`：当前 dirty checkout 中 10 个测试有 2 个通过、8 个失败。workdir 越界与 read-only patch 拒绝通过；5 个 shell 测试返回 `shell_exec_failed:Invalid argument (os error 22)`，3 个 workspace-write 测试返回 `execution_workspace:write_scope_required`。未在未修改基线重跑，未修改测试或业务逻辑。
- `kiana-daemon/src/model_client.rs`：产品选型与 wrapper、测试 fixture 装配、provider 适配、流式转换、DTO 映射和 mock/helper 已完成一轮注释；测试函数内部仍待细读，故保留“部分完成”。
- `cargo fmt --all --check`、`cargo check -p kiana-core --locked --offline` 和源码差异审计覆盖了 lifecycle 批次，均通过。
- 多角色请求的完整产物写入/回执边界仍需沿 daemon `RequestBody::Symposium/Spawn` 和 core collaboration 逐段核验；旧入口面哪些具体命令绕过 DaemonHost 也未穷尽。

## 2026-10-10 追加注释审阅

- `kiana-domain/src/text_normalization.rs`：将英文文件头替换为中文职责/位置说明，标出当前只有领域测试调用、尚未找到生产调用；补充 profile、敏感策略、结果结构及字段含义，逐步解释规范化、token、语言提示、脱敏和摘要数据流，并说明摘要与 `secret_free` 的证明边界。
- `kiana-core/src/authority_read_model.rs`：按实际投影函数重新核对已修改注释，说明输入事件来源与顺序由调用方负责、`source_cursor` 未验证、投影本身没有生产调用者；修正“手工改投影必然是 bug”等过宽断言，并明确 Grant、Cell、Budget、Lease 的读取范围与检查边界。
- `kiana-core/src/authority.rs`：补充配置修订摘要、授权代次和 EventLog stream version 的区别，说明多聚合 CAS 保护的范围，以及授权配置事件提交后审批/运行清理失败不会回滚事件、同配置重试不会续做清理。
- `kiana-core/src/recovery.rs`：说明待审批列表中可选 Run ID 的实际用途、恢复输入重新从受控 Store 读取，以及 resume claim 先于 Runner/Cell 恢复持久化；恢复失败可能留下已 claim 的状态。
- `kiana-domain/src/extensions.rs`：补充扩展清单、签名输入、短期执行 scope、Broker/Daemon 双重准入、依赖图和绑定快照的数据流与字段语义；区分 Ed25519 manifest 签名与无密钥 SHA-256 摘要，并说明网络 allowlist 目前不会启用 Network capability。依赖图/绑定快照当前只找到测试构造，未找到生产调用点。
- 静态问题记录：`text_normalization.rs` 的 malformed-JSON helper 对合法 JSON 直接返回未命中；普通文本 marker 也不匹配带引号的 JSON 键（例如 `"token":"value"`）。它还只看原文首字节，带前导空白的 malformed JSON 不进入该扫描。由此存在结构化凭证未触发敏感处理的路径；本任务没有改检测行为。
- 需调用方明确处理的新鲜度边界：`ExtensionBindingSnapshot::revalidate()` 没有当前 `authority_epoch` 入参，只能校验快照里保存的 epoch 非零，不能发现当前授权代次变化；本文件的 graph/binding 调用目前仅见测试，尚不能判定其对生产造成影响。
- 本批没有运行测试、构建或格式检查。代码路径只增加/修订注释；另一 Codex 在这些源文件里的逻辑改动按工作区状态保留，未被本任务覆盖或修复。

## 2026-10-10 脱敏注释批次

- `kiana-domain/src/redaction.rs`：按实际调用点改写文件级说明，区分通用结构化脱敏、显式哨兵扫描、有界 profile 编码和跨分片流式脱敏；补充 profile/结果/finding 字段语义、错误边界、敏感字段 allowlist、URL/JWT 启发式和 `StreamingRedactor` 状态机说明。确认 scanner 由多个 Domain/Core/Runner/Daemon/Entrypoints 边界显式调用，不是自动包围所有输出的中间件；`scan_secret_channels()` 当前直接调用只见 Domain 测试。
- `kiana-domain/src/storage_security.rs`：补充存储安全值对象在 EventStore、Artifact、ACP、StoragePreflight 与 Port 能力矩阵中的位置；说明 secret-free 是“脱敏会改变就拒绝”，不是自动修复或独立 scanner；解释加密绑定、文件身份快照、权限位、路径词法检查和平台能力字段，并标明这些领域对象不执行加解密或文件操作。
- `kiana-domain/src/extensions.rs`：复核 scope/contract 准入说明，明确 contract 的 `check()` 不读取当前时间或 registry generation，须由 `validate_at()` 与 Daemon `ExtensionAdmission` 补齐上下文检查；修正 Ed25519 64 字节签名对应 128 个小写十六进制字符的描述，并将 DFS 首次出现补为“深度优先搜索（Depth-First Search，DFS）”。
- 静态问题记录：`scan_secret_sentinels()` 的 `sk-` 前缀规则只有在显式调用 scanner 的路径才生效；通用 EventStore 的 `validate_event_for_storage()` 调用 `storage_security::validate_secret_free()`，后者只比较 `redact_value()` 前后结果，不调用 sentinel scanner。因此该规则不覆盖这条通用写入校验路径。其他显式 scanner 调用仍按各自调用点生效；本任务未改任何执行逻辑。
- 边界说明：`contains_json_secret_key()` 只解析合法 JSON；有界文本编码器另有针对受限大小畸形 JSON 的轻量键名预检，但它不是 JSON 解析器。`scan_secret_sentinels()` 的 `Ok(())` 仅表示当前有限规则没有命中，不构成任意 secret 均不存在的证明。
- 诊断摘要边界：普通 finding 的 `marker_digest` 摘要规则标识；`Echo` finding 摘要调用方提供的哨兵本身。结构中没有存哨兵明文，但已知或低熵哨兵仍可被猜测，因此不能把摘要当成匿名化凭证。
- 平台声明边界：`StorageSecurityCapabilities::current()` 按编译目标给出能力标志，不探测具体挂载点/adapter；`StorageFileIdentity::validate()` 仅校验快照字段，不重新访问文件系统或消除 TOCTOU。`StorageEncryptionBinding` 在当前源码中只找到测试直接构造，不能据此声称加密路径已经接入。
- 本批只新增/修订中文注释与进度文档，没有运行测试、构建、格式化或提交。目标文件原有的其他 Codex 逻辑改动保留，未在本任务中修改。

## 2026-10-10 事件追加路径复核

- `kiana-core/src/events.rs`：沿 `lifecycle.rs` 的 start/continue 调用核对 `run.prompt` 载荷确含 Run ID，故 `aggregate_for_event()` 将其路由到 Run stream，Unknown fence 读取的也是这条流；补充关联 ID、载荷脱敏/扫描、artifact 引用、CAS 重试和 capability result wrapper 的边界说明。修正 artifact 上限文字：256 指排序去重后的唯一引用数，4096 按 `String::len()` 是 UTF-8 字节。
- `kiana-core/src/lib.rs`：修正 `RunTerminalScope.recorded` 字段注释。全仓唯一使用点仅对 `AsyncMutex<bool>` 加锁，没有读取或更新 bool；互斥用于串行化同一 Run 的终态提交，是否已有终态由 EventLog 查询决定。
- 静态风险记录：`prepare_event_payload()` 先对原始 JSON 调用无深度上限的递归 `redact_event_value()`，之后才检查最大 32 层；极深嵌套输入可能在深度拒绝之前导致递归栈耗尽。本任务只在注释中标记，没有调整检查顺序或算法。
- 元数据边界：`prepare_event_payload()` 只创建 Audit `RedactionProfile` 并记录其 digest，实际先调用普通 `redact_event_value()`，本函数对脱敏 payload 及 EventStore 对完整 RuntimeEvent 都按 1 MiB 上限校验，并未执行 `encode_bounded_value()` 的 Audit 默认 16 KiB profile 上限。`RuntimeEvent::validate_redaction_metadata()` 也只校验摘要形状，不解析 profile 重新核验策略；不应把所存 digest 当成所有 profile 限制均已执行的证明。
- 本批未运行测试、构建、格式化或提交。以上风险和边界均未修改业务逻辑。

## 2026-10-10 事件契约调用关系复核

- `kiana-domain/src/event_contracts.rs`：按最新工作区源码复核事件类型登记表、版本兼容、未知类型策略、迁移解析、payload 字段白名单与类型专属校验；检查当前生产调用点后，补充 `validate_runtime_event()` 在 EventStore 追加、Core 撤销事实分页扫描、凭证恢复投影、Quality 规范化中的调用关系，以及 ACP 对待审批快照逐项执行 `validate_event_payload()` 的路径。
- `resolve_event_payload()` 当前仍只有领域契约测试直接使用，尚未找到生产读取路径；旧版迁移目前是保持 payload 形状的 identity transform，且迁移结果仍按当前契约校验。注释明确表示这是读取解释器，不会回写事件或执行事件动作。
- 文件内的事件白名单、双生产者 schema 分支和模型预算身份交叉校验已有字段及拒绝原因说明。本轮未发现需要通过注释修复的业务行为；任何 schema/白名单变化仍需同步事件生产者、EventStore 与投影消费方。
- 本批只更新注释和进度文档；未运行测试、构建、格式化或提交。事件登记规则及其他 Codex 的工作区逻辑改动均保持原样。

## 2026-10-10 模型预算准入注释批次

- `kiana-core/src/model_budget.rs`：按 DaemonHost 装配、Harness attempt、Provider admission 和 EventStore 事件流追完 `reserve_prepared()` → `consume_prepared()` → transport → `settle()`；为 JournalModelBudget、端口方法、墙钟读取及共用 `reserve_call()` 补充职责、参数语义、提交副作用、错误传播和多聚合 CAS 原因。
- 补充模型预算的状态关系：每次重试各有 attempt reservation；成功用量结算为已报告的总 token；用量未知时按预留上限计费；超额结算先持久化再返回错误。Run/authority/budget/attempt 的快照版本进入同一 TransitionBatch，避免各 stream 在检查后被并发修改。
- 当前未找到生产代码调用无 PreparedModelCall 的 `ModelBudgetPort::reserve()`；生产 Harness 路径调用 `reserve_prepared()`。预算预留 fact 的 `expires_at` 不会使未结算记录自动释放，崩溃后未结算的上限继续占用 Run 预算；Provider permit 自身的过期检查在 `consume_prepared()`。
- 静态疑点：`reserve_call()` 读取的 `authority_version` 来自 Authority EventStore stream_version，却将它传给 `BudgetReservationFact::new()` 的 `authority_epoch` 参数；`authority.rs` 明确区分 stream version 与语义授权 epoch。未改该值，需在实现任务中核对是否存在字段语义混用。
- 本批只增加或修订注释及进度记录；未运行测试、构建、格式化或提交。另一 Codex 在 `model_budget.rs` 中的 permit 脱敏逻辑修改保持原样。

## 2026-10-10 EventStore observer 边界复核

- `kiana-eventlog/src/stream.rs`：复核并修订最新工作区注释，明确该 `StreamEventStore` 只在 `commit_transition()` 返回首次 `Committed` 后广播；`append*` 和读取 API 直接代理、不通知；`Conflict`、`Replayed`、`Unknown` 与底层 PortError 都不产生通知。`Committed` 代表底层 adapter 确认提交，不保证一定是磁盘落盘，因为 adapter 也可为内存实现。
- 核实调用关系：当前 workspace 直接注册/实现 `EventStoreCommitObserver` 只见 `kiana-eventlog/tests/oa06_commit_observer.rs`；Daemon 使用的是另一个 `kiana-daemon::run_stream::StreamEventStore` 与 `RunStreamBus`。文件级说明不再把 observer 写成现行 UI/MCP 生产订阅路径。
- 标明 `committed_cursor` 只统计这个 wrapper 创建后观察到的最大 transition cursor；初始化不从底层读取，其他 handle 或 append 写入也不计入。观察者按注册快照顺序逐个 await，任何慢回调都会延迟 commit 调用返回，且没有本地超时；回调失败只进有界诊断记录，不改变已提交 outcome。
- 本批只更改中文注释和进度记录；未运行测试、构建、格式化或提交。

## 2026-10-10 AUT-15 intent/observation 合约复核

- `kiana-core/src/automation_dispatch.rs`：修正其他 Codex 添加的模块说明中过度推断的分发链路。实际文件只是转调 Domain validator；当前 workspace 没有生产调用者，不执行 Policy/Gate/worker/Broker，也不写 `dispatch.automated.*` 事件。`CURRENT_STATUS.md` 与 AUT-15 baseline 将其明确记为 source-level、partial 合约。
- `kiana-domain/src/automation_dispatch.rs`：补充 intent、observation、状态 enum、身份和摘要字段说明；逐项解释文本/摘要限制、intent 自身 SHA-256 指纹及其非签名边界；记录 Succeeded/Failed 都归为 `Observed`、Unknown 保持 `Unknown`，validator 只比较调用方提供的快照，不查询当前授权或 worker。
- 当前可确认的输入到输出流是 `AutomationDispatchIntent + AutomationObservation → Domain 结构/摘要检查 → Claimed 状态及五项绑定比较 → 返回重算后的 intent 副本`。这不证明 worker 执行过 action，也不持久化结果；调用方如果需要 durable dispatch 或可信观测，仍需其他实现路径。
- 本批只修订源码注释和进度文档；未运行测试、构建、格式化或提交。AUT-15 业务实现和其他 Codex 的工作区修改均未更动。

## 2026-10-10 Core workflow automation 注释批次

- `kiana-core/src/automation.rs`：按实际分发关系重写文件级说明，区分 Domain DTO、`kiana-workflow` 纯 planner 与 Core 事件提交/执行适配层；补充命令入口、能力守卫入口以及完整的命令到观测数据流。planner 的 `WorkflowPlanIntent` 在本路径被计算但不持久化，事件重放仅使用 planner 的新状态并丢弃 effect。
- 为聚合 key、时间戳、运行事件引用、runtime proof、authority 快照，以及 workflow snapshot/command/reject/load/commit/observation/proof/cancel helpers 补充调用者、输入输出、事件副作用、错误传播和设计边界说明。
- 标出 command 事实提交与外部 dispatch/cancel 之间不存在跨系统事务：dispatch 错误按 `ResultUnknown` 处理，提交后的取消动作失败不会回滚已提交请求；外部幂等重放不会再执行 effect；观测写入最多重试 3 次 CAS 冲突，但不会重试外部动作；取消请求也不代表外部副作用已回滚，终态仍需 Reconcile 证据。
- Reconcile 注释按 AgentTask/Capability/SubWorkflow 三类来源说明 Company Run 身份检查、能力事件映射、审批等待、lease 过期降为 Unknown 和证据引用边界。`guard_workflow_capability()` 只检查匹配 workflow execution ID 的实例/会话/角色归属；普通能力请求仍由后续 Company、Policy、Gate、审批与 Broker 守卫决定。
- 本轮在 `automation.rs` 的源码差异仅有注释；保留另一 Codex 已有的 `Safe | Ask` Profile 拒绝逻辑，没有改它。没有发现需在本轮注释范围内修正的 workflow 业务行为；`now_ms()` 在系统时钟早于 Unix epoch/读取失败时返回 0 是现有行为，注释已如实标出。
- 本批只增加/修订注释及此进度记录；未运行测试、构建、格式化或提交。

## 2026-10-10 Durable workflow 领域与 planner 批次

- `kiana-domain/src/automation.rs`：重写文件级调用链，说明 DTO 在通用 command、Core Automation、纯 planner、EventStore replay 间如何流动；补充 definition/node/artifact/trigger/instance/execution/status/command/authority/proof/envelope/effect/intent 的字段语义、Serde 兼容、调用边界和摘要用途。
- `kiana-workflow/src/lib.rs`：修正 crate 头注释，把测试覆盖的简单 `WorkflowState` 转移图与生产 Core 调用的 `durable` planner 区分开；说明两者都不持有执行权，避免误认成另一条产品执行循环。
- `kiana-domain/src/automation_signal.rs` 与 `kiana-core/src/automation_signal.rs`：说明 AUT-18 领域 fact validator/Core facade 的真实 scope、字段格式、错误和无副作用边界。生产代码未发现实际调用者；`consumed == state` 只检查单条结构，不维护 signal_id 历史、不验证 checkpoint/epoch 新鲜度，也不执行 worker pause/resume。
- `kiana-domain/src/automation_retry.rs` 与 `kiana-core/src/automation_retry.rs`：补充 AUT-16 输入字段、Unknown-first 分支、attempt/deadline/budget/idempotency 门槛和稳定错误说明；明确 Core facade 仅转发、生产路径尚无调用者，且它与 durable workflow 的纯节点 Retry 命令不是同一实现。
- `kiana-domain/src/automation_cancellation.rs` 与 `kiana-core/src/automation_cancellation.rs`：说明 AUT-17 fact 状态字段、generation/epoch 仅为载荷值、单条状态矩阵检查范围和无存储/worker 副作用边界；明确它没有接入实际 workflow Cancel effect 路径。
- `kiana-domain/src/automation_compensation.rs` 与 `kiana-core/src/automation_compensation.rs`：说明 AUT-20 plan 字段、digest、原 permit 禁用条件及其仅为 DTO 校验的边界；与已存在但独立的 durable workflow `Compensate` 命令路径作区分。
- interval trigger 注释逐项说明绝对时间游标、Skip/FireOnce/CatchUp 的实际计算差异、单批最多 32 项、溢出错误及返回字段；`validate_shape()` 的边界明确限定为 shape，包含 `first_at >= expires_at` 仍可能通过的源码边界。
- `kiana-workflow/src/durable.rs`：审阅纯规划职责和状态转换；补齐 plan intent、输入摘要、节点 lease、审批/信号、trigger occurrence 去重/队列、重试、补偿与执行 effect 的解释。Start intent 缺少新 instance ID 的边界已写明。
- 真实调用链经只读追踪：CLI `kiana command`、Web `POST /api/command`、Workbench chat `/command` → 通用 RequestEnvelope → DaemonHost → Core `workflow.command.v1` → Core EventStore 状态折叠与 proof/authority 构造 → planner → Core CAS 提交 → Company StartRun 或 `authorize_and_execute` → RecordObservation/Reconcile → 后续 Advance。协议专用 workflow builder 当前未发现生产调用者；旧 `kiana tasks workflow` 是兼容命令，不是 durable workflow。
- 静态确定的问题：Resume 对非终态 Paused 实例仍保持 Paused；ResultUnknown 节点为 terminal 且 observe 拒绝再次更新，阻断后续已知证据收敛；FanIn 结果只写 execution.output，未进入 instance.outputs；SubWorkflow 成功输出只写父 node.output，未进入 parent.outputs；Trigger Replace 只标记旧实例 CancelRequested/排队，但没有返回 Cancel effect，Core 不会从此分支向旧外部运行发取消请求。
- 其他明确边界/风险：Approval/WaitSignal lease 过期仅拒绝 Decide/Signal，不会自动终结等待节点；Replace 与并发 active 检查把 ResultUnknown 当 terminal，可能不再计为 active；subworkflow 节点 ended_at 取父子 created_at 最大值；Start intent 对新实例没有准确身份。artifact 在此 planner 只检查逻辑输出 key，不证明文件或 ArtifactReceipt 已创建。
- 需要产品契约确认的风险：Interval 首次时间晚于 trigger expiry、CreateInstance 只要求输入 schema key 存在而不拒绝额外 key、补偿节点按 ID 排序、嵌套子流程预算是否应累计进父实例 steps_used。
- AUT-18 状态账本文案与 validator 能力存在差距：账本提到 single-consume state，但本文件只检查一条 fact 的 `consumed` 与 `state` 一致，不能拦截不同记录重复 signal_id。已记录边界，未改状态账本或执行逻辑。
- AUT-16 分类器信任调用方提供的多组布尔快照，未校验它们之间的一致性；如 `effect_started=true`、`effect_known=true`、`transient_pre_send=true` 可在其余门槛满足时得到 RetryNewAttempt。是否违反 AUT-16 输入契约需维护者判断；本批仅在注释中说明调用方前提。
- AUT-17 validator 未强制 `Stopping => stop_requested`，也未校验 generation/epoch 当前性或状态转移历史；`ResultUnknown`/`Fenced` 只检查少量局部条件。完整矩阵是否需要更强约束取决于 AUT-17 契约，作为风险保留，未改校验。
- AUT-20 动作摘要必须不同时仅按字符串比较；SHA-256 hex 校验接受大小写，因此同一 digest 的大小写不同表示可被视为不同动作摘要通过校验。是否违反计划语义需契约维护者确认；本批未改变比较方式。
- 本批仅新增/修订中文注释与进度文档；没有运行测试、构建、格式化或提交。其他工作区 Codex 的改动保留；没有改任何业务逻辑。

## 2026-10-10 AUT-14 reservation 与 AUT-19 FanOut 注释批次

- `kiana-domain/src/automation_effect_reservation.rs`：逐字段解释 action、执行身份、capability、authority/config/policy 围栏、budget/path scope 摘要、可选审批摘要、状态和 reservation digest；说明 `validate()` 的格式与自摘要检查顺序，以及 digest 不等于签名或授权。
- `kiana-core/src/automation_effect_reservation.rs`：说明 Core facade 仅向 Domain 的可变内存 ledger 转发，当前没有生产调用者或 Broker/EventLog/worker 路径；成功只登记当前对象中的 `Reserved` fact。
- reservation 的当前数据流是“调用方构造 fact → 校验形状和 digest → 对照三项版本快照 → 按幂等键返回相同记录或拒绝冲突 → 新记录规范成 Reserved 并写入 BTreeMap”。账本没有持久化、恢复、删除、过期清理、容量上限或状态迁移；与 AUT-14 baseline 所称 bounded ledger 存在实现差距，已标出但未改行为。
- `kiana-domain/src/automation_fanout.rs` 与 `kiana-core/src/automation_fanout.rs`：说明 AUT-19 是计划 DTO 的静态校验和 Core 转发门面，当前未找到生产调用者；它不同于 durable workflow DAG FanOut/FanIn，不创建子实例、不做权限或真实并发/budget/scope 管理、不执行 fail-fast、不汇总结果。
- FanOut validator 对 child 数量/唯一性、深度、并发声明、预算乘积、TTL、由调用方提供的 scope/cycle 布尔声明、fan-in 标志和自摘要执行结构校验。确认的算术边界：预算使用 `saturating_mul()`；乘积与父预算都饱和为 `u64::MAX` 时，`>` 比较仍放行。此处只记录现状，不修改算术或 validator。
- 合并了前几批在来源清单中重复追加的 AUT-16/AUT-17/AUT-20 源文件行，保留一条记录并更新其状态；本次更新 AUT-14/AUT-19 四个源文件行。
- 本批仅更改中文注释和注释进度/来源清单；未运行测试、构建或格式化，未提交。其他 Codex 的代码改动保留。

## 2026-10-10 AUT-21 启动恢复 fact 注释批次

- `kiana-domain/src/automation_boot_recovery.rs`：补充恢复 fact 的文件职责、数据流、游标/generation/epoch/Unknown/index/引用字段语义、SerDe 未知字段行为、错误码和无 I/O 边界；说明摘要只能校验载荷自洽，不证明来源可信。
- `kiana-core/src/automation_boot_recovery.rs`：说明 Core API 只是复用 Domain validator 的只读 facade；当前没有生产启动恢复调用者，不读取 EventStore、不重建投影或索引、不扫描 Unknown、不追加恢复事件、不恢复 worker。
- 静态差异：AUT-21 baseline 说 `Ready` 必须在投影游标追平源游标时才允许，但 validator 只检查 `projection_cursor <= source_cursor`，没有要求相等。因此落后投影、零 Unknown、非 stale index 的 fact 在摘要匹配时可被标为 Ready。此处只记录合同与实现差异，没有修改条件。
- 本批只补充中文注释及来源清单/进度记录；未运行测试、构建、格式化或提交。

## 2026-10-10 Execution Permit 领域契约注释批次

- `kiana-domain/src/dispatch.rs`：补充 DispatchPermit 的系统位置、DTO 字段含义、SerDe/schema 边界、摘要用途、各校验错误和副作用；沿实际源码说明 Core `dispatch_authorized()` 写入 `execution.prepared`、Broker `JournalPermitVerifier` 读回核验/消费后才进入 handler、结果再由 Core 分阶段提交。
- 解释 `validate()` 只是对象内部检查，`validate_for_request()` 还绑定请求 action、项目身份和消费时间窗口；Broker 随后仍要核对 authority 版本并以 CAS 消费 permit。许可 digest 不是签名，单独验证通过不代表当前仍有执行权。
- 记录版本规则和 ID 语义：SchemaVersion 当前按 major 相同判兼容；invocation UUID 必须由 request UUID 派生；`derived_request_id()` 用 purpose 与 subject 的带分隔 SHA-256 确定性派生，不读取时钟也不执行命令。
- 另一 Codex 在该文件中新增的 invocation/request 绑定校验保留未改；本批为它补充解释。此批只增注释及进度/来源清单记录，未运行测试、构建、格式化或提交。

## 2026-10-10 AUT-22 自动化查询快照注释批次

- `kiana-domain/src/automation_snapshot.rs`：补充快照各 view DTO、字段语义、SerDe unknown-field 行为、单项 digest、全局 digest、状态/证据条件和跨对象 ID 绑定的中文说明；解释 validator 错误为首个失败字段的静态字符串，完全没有存储或状态副作用。
- `kiana-core/src/automation_snapshot.rs`：说明 Core 只转发 Domain `validate()`，当前没有生产 builder/调用者；不读 EventStore、不建 projection、不提供 UI/scheduler query service、不 claim 队列或改变 incident。
- 区分 AUT-22 `AutomationSnapshot` 与协议 `AUTOMATION_SNAPSHOT` 命令：后者由 Core `workflow_snapshot()` 读取 EventStore 并重放 durable workflow，返回另一种 state DTO，不调用 AUT-22 facade。
- 记录当前边界：游标/version/epoch 只验非零；workflow 状态 enum 未包含 durable workflow 全部失败/取消/暂停/等待状态且无映射器；workflow evidence 限制 64 项并逐项检查，trigger/receipt/incident 的 evidence 只在特定 Unknown 或非空列表条件下看列表长度，其他元素/顶层列表无相同限制；incident ID 和 trigger occurrence key 未去重。是否违反目标 UI/query contract需单独确认，未改校验行为。
- 本批仅增加中文注释和更新来源清单/进度；未运行测试、构建、格式化或提交。

## 2026-10-10 AUT-24 release evidence 注释批次

- `kiana-domain/src/automation_release_evidence.rs`：补充 EvidenceRow/Gate 字段语义、feature/proof enum、Serde 边界、逐行和整份 digest 计算、step 去重、错误顺序及无副作用范围；说明该 DTO 只验证调用者声明，不能读取或验证外部证据文件、审批或 CI。
- `kiana-core/src/automation_release_evidence.rs`：说明 Core 只转发 Domain validator；未找到生产调用者，不执行 release/publish，不修改 roadmap/CURRENT_STATUS proof level。
- 静态校验边界：`snapshot_digest` 只验格式、`workflow_ref/evidence_ref` 只验文本；Implemented 仅排斥精确 `"none"`，非空占位文本仍可通过；limitations 列表无数量上限；Live/Physical 门槛依赖 `Aut24ProofLevel` 的 Ord variant 顺序。均只记录，没有改校验。
- 工作区核对发现：`CURRENT_STATUS.md` 和 AUT-24 baseline 引用 `.github/workflows/aut24-release-evidence.yml`，Domain fixture 还引用 aut22 workflow，但当前 checkout 下这两个文件均不存在；fixture 辅助函数总会填入 `evidence:{step}`，没有实际覆盖 Implemented + `evidence_ref="none"`。按任务限制只记为文档/fixture 差异，不创建 workflow、不改测试。
- 本批仅补充中文注释及进度/来源清单；未运行测试、构建、格式化或提交。

## 2026-10-10 AUT-23 五入口 UAT 矩阵注释批次

- `kiana-domain/src/automation_surface_uat.rs`：说明 case 和汇总矩阵字段、Serde snake_case wire 名称、required/digest 格式、`new()` 从首行继承 header 或生成必然无效的空矩阵，以及完整校验顺序（五种入口恰各一行、command/snapshot/cursor/epoch 一致、禁止 direct route、拒绝时 Broker/handler 为零、两层 digest）。
- `kiana-core/src/automation_surface_uat.rs`：说明 Core 仅转发 Domain validator；没有启动五个界面、读取 daemon 或收集 runtime trace 的副作用。源码只看到导出和测试/guard 使用，未找到生产 UAT consumer。
- 明确所有计数、snapshot/command digest 和 `direct_route=false` 都来自调用方声明，校验不证明五入口真的连到同一 DaemonHost 或经过 ControlPlane。当前成功 case 不要求 `broker_calls` 或 `handler_calls` 非零；是否需要成功路径触达约束要由 AUT-23 的验收契约决定，未改变 validator。
- fixture 当前五条 case 均为 `denied=true`；它没有覆盖允许执行路径或真实跨入口 runtime parity。surface 集合比较不依赖输入顺序，但 cases 是 Vec 且按原顺序进入顶层 digest，重排会改变摘要。
- `CURRENT_STATUS.md` 与 AUT-23 baseline 声称有 `.github/workflows/aut23-surface-uat.yml`；本次对隐藏 workflow 目录和 Git index 做精确检查，当前 checkout 中无此文件。只登记状态/工作树差异，不新增工作流。
- 本批只增加中文注释及进度/来源清单；未运行测试、构建、格式化或提交。

## 2026-10-10 Core Dispatch 主链注释复核

- `kiana-core/src/dispatch.rs` 已包含三阶段派发、单 EventStore batch 的 CAS 边界、许可准备/一次性消费、结果 receipt 与 Runner 回灌的中文说明；本次再按 Broker 调用前消费链核对后，将 `committed_stop_metadata_matches()` 的英文说明补成中文，并扩展 `JournalPermitVerifier::new()` 的装配者、EventStore 依赖和无构造期副作用说明。
- 保留另一 Codex 对派发逻辑的工作区修改；本次在 Core dispatch 中只更改 doc comments。`kiana-domain/src/dispatch.rs` 的 permit DTO 说明与 Core 事件/消费说明互相对应。

## 2026-10-10 Daemon 执行控制与长进程监督注释批次

- `kiana-daemon/src/execution_control.rs`：按 DaemonHost 装配、Core 授权/Broker 路由、Runner shell 输出复用关系补充文件级数据流说明；为输入消息、活动进程表和共享控制器解释身份、状态、channel、内存预览及 EventStore 的分工。
- 为 `register()`、`fact()`、`authority()`、`start()`、`continuation()`、Handler 路由、输出捕获/存档/分页、本地包与身份/预算 helper 补充调用者、输入输出、副作用、错误与设计原因；解释 CAS 冲突、blocking task、暂存工作区、取消/deadline/authority 监控、PTY、分页 cursor、重启后只 poll 等边界。
- 强调 `process.start` 的 `running/started` 仅是启动受理，不是进程成功；started 事实写入失败会尽力停止并返回 Unknown；停止未确认时保留 path leases 和 semaphore permit；`StopReport.confirmed` 代表 supervisor 确认 leader 已回收且进程组最终为空，不等于逐个 reap 所有后代。
- 区分两条输出数据流：长驻进程 `capture()` 每个 reader 按 32 MiB 原始字节上限，活动态仅保留约 65536 字节脱敏尾部；Runner shell 的 `store_output()` 则在用户级 Kiana 输出目录存归档、将返回内容缩成预览，并通过 15 分钟 `ExecutionOutputRef` 分页读取。`environment.inspect` 的 `behavior_verified=false` 仅返回能力/预算声明，不是平台隔离实测。
- Handler 测试区说明直接构造内存控制器且只检查 `tool.search` 分支，不覆盖真实子进程、完整 DaemonHost/ControlPlane 授权链或 live sandbox。
- 静态差异和待核对项（均未改行为）：① `capture()` 是 stdout/stderr 各自最高 32 MiB 原始输入；`ProcessResourceBudget.max_output_bytes` 和 `environment.inspect.limits.output_bytes` 声明为 1 MiB，而 `ProcessSupervisor` 的 resource receipt 不执行这个输出字节预算，需确认三者是不同层级还是契约不一致。② `operation_catalog.rs` 的 stdin schema 上限是 65536 字节、rows/cols 上限 4096；当前 handler 分别拒绝大于 16384 字节和 1000 的 rows/cols，表现为更严格的执行期限制，需确认上游调用者是否预期看到该拒绝。③ `JobHandle.authority_epoch` 在本文件由 authority stream 的 `stream_version` 赋值；该读取器没有重放领域账本来推导 epoch，字段语义需结合 authority writer 契约确认。④ `ExecutionHandler` 只实现 `execute()` 并继承 Broker 默认 cancellable 行为；当前观察到的 `process.start` 是 operator command 路径，若以后接入可取消 Run，应确认 Broker 的取消确认是否能代表后台子进程已停止。⑤ 预览在 read loop 中裁到约 65536 字节，EOF 后再追加 redactor 尾部占位符但不再次裁剪，极端情况下预览可略超 `preview_limit_bytes`。
- 注释还说明了 `run_id`/`turn_id` 从操作参数解析，仅在这里绑定到句柄而非重新向 Core 验证；避免将传入关联值描述成已在本函数中验证的活动 Run/Turn。
- 本批仅增加/修订中文注释、来源清单和进度记录。另一 Codex 对 `execution_control.rs` 的执行逻辑改动全部保留；未运行测试、构建、格式化或提交。

## 2026-10-10 Authority 领域 reducer 与 Core 调用边界复核

- `kiana-domain/src/authority.rs`：将英文文件头替换为中文架构位置/数据流说明，区分“领域 reducer 能解析”与“当前生产 writer 实际写入”；逐字段说明 PolicyProfile、DataBoundary、SharingGrant、AuthorityLedger 的 ID、状态、epoch、版本、摘要与 allowlist 语义，并解释 Serde 严格字段、构造器规范化、验证错误、只读 iterator 和跨项目 grant 候选筛选。
- 为 `required`、`digest`、`list`、`decode` 和全部主要构造/校验/查询/reducer 方法补充输入、输出、副作用与设计理由。说明 SHA JSON digest 只校验内容一致性，不是签名；`allows()`/`sharing_operations()` 的 true/返回集合都不是最终 ControlPlane 授权决定。
- 为 `AuthorityLedger::apply_event()` 说明连续 stream version、未知 event 拒绝、branch-specific DTO 解码、项目分配对已存在 role assignment 的要求，以及只在整个事件分支成功后推进 `stream_version`。
- 静态风险记录：① `insert_unique()` 先 `BTreeMap::insert` 再检测旧值，duplicate 错误返回前会覆盖旧 entry；② revoke 分支先更新 epoch/grant 和 digest 再 `validate()`；因此直接增量调用 `apply_event()` 后若忽略 Err，ledger 可能部分改变但消费游标不动。Core `rebuild()` 遇错会丢弃局部 ledger。均只加警示注释，没有修复。
- epoch 合同待核对：`authority.revised` 把 event stream version 直接作为 epoch，其他领域事件则可把 epoch 推进到大于当前 stream version；后续较小版本的 `authority.revised` 会被 reducer 拒绝。当前生产 writer 只确认写 `authority.revised`，因此这是接入其他事件 writer 前应澄清的合同风险。
- `kiana-core/src/authority.rs`：修正“账本实际包含 membership/assignment/policy/share 状态”的过强描述，改为 reducer 支持这些事件而生产 writer 目前只确认写 `authority.revised`。在 fallback command key 与重放查询旁标注：无 event-level idempotency key 时 fallback 只进入 command ID，没有写回 event；重放读回条件仍比较 event key/kind/data，同一聚合出现内容相同的无 key 事件时可能匹配旧记录。
- Core 另有已核对边界：authority event 提交后若审批失效或项目 Run 停止失败，不回滚已写事件；同配置 digest 早退也不会续做清理。该行为只记为部分失败/重试限制，不改提交顺序。
- 本批只修改注释、来源清单和进度文档；没有修改 Domain/Core 执行逻辑，也未运行测试、构建、格式化或提交。

## 2026-10-10 子进程预算与停止确认注释批次

- `kiana-domain/src/process_supervisor.rs`：补充预算/停止事实的文件职责、Domain 与 Daemon 分工、两类数据流及“DTO 自洽不等于平台强制/OS 证明”边界；解释 ProcessResourceBudget 每个字段、默认数值和 `validate()` 只校验形状，不探测 backend 能力。
- 解释 StopMethod/ProcessGroupState 三态语义、StopReport 中 leader 与 process group 的区别、信号字段、确认条件、等待耗时与普通 digest；标注 `unknown()` 的无效哨兵回退语义。
- 静态契约差异：`StopReport::validate()` 允许 `confirmed=true` 且无 process_group_id，只要 leader_reaped 且 group=Empty；也没有要求 method=Term 时 term_sent=true，或 kill_sent=true 时 method=Kill。Daemon 生成路径有更严格确认/选 method 条件。只记录 DTO 比生成路径更宽，没有扩大校验。
- `kiana-daemon/src/process_supervisor.rs`：说明 shell 与 ExecutionControl 的调用关系、Unix `process_group(0)`、`pre_exec/setrlimit` 映射、CPU ms 向上取整为秒、全部软硬值相等、失败如何阻止 exec、wall deadline/output/RSS/cgroup 的实施归属。
- 解释 stop 的 TERM → leader 等待 → group 复查 → KILL → leader 等待 → 最后 group 观察顺序；100ms/500ms 等的是 Child leader，不是对所有后代分别等待；confirmed 要求 leader 已 reap、PGID 存在、最终 group Empty。ProcessGuard Drop 只 best-effort 发一次 SIGKILL，不等待也不产生确认。
- 静态限制：Linux `RLIMIT_NPROC` 以 real UID 范围计数，不是单个任务树硬上限，root/特权能力还可能改变强制效果；回执 `pids=per_process_rlimit_nproc` 文案可能高估隔离范围。`resource_receipt()` 只抄 budget 字段和静态 enforcement label，不采样 RSS/OS 状态；非 Unix 不设 process group/rlimit，若上游仍生成同一回执则 enforcement 字段会高于实际执行。非 Unix 有 PGID 时 stop 分支不会回退到 child.start_kill，group 观察始终 Unknown，因此不能返回确认停止。
- 本批仅加中文注释并更新来源清单/进度；没有改变资源限制、进程信号、错误处理或回执规则，未运行测试、构建、格式化或提交。

## 2026-10-10 子进程输出采集与预览注释批次

- `kiana-daemon/src/execution_output.rs`：解释该模块在 `harness_capabilities::run_confined_cancellable()` 中为 stdout/stderr 分别启动 reader，并把采集结果交给预览和后续归档；与长驻 `process.start` 的内存滚动预览区分。
- 为 `CappedOutput`、`read_capped()`、`drain_capped()`、脱敏/终端清理 helper、预览转换和 metadata 增加中文说明，覆盖每条 stream 独立预算、collect 满额后继续 drain 的原因、read block 导致的上限越界、reader 超时/Join 失败丢失未交付部分、无效 UTF-8 处理、ANSI CSI/OSC 清理和 marker 不计入预览正文预算等细节。
- 复核 `SecretScanChannel` 的真实实现后更正注释：stdout/stderr 等 channel 目前只作为 finding 来源标签，扫描规则相同；传递真实 channel 能保留诊断来源，不会启用另一套检测规则。
- 主要数据流：子进程 pipe → `read_capped()` → `CappedOutput` → `render_capped_for()` 生成短预览/metadata，同时调用方可将受限采集字节交给 `execution_control::store_output()` 归档并形成 `ExecutionOutputRef`。单流 reader 分别施加预算，不能把各字段理解成 stdout+stderr 的合计值。
- 本批只增加/修订中文注释及来源清单、进度文档；未改变采集、脱敏、截断、归档或错误行为，也未运行测试、构建、格式化或提交。

## 2026-10-10 执行输出领域契约注释批次

- `kiana-domain/src/execution_output.rs`：将英文文件说明替换为中文调用位置和输出数据流；说明该 Domain 文件只定义预算与引用，不读 pipe、不写归档、不授权、不清理过期内容，并指出 Daemon shell 归档和 `ToolOutputSpill` 对该引用的复用关系。
- 为 schema/最大字节常量、`ExecutionOutputBudget` 全字段、默认值、`validate()`、digest 形状 helper、`ExecutionOutputRef` 全字段及构造/验证/摘要方法补充输入输出、数据来源、失败与安全边界说明。特别区分 collect/preview/persist/observed/line 各阶段，并说明 stdout/stderr 的单流预算可分别生效。
- 明确引用中的 SHA-256 只用于结构/内容一致性指纹，不是签名、capability 或读取授权；Domain 验证不检查归档存在、内容摘要与文件是否匹配、当前 owner/data policy 或真实过期状态，这些由调用者读取路径再次核验。
- 本批仅增加中文注释和更新来源清单/进度记录；未改变领域字段、预算数值、校验规则或运行时行为，未运行测试、构建、格式化或提交。

## 2026-10-10 工具输出 spill 与分页 DTO 注释批次

- `kiana-domain/src/tool_output_spill.rs`：说明 Domain/Protocol 暴露的 `ToolOutputSpill` 和 `ToolOutputPage` 如何将完整输出引用、Run/Turn/Capability/source scope、预览、分页数据和 cursor 绑定；补充可读的 ASCII 数据流。
- 为 schema/字节上限、必填字符串与 digest 形状 helper、spill/page 全字段和主要构造/校验/分页方法补充中文注释，解释 preview 不是完整内容、`binary` 对 UTF-8 校验的影响、offset/next_offset 的 byte 语义、page cursor 如何绑定身份与位置，以及错误由上层读取适配器处理。
- 真实调用点检索目前只找到 Domain/Protocol 导出和契约测试，未找到生产 adapter 创建或调用 `page()`；因此注释与清单将其标为 DTO 契约而不是已接通的输出读取能力。读取权限、存储取回、过期和当前 owner 检查仍需由未来/上层 adapter 负责。
- 静态边界：`cursor` 参数目前是可选的，若提供才校验；page cursor 绑定 offset 但不绑定 limit。自摘要是未加密钥的 SHA-256 一致性指纹，不能作为授权或签名。
- 已确认校验缺口：`ToolOutputSpill::new()` 将传入的 `RunId` 包成 `Some`，但 `validate()` 只拒绝 `None`，没有拒绝 nil UUID；因此 nil Run 可通过 spill 构造/验证，`page()` 随后在创建 `ToolOutputPage` 时被其 nil run 检查拒绝。只补充警示，没有修复。
- 本批只解释现有行为，没有扩大或收紧校验。
- 本批仅新增中文注释并更新来源清单/进度记录，未改 DTO 字段、分页规则或执行逻辑，未运行测试、构建、格式化或提交。

## 2026-10-10 执行身份 DTO 注释批次

- `kiana-domain/src/execution_identity.rs`：补充 Core 与 Runner 之间的 Run/Turn/Step/Invocation/ModelAttempt 身份数据流；从源码核实 Core lifecycle 的 Start/NewTurn/LegacyContinue、Core recovery 的 Resume、Core dispatch 的 InvocationIdentity，以及 Runner 每个模型 step 和 provider 重试 attempt 的实际构造点。
- 为 `TurnSemantics`、四种 schema 常量、字符串/digest helper、四个身份 DTO 的字段及主要构造/validate/canonical-bytes/digest 方法补充中文解释，区分逻辑 turn number、step number、retry attempt、客户端/模型 call_id 与服务端生成的稳定 ID。
- 明确 `identity_digest`/canonical journal bytes 只用于确定性编码和自摘要一致性，不带密钥、不授予权限；本文件不会创建 Run、执行模型/能力或写 EventStore。跨事件 Run/Turn 关系仍由 Core dispatch/recovery 读取权威事实后核验。
- 校验边界：TurnIdentity 没有检查 RunId/TurnId 底层 UUID 非 nil；Invocation/Step/ModelAttempt 也不在自身 validate 中检查 UUID 非 nil 或各 ID 之间的事件级关系。当前主要生产构造点使用服务端上下文生成/传入这些 ID，是否需要领域 DTO 自身更严格拒绝需按身份契约确认；本批未修改校验。
- 本批只增加中文注释和更新来源清单/进度文档；未改身份字段、校验逻辑、构造顺序或事件行为，未运行测试、构建、格式化或提交。

## 2026-10-10 Domain 与 Entrypoints crate 根说明批次

- `kiana-domain/src/lib.rs`：补充底层 Domain 契约如何被入口/协议、Daemon/Core、EventStore/Receipt 消费的总览，并解释私有领域模块经 crate 根重导出后的外部类型路径；强调校验值存在不代表 Core 已授权或副作用已经完成。
- `kiana-entrypoints/src/lib.rs`：复核已有文件级说明，确认对外模块虽多，产品动作仍汇聚 `DaemonHost`；为仅在测试构建出现的 `test_support` 增补 EnvLock/ScopedEnv、Mutex poison recovery、OS 原始环境值、RAII drop 逆序恢复和三项并发测试的用途。
- 测试时序边界：并发测试用 channel 协调 writer ready、reader start 与 writer release；即时 `reader.is_finished()` 检查本身不能证明 reader 已实际进入锁等待，已在注释中避免把它描述成更强的同步证明。未运行该测试。
- 本批只增加 crate/test-helper 注释及更新来源清单/进度文档；未改变入口导出、环境变量操作或测试行为，未运行测试、构建、格式化或提交。

## 2026-10-10 kiana 二进制启动入口注释批次

- `kiana-entrypoints/src/bin/kiana.rs`：补充操作系统进程入口到 Tokio async runtime、初始化、CLI 路由与 cleanup handler 的顺序图解；说明 CLI 读取命令参数后仍走 `DaemonHost`，此 main 不另建业务执行循环。
- 解释 `init` 错误会由 `?` 直接传播，CLI 的 Ok/Err 返回后仍尝试一次清理；cleanup 失败只写 stderr、不覆盖 CLI 主结果。补充普通结果返回、panic/强制退出与常规清理之间的边界。
- 已对照 `init.rs`：当前 `init()` 可恢复设置问题通过状态/日志处理并最终返回 Ok；main 的 early-return 描述保留的是该函数签名及未来真实 Err 的语义，不声称当前存在一个已触发的初始化错误路径。
- 本批只增加中文注释和来源清单/进度记录；未改变 Tokio runtime、启动顺序、CLI 路由、清理顺序或返回值，未运行测试、构建、格式化或提交。

## 2026-10-10 Entrypoints 初始化、缓存与退出清理注释批次

- `kiana-entrypoints/src/init.rs`：逐段说明 `init()` 的启动顺序、扩展钩子当前为空的事实、Tokio 信号监听与 `graceful_shutdown()` 的连接、进程级异步清理 handler 的注册/注销/一次执行，以及首次启动状态、远程设置缓存和 scratchpad 如何通过文件与环境变量交给后续配置加载器。
- 为主要静态值、清理句柄、远程设置结果类型、路径/状态 helper、HTTP fetch、缓存读写、权限设置和测试 helper 补充中文解释；明确原子标记的进程内作用、mutex 不跨 await、缓存状态只检查文件路径、JSON 语法读取与 schema 校验的区别、HTTP 失败回退及错误是否会阻断 CLI。
- 对照当前源码更正缓存清理说明：清理正文时仅在路径相同时移除 `REMOTE_SETTINGS_FILE_ENV`，仍保留 `REMOTE_SETTINGS_CACHE_FILE_ENV`；没有声称当前代码重复移除同一个变量。
- 静态问题/易误改边界（本批未修复）：首次启动文件在 create-new 后写入或权限步骤失败可能留下不完整文件；无新 checksum 时旧 meta sidecar 不会清除；远程 endpoint 不强制 HTTPS/host allowlist，配置 endpoint 与凭据环境变量共同决定凭据发送目标；`0700` helper 也作用于 regular file；96 字符 cwd 分量截断不是唯一 ID；cleanup 单元测试直接 reset 共享注册表却没有与其他测试共用的环境锁。
- cleanup 运行时先置一次性闸门、再复制 handler 快照；快照后的并发注册不会进入本轮，下一次调用也不重试；从注册表注销不能撤回已经复制进快照的 Arc。返回 `Err` 会继续其余 handler，但不重跑；panic 会跳过后续 handler，永不返回的 handler 会阻止信号退出路径到达 `process::exit()`。注释已明确这些边界，没有新增锁、超时或重试。
- 远程设置的环境变量状态只是启动诊断投影，不是 EventLog 事实，也不证明下游配置解析成功；空钩子被调用不代表对应能力已经实现。来源清单已将该文件由待注释更新为已补充说明。
- 本批只增加/修订中文注释、来源清单与进度文档；未改初始化顺序、请求逻辑、文件操作、测试同步、API 或依赖，未运行测试、构建、格式化或提交。

## 2026-10-10 Domain 身份类型注释复核

- `kiana-domain/src/ids.rs` 原有中文说明已较详细，本批按真实调用点修订易误读处：UUID 外形不代表秘密或不可猜测；透明序列化（Serde `transparent`）保留底层 UUID/字符串线格式，但离开 Rust 类型边界后不会携带 newtype 名；`from_uuid()` 与 `parse_str()` 都不拒绝 nil UUID。
- 补充主链常见 `RequestId`、`RunId`、`TurnId`、`StepId`、`ModelAttemptId`、`InvocationId`、`ExecutionId`、`EventId` 的角色区别，并说明类型包装不会自动校验这些 ID 的关联关系。依据 Core dispatch、lifecycle 与 Runner attempt 的源码核对；保留全部原有宏调用和类型定义。
- 修正 `Default` 风险说明：其主要问题是漏赋值时静默生成一个合法但与调用方预期关系无关的新 UUID，而不是把随机 UUID 碰撞说成常见后果。说明 `SessionId::new()` 不裁剪、不拒绝空字符串。
- 为 `fnv1a64()` 解释 offset basis、FNV prime 与 64 位 wrapping 乘法。已确认 `WorkFingerprint::from_parts()` 以 U+001F 连接 input refs 且不转义：一个含分隔符的引用与两个相邻引用可生成相同规范字节，属于哈希前的编码歧义；只记录，没有修改编码或验证行为。
- 特殊边界：`kiana-domain/src/actions.rs::capability_action_catalog_digest()` 将 `include_str!("actions.rs")` 源码全文纳入摘要；仅加注释就会改变 catalog/action digest 与依赖该摘要的执行范围校验。因此在“运行时行为完全不变”的限制下，本批不改该源文件注释，并在后续审阅前将其列为需单独处理的兼容边界。另有测试通过 `include_str!` 检查多份源码，相关文件需先阅读 guard 的实际匹配方式再编辑。
- 本批只修订 `ids.rs` 注释和更新来源清单/进度文档；没有改 UUID 构造、Serde 形状、规范化字节、哈希算法或 ID 类型，未运行测试、构建、格式化或提交。

## 2026-10-10 Domain 能力错误分类注释批次

- `kiana-domain/src/errors.rs`：补充文件在 Domain、Core、Protocol、Daemon 与 CLI 之间的错误分类位置，说明 `CapabilityErrorCode` 只覆盖能力错误分类、不是 `DomainError` 总表；标注稳定 reason 名称是跨模块兼容契约。
- 说明 `CapabilityErrorPolicy` 六个字段的业务意义与执行边界；按源码核实当前表所有类别的 `retryable` 都为 `false`，`requires_*` 是供调用层消费的策略数据，不会在本文件自行授权、重试、补偿或对账。`http_status` 目前作为策略值定义，不能只由字段存在推断所有 HTTP adapter 都读取它。
- 为 `error_codes!` 说明单一表生成 enum、稳定名称、`ALL` 和 policy 的关系；为错误类别表补充分类概览，并解释 CLI 退出码与 HTTP 状态码不是错误 ID。
- 为 `from_reason()` 补充调用者、输入包装、精确前缀分类、已知别名、fallback、副作用/生命周期影响及不使用任意子串搜索的原因；为 `Display` 说明它只输出稳定错误码字符串。
- 静态边界：未知原因默认是 `ExecutionFailed`；`port_conflict:` / `port_unavailable:` 在其内部原因未知时保留相应 fallback。`TimedOut` 虽有 reconciliation 标志，但 Core 的 finalized 状态不会仅凭该标志自动改成 `ResultUnknown`。此外 `CapabilityErrorCode` 与 `DomainError` 分离，不能把前者描述成所有领域错误的完整映射。
- 本批只增加/修订中文注释并更新来源清单；没有改错误码枚举、映射、数值、策略、`Display` 输出或调用顺序，未运行测试、构建、格式化或提交。

## 2026-10-10 Core 并发准入调度器注释批次

- `kiana-core/src/capability_scheduler.rs`：补充它在 `dispatch_capability_action()` 与 Broker dispatcher 之间的位置图，说明它只控制进程内排队、并发槽与保守 footprint 隔离；Cell budget、授权、EventLog、跨进程路径锁和 OS 资源预算仍由其他边界负责。
- 为准入上限、Read/Write/Unknown footprint、scope 到路径集合的选择与通配回退、路径规范化、活动/待处理/隔离状态、ticket、lease 和 outcome 增加中文业务语义。为 `acquire()` / `release()` / `cancel_pending()` 说明调用者、输入输出、副作用、错误返回、取消时序、公平性和最终状态流转。
- 解释同步 Mutex 的短临界区、Mutex poison 恢复策略、Tokio Notify 的“唤醒后重检”语义；说明 `AdmissionLease` 没有 Drop 自动释放，调用者需由统一 finalizer 回收。
- 静态边界/风险：`next_ticket.saturating_add(1)` 到达 `u64::MAX` 后会重复票号，可能破坏 active/pending 的 ticket 唯一性；极端生命周期风险只记录、不改。`AdmissionOutcome::Unknown` 会把 footprint 放进当前 scheduler 的进程内 quarantine，但本模块没有删除入口，未知项会保留到实例销毁；隔离集合也没有独立上限。以上注释明确当前行为，不暗示已有 reconciliation 清除路径。
- 本批只增补/修订中文注释与来源清单、进度文档；保留源码 guard 所需的原始标记，未修改并发条件、常量、通知、锁、ticket 计算、错误、状态变更或调用顺序，未运行测试、构建、格式化或提交。

## 2026-10-10 Broker 文件级调用链注释批次

- `kiana-capability-broker/src/lib.rs`：补充 DaemonHost 装配、ControlPlane 授权、Broker 精确 handler 路由、Connector effect/quota 与一次性 permit 检查、handler 执行、结果回到 Core finalizer 的主链图。
- 说明 Broker 不写 EventStore、不签发授权；`authorization_id` 不是密码学签名；`discovery` 子模块虽被重导出，也不能因此推断它已接入模型工具面或能单独执行。
- 来源清单标记为部分完成：该文件已有较多中英混合函数说明，尚需逐函数核对哪些已准确、哪些需要完善/翻译，不能仅凭这次文件头更新标为全文件完成。
- 本批只增补文件级中文文档和清单；没有改类型、注册、校验、分发或 handler 调用逻辑，未运行测试、构建、格式化或提交。

## 2026-10-10 Broker 动态候选判定注释批次

- `kiana-capability-broker/src/discovery.rs`：补充授权快照、扩展准入证据、搜索请求/结果、候选、报告之间的数据流；解释候选声明与服务端登记值的区别、能力集合只能取交集、`Admitted`/`Denied`/`Unknown` 的语义，以及 `promotable()` 为什么只在 Admitted 状态返回候选。
- 为 schema/数量上限、信任来源、目录字段、项目与授权代次、manifest/package/schema 摘要、过期时间、`validate()`、`evaluate()` 和 `admit_candidate()` 增加中文说明；明确 digest 是未加密钥的内容一致性摘要，不是签名、授权票据或真实来源证明。
- 依据当前全仓 `rg` 调用点核对：生产 `kiana-core`、`kiana-daemon`、Runner 没有调用该模块，当前调用来自 `kiana-capability-broker` 的 CAP-30 测试；因此注释没有把它描述为已接通的动态搜索或可执行工具注册。
- 易误改边界：`DiscoverySearchOutcome::Failed` 必须保持 `Unknown`，不能折叠为空列表；候选超范围时整条拒绝，不能自动裁剪后放行；重签报告摘要不能替代 `validate_against()` 的原始输入重算；真正执行仍须重新走 ControlPlane。
- 本批只增加/修订中文注释和清单/进度文档；未改拒绝优先级、字段、摘要、上限、候选准入逻辑或任何执行路径，未运行测试、构建、格式化或提交。

## 下一批

## 2026-10-10 Domain 取消事实注释批次

- `kiana-domain/src/cancellation.rs`：补充取消事实从 CLI/UI 意图、Core 的 CAS 版本与事件事实，到 Runner/Daemon 停止尝试、`Cancelled`/`ResultUnknown` 终态、EventStore、恢复投影和 Receipt 的完整数据流。区分进程内取消通知与可恢复事实：通知只负责尽快唤醒任务，事实才是生命周期恢复和审计可以重新解析的契约。
- 为 `RunCancellationFact` 的 Run/Command/Invocation 身份、actor、目标集合、预期 stream version、stop requested/confirmed、事件时间和两个 digest 字段补充业务含义；解释目标排序去重是幂等摘要的规范化步骤，`expected_version` 只由上层 CAS 使用，本文件不检查当前流版本。
- 为 `new`、`from_json`、`to_json`、`validate`、`digest` 和摘要形状 helper 补充调用者、输入输出、副作用、失败传播和状态不变量：`Cancelled` 必须有 `Some(true)` 停止确认，`ResultUnknown` 不能声称已确认，`Active` 不能伪装成取消事实，目标 ID 和摘要必须满足契约。
- 明确 digest 是未加密钥的一致性指纹，不是授权、签名或停止证明；校验失败不会自行发送信号、写失败事件、重试或对账，具体恢复动作由 Core/Daemon/Runner 的上层调用者决定。仅记录现有边界，未收紧校验或改变状态流转。
- 本批只增加中文注释并更新来源清单/进度文档；未改事实字段、排序、摘要输入、serde 形状、错误字符串或执行顺序，未运行测试、构建、格式化或提交。

## 2026-10-10 Query crate 根模块注释批次

- `kiana-query/src/lib.rs`：按真实源码调用关系补充查询适配层在产品主路径中的位置。受控上下文路径是 `entrypoint/core normalize -> ControlPlane.authorize_and_execute -> daemon context_query handler -> kiana-query index/repo_map -> CapabilityResult/receipt`；另有 Direct Connect 的 HTTP bearer 路由直接调用部分 materialization helper，以及遗留 `kiana-tools` 的 pre/post hook 路径，不能把这些入口混为同一条授权链。
- 将 29 个公开模块按职责分为三组：投影/查询模块只把调用方提供的已提交事实折叠成有界读模型；workspace context 模块负责索引、repo map、chunk、manifest、generation、cache 和检索适配；hook/reducer 模块分别处理外部 hook 进程与纯状态意图。特别标出 `write_*`、`build_persistent_*`、`ingest_context_artifacts` 会写本地文件，`stop_hooks` 会启动外部进程，不能笼统描述为“纯查询”。
- 解释 `pub mod`（声明并暴露模块路径）与 `pub use`（提供 crate 根快捷路径）的区别，并说明根导出只是 API 可见性，不代表所有导出已经有生产调用者；当前生产使用集中在 daemon 的 context/health 适配器、entrypoints 的 CLI Direct Connect、legacy tools 的 hooks，其余许多投影/检索导出主要由 query tests、source guards 或 daemon 内部使用。
- 记录与实现的文档边界：`kiana-entrypoints/src/cli.rs` 的部分 Direct Connect 写路由在 bearer 校验后直接调用 `build_persistent_*`/`ingest_context_artifacts`，没有经过 `ControlPlane.authorize_and_execute`；这是当前源码事实和规范目标之间的冲突，本批只在注释中标明，不改变入口或授权逻辑。
- 本批只增加模块级中文注释和来源清单/进度文档；未改模块声明、重导出、依赖方向或任何查询/Hook 实现，未运行测试、构建、格式化或提交。

## 2026-10-10 Daemon 受控上下文查询注释批次

- `kiana-daemon/src/context_query.rs`：补充入口到 handler 的真实链路：`ControlPlane::authorize_and_execute` 将已规范化的 Query action 交给 Broker，Broker 按 `(CapabilityKind::Query, operation)` 找到这里的 handler，handler 再调用 `kiana-query` 并把 `CapabilityResult` 交回 Core finalizer/receipt。明确本文件负责本地工作区访问，不负责签发授权、决定审批或写最终生命周期事实。
- 区分只读 operation（repo map、artifact graph/readiness、search、vector search、pack）与可能落盘的 materialization operation（index/artifact/artifact-store cache、ingest）；解释为什么同步扫描/解析/写入放进 Tokio `spawn_blocking`，以及为什么不能把 cache/ingest 藏在普通 read operation 的可选字段里。
- 为 `register`、两类 handler 的 `execute`、operation key/label 映射、参数解析、read/materialization dispatch、输出 envelope、provenance 递归收集和 text formatter 增加调用者、输入、输出、副作用、失败传播和“结果未知不等于空结果”的说明。对当前最新代码中的 server-stamped metadata、Core 省略 null 可选字段和未知业务键 fail-closed 规则补充解释。
- 详细说明 `canonical_project_root`、`confined_context_root`、existing/write path helper 的两层边界：先检查字符串是否为相对路径，再对 canonical 目标或最近存在的 ancestor 做 containment；治理策略仍会在扫描/写入前拒绝 retained revoked source。说明 ingest 额外拒绝 source 位于 store 内，避免把刚写出的内容重新当作输入。
- 说明 `render_output` 中的 `source_snapshot` 是按 `path + content_hash` 收集的未加密钥读取来源指纹；`captured_at_read` 和 `atomic_workspace_snapshot=false` 只描述当前读取时刻，不能当作锁、授权签名或完整 workspace snapshot。
- 该文件在本批开始时已有另一 Codex 的参数 allowlist 与测试改动；本批只在其最新源码上增加/改进中文注释，并更新来源清单/进度文档，没有改 operation 字符串、参数键、路径判断、异步调度、错误字符串或测试逻辑，未运行测试、构建、格式化或提交。

## 2026-10-10 Core 上下文意图规范化注释批次

- `kiana-core/src/context_query.rs`：补充它与 Daemon 查询 adapter 的边界。入口的 `context.query.v1` 先由 `handle_context_query` 接收；本文件只把人类可读 operation/options 转换为稳定 Broker operation、风险级别和闭合参数，不直接读取文件、不调用 `kiana-query`，最终交给统一 `authorize_and_execute`。
- 为 `handle_context_query` 说明调用者、输入的 `RequestContext`/JSON intent、成功路径和失败路径：规范化失败仍先写 `request.accepted` 与 `command.rejected`，再返回 blocked；事件写入失败按 `CoreError` 传播，不能伪造“拒绝已记录”。成功只表示进入后续授权评估，不表示文件已读、cache 已写或 receipt 已生成。
- 为 `normalize_context_query_arguments` 的各 operation 分支补充业务语义：repo map、graph/readiness、内存 index/artifact store、明确的 cache write、artifact ingest、search/vector/pack 如何映射到 operation 与 `RiskLevel`；解释 16 MiB 单文件上限、1000 条结果/1000 行片段上限、非空 query、cache/source 必填和 pack 的额外字段。
- 为 option allowlist、正数有界整数、可选/必选相对路径、`NormalizedContextQuery` 和错误枚举补充 fail-closed 原因、调用者看到的错误类别及后续 Daemon canonical containment 的职责；明确词法 traversal 检查不能替代 symlink/真实路径 containment。
- 本批只增加中文注释和清单/进度记录；未改 operation 名称、风险级别、事件顺序、参数键、数值上限、路径判断、错误映射或 `authorize_and_execute` 调用，未运行测试、构建、格式化或提交。

## 2026-10-10 Core 唯一命令路由复核批次

- `kiana-core/src/commands.rs`：在已有文件级命令矩阵说明基础上，补充 `handle_command`/`handle_command_inner` 两层的关系：外层只 boxing 巨大的 async 状态机，内层按命令名称顺序路由；特权前置校验、纯判定、专用 handler、统一授权通道和最终 fallback 各自承担不同职责。
- 为 `context.query.v1` 分支补充实际链路说明：这里只决定命令归属，必须交给 `handle_context_query` 做参数规范化和 Query capability 授权，不能在入口或 Daemon 另起路径读取文件、重复做权限判断或启动模型循环。
- 为最后的 unknown/project-untrusted fallback 解释“先记 request.accepted，再记 command.rejected”的事实意义，以及 `Ok(blocked)` 与 `Err` 的业务区别；未知命令不能静默当成成功。
- 本批只增加中文注释并更新清单/进度文档；未改命令分支顺序、boxed future、授权调用、事件顺序、错误码或任何业务处理，未运行测试、构建、格式化或提交。

## 2026-10-10 EventLog crate 根说明批次

- `kiana-eventlog/src/lib.rs`：补充从 `kiana-ports::EventStorePort`、Core 事件校验，到 Daemon 注入 `JsonlEventLog`/`MemoryEventLog`，再到 recovery/audit/receipt/query/UI 投影读取的模块地图。说明 `event_store_core`、`journal_core`、`stream`、`integrity`、`writer_queue` 和其他本地 store 的职责边界，避免把所有公开导出误认为同一个事实写入器。
- 进一步区分 JSONL 落盘与进程内 Memory adapter 的生命周期：共享端口语义不等于共享持久性；adapter 接受写入也不自动证明外部副作用、durable、live 或 physical 结果。
- 为 `default_sessions_log_path()` 补充调用者、输入来源、固定 `<home>/sessions/events.jsonl` 后缀、`KIANA_HOME`/`HOME` 失败码及“不回退当前工作目录”的原因；明确它只计算路径，不创建文件、不验证 JSONL、不注册全局事实源。
- 本批只增加中文 crate/函数注释和清单/进度文档；未改路径拼接、环境变量读取、错误码、模块导出或存储实现，未运行测试、构建、格式化或提交。

## 2026-10-10 Bootstrap crate 根说明批次

- `kiana-bootstrap/src/lib.rs`：补充它在 `kiana-entrypoints` 启动、Runner/model adapter 和旧 CLI 状态之间的兼容层位置图；说明 `config` 负责读取文件/环境 overlay，`state` 负责进程内 UI/统计辅助状态，真正的授权、Run/Request 身份和 EventLog 事实仍在 DaemonHost/ControlPlane/Domain。
- 明确 `api_key`、`base_url`、session id 和 `GlobalState` 的证明边界：读取到配置不等于请求已获准，兼容 session id 不等于 Domain Run/Request ID，`Arc<Mutex<State>>` 不提供跨重启 durable 恢复。crate 根重导出只改变调用路径，不代表每个入口都会使用所有类型。
- 本批只增加 crate 根中文注释和来源清单/进度记录；未改模块声明、重导出或任何配置/状态实现，未运行测试、构建、格式化或提交。

## 2026-10-10 Bootstrap 配置合并注释批次

- `kiana-bootstrap/src/config.rs`：补充 `KIANA_CONFIG_FILE`/默认 home 配置、remote/user/JSON overlay、Anthropic 环境覆盖和最后 managed overlay 的真实合并顺序；说明 `load_config()` 每次重新读取、解析失败的当前回退行为，以及为什么 managed 放在最后。
- 为 `Config`、`Settings`、`AutoModeSettings` 字段补充业务含义，区分 UI/Runner 偏好、API key/base URL 字符串、timeout 毫秒值、sandbox JSON 和策略授权；强调 `Option` 为空表示未配置，不表示匿名访问或请求已准入。
- 为 `load_config`、基础文件读取、格式判断、overlay 合并、非空字符串/规则列表清洗、`load_config_file`、`save_config_file` 和 `config_path` 补充调用者、输入输出、副作用、失败/回退和优先级边界；特别记录列表是追加且不去重，保存操作会创建目录并覆盖文件但不原子替换、不写 EventLog。
- 本批只增加中文注释和来源清单/进度文档；未改配置字段、serde alias、来源顺序、空值过滤、文件读写或错误返回，未运行测试、构建、格式化或提交。

## 2026-10-10 Bootstrap 进程状态注释批次

- `kiana-bootstrap/src/state.rs`：补充旧入口/兼容 Runner 使用的 `GlobalState -> Arc<Mutex<State>>` 位置图，说明它保存的是进程内 UI、统计、session 缓存和请求边沿状态，不是 EventLog、Domain Run/Request 身份或 ControlPlane 授权事实。
- 为 `ChannelEntry`、`ModelUsage`、`SessionCronTask`、`InvokedSkillInfo`、`SlowOperation` 和 `State` 字段分组补充业务语义，解释路径、ID、计数、耗时、`Option`、header latch、prompt cache、trust/session 开关各自表示什么，以及哪些值只是本地观察。
- 为 `State::default`、`GlobalState::new`、session ID 生成/切换、工作目录读写、API/tool/Hook/行数/token 统计、skill 缓存清理、compaction 一次性标记、成本窗口重置和 beta header latch 清理补充调用者、输入输出、锁边界、副作用、失败语义和不能替代的上层事实。
- 明确 `regenerate_session_id` 在同一锁内清理旧计划缓存并重置项目覆盖，`clear_invoked_skills` 的 `None`/空集合/非空集合三态语义，以及 `consume_post_compaction` 的一次性消费性质；提醒不要在持锁期间执行网络、文件或模型调用。
- 本批只增加中文注释和清单/进度文档；未改字段、默认值、session/cache 更新顺序、计数规则、锁使用、错误行为或测试逻辑，未运行测试、构建、格式化或提交。

## 2026-10-10 Core CP-30 产品流程门面注释批次

- `kiana-core/src/control_plane_product_flow.rs`：补充 CP-30 证据校验门面在 `DaemonHost -> ControlPlane -> KianaHarness/CapabilityBroker -> EventStore/Receipt -> Cp30ProductFlowBundle` 链路中的位置，并说明四个产品界面共享同一 flow evidence 的背景。
- 说明 `Cp30ProductFlowBundle`、三种流程和四界面 receipt 的结构校验实际由 `kiana-domain` 维护；Core 只通过公共门面委托校验，避免复制一份会漂移的审批、receipt、surface parity 规则。
- 为 `validate_control_plane_product_bundle` 补充调用者、输入来源、成功条件、稳定错误码、无副作用失败处理和 proof-level 边界；明确它不会创建 DaemonHost、启动 Runner、发送模型请求、调用 Broker effect、自动批准或写 EventLog。
- 记录 CP-30 CI fake model/effect fixture 只能证明 source/fixture 契约，`Ok(())` 不等于流程已 live、durable 或 physical 执行；不能把这里改成第二条执行循环或为了让 fixture 通过而放宽 Domain 校验。
- 本批只增加中文 crate/function 注释和清单/进度文档；未改 Domain 校验委托、公共签名、错误传播或任何执行路径，未运行测试、构建、格式化或提交。

## 2026-10-10 Domain CP-30 证据合同注释批次

- `kiana-domain/src/control_plane_product_flow.rs`：补充 CP-30 证据事实模型的完整数据流：真实 DaemonHost/ControlPlane/Harness/Broker/EventStore/Receipt 先产生观察结果，四个界面再形成 receipt，最后由本文件做 Domain 层合同校验。
- 为 `Cp30FlowKind`、`Cp30FlowStatus`、`Cp30Surface` 解释只读、审批写入、取消恢复以及 `AwaitingApproval`、`ResultUnknown`、`Reconciled` 的业务语义；明确四个 surface 是同一事实的展示面，不是四条独立执行路径。
- 为 `Cp30SurfaceReceipt`、`Cp30ProductFlowEvidence`、`Cp30ProductFlowBundle` 的字段、构造函数、校验函数和 canonical digest 补充调用者、输入输出、字段关系、失败码传播、无副作用边界和摘要重算原因。
- 解释验证顺序：schema/数量/文本形状、单面 receipt 自洽、四面 identity/status/cursor/receipt parity、流程特有 effect/approval/recovery 不变量、最后的 digest 一致性；强调未知结果不能被当作成功、审批等待不能被当作完成。
- 明确 `sha256:` digest 只是未加密钥内容一致性指纹，不是签名、授权、重放防护或真实副作用证明；CP-30 fixture 的 proof 上限是 source/fixture，不能宣称 durable、live 或 physical。
- 本批只增加中文 Domain 注释和清单/进度文档；未改 schema 常量、serde 形状、字段、错误字符串、集合检查、digest 输入、校验顺序或任何执行路径，未运行测试、构建、格式化或提交。

## 2026-10-10 Core CP-29 authority 门面注释批次

- `kiana-core/src/control_plane_authority.rs`：补充 CP-29 从 `ControlPlane/TransitionBatch/Journal` 产生已提交事实，到 `Cp29AuthorityScenario` 再到 Domain `validate()` 的真实证据数据流；说明当前仓库生产接入主要是 crate 根导出和 source guard，不能据此宣称 runtime authority matrix 已运行。
- 为 `validate_control_plane_authority_scenario` 补充输入、输出、稳定错误码、权限/预算/生命周期/terminal/replay/Unknown fencing/crash matrix/online-replay digest 检查语义，以及无 EventStore/Broker/Runner/provider/fault 注入副作用的边界。
- 解释 Core 保持薄委托是为了让 Domain 作为唯一 authority 合同事实源，禁止在这里复制规则、自动恢复、自动批准或建立第二执行循环；`Ok(())` 仍只代表 source/fixture 证据满足结构，不代表 live、durable 或 physical 证明。
- 本批只增加中文 crate/function 注释和清单/进度文档；未改公共签名、Domain 委托、错误传播或任何执行路径，未运行测试、构建、格式化或提交。

## 2026-10-10 Domain CP-29 authority 状态机注释批次

- `kiana-domain/src/control_plane_authority.rs`：补充 CP-29 从已提交 `ControlPlane -> TransitionBatch/Journal/EventLog` 事实到 Domain 合同验证的完整数据流，明确本文件不启动进程、不杀子进程、不调用 Broker/Runner/provider，也不把模拟崩溃提升为 runtime 证据。
- 为 `Cp29LifecycleState`、`Cp29CrashPoint` 和 `transition_allowed` 解释 queued/running/paused/cancel/terminal/unknown 的业务含义，说明 `ResultUnknown` 有意不算 terminal、七个 crash point 必须恰好覆盖，以及状态迁移白名单不能随意放宽。
- 为 `Cp29CommandFact` 的 sequence、command ID/digest、authority epoch、permission rank、budget、effect、replay、unknown 和 canonical digest 补充字段语义；为构造和 validate 说明权限只能收紧、预算不能超支/回滚、terminal 不复活、replay 不产生第二 effect、同状态不能伪造 effect。
- 为 `Cp29CrashObservation` 说明 effect started/confirmed、最终状态、resource fence、duplicate effect 和 source ref 的关系；重点解释“已开始但未确认 -> ResultUnknown + fence”、确认后不能 Unknown、未开始不能 Completed，以及失败不会自动 fence/补偿。
- 为 `Cp29AuthorityScenario` 说明 online/replay fold、连续序列、command ID 幂等/payload drift、authority/budget 单调、crash matrix 完整性和 scenario digest 的跨事实校验顺序；明确所有 digest 是未加密钥的一致性指纹，不是签名/授权/现实效果证明。
- 本批只增加中文 Domain 注释和清单/进度文档；未改枚举、字段、serde 形状、迁移白名单、数量上限、错误字符串、集合检查、digest 输入或执行路径，未运行测试、构建、格式化或提交。

## 2026-10-10 Core BQ-18 fallback 重新准入注释批次

- `kiana-core/src/fallback_admission.rs`：补充 primary attempt → ControlPlane → Domain `admit_fallback_attempt` → 新 attempt/reservation/opaque permit → 既有 Broker/ProviderGateway 的真实边界；明确它不是 provider 内部重试，也不创建第二执行循环。
- 解释 `FallbackAdmissionRequest`、`FallbackAdmissionCandidate`、`FallbackRouteAllowlist` 和 `attempt_id` 的输入来源与业务语义，说明 fallback 必须重新检查 authority、数据边界、预算、凭据、精确 route allowlist 和价格事实。
- 为 `ControlPlaneFallbackAdmission::re_admit`、`ControlPlane::re_admit_model_fallback` 补充调用者、输出、Domain 稳定错误、无 EventLog/provider/网络副作用和 Broker/Provider effect-time 继续校验的边界；强调不能继承 primary 的 unknown、receipt、reservation 或 permit。
- 本批只增加中文注释和清单/进度文档；未改 Domain 函数调用、公共签名、错误传播、候选选择、permit/reservation 规则或任何执行路径，未运行测试、构建、格式化或提交。

## 2026-10-10 Core BQ-19 成本归属准入注释批次

- `kiana-core/src/billing_allocation.rs`：补充 BQ-13 usage/server-owned scope → `CostAllocationAdmission` → `RuntimeEvent(cost.allocation)` → 既有 ControlPlane/EventStore → billing/query projection 的数据流，明确本文件不是账本或财务 authority。
- 为准入错误分类、`authorize`、`authorize_wire`、跨项目 `SharingGrant` 检查、`allocation_event` 和 `ControlPlane::admit_cost_allocation` 说明调用者、输入、输出、失败类别、重试/写入副作用和下游处理。
- 重点解释 wire scope 只是声明、trusted scope/source project 必须由服务端提供；同项目不应携带 grant，跨项目必须精确匹配 active grant、操作权限、当前时间和 authority epoch；RuntimeEvent metadata 固定 aggregate type/id/revision。
- 明确重复 Domain `validate` 是事件边界的防篡改检查，成功只表示可以进入提交阶段；本文件不会 append EventStore、结算/扣款、调用 provider、自动选 grant 或改变财务状态。
- 本批只增加中文注释和清单/进度文档；未改错误枚举、准入顺序、grant 条件、事件 schema、stream metadata 或任何执行路径，未运行测试、构建、格式化或提交。

## 2026-10-10 CI-12 产品链证据注释批次

- `kiana-core/src/ci12_product_gate.rs`：补充 Core facade 在统一 `DaemonHost -> ControlPlane -> handler/provider` 证据链中的位置，说明它只委托 Domain `Ci12ProductGate::validate`，不创建 provider/approval/capability 第二路径。
- `kiana-domain/src/ci12_product_gate.rs`：补充十类场景从入口事实到 case/gate digest 校验的数据流；解释缺认证、项目 trust、跨项目范围、过期审批、secret、TOCTOU、replay 必须 deny-first 且 handler/provider/effect 为零。
- 为 `ResultUnknown`、`FakeProviderSuccess`、`LiveOptIn` 及 `Denied/Completed/Unknown/Unverified` 补充业务语义，明确 Unknown 必须 receipt+limitation，fake success 不能携带 live evidence，live opt-in 仍需 typed approval/provider evidence 且 source fixture 不等于 live/physical。
- 为 `Ci12Case`/`Ci12ProductGate` 构造、validate、canonical digest 补充调用者、字段含义、失败码/副作用边界、十场景唯一覆盖和为什么不能只信 digest；明确校验不会执行、批准、回滚或修改真实运行状态。
- 本批只增加中文注释和清单/进度文档；未改场景枚举、计数规则、状态合同、schema、错误字符串、digest 输入或任何执行路径，未运行测试、构建、格式化或提交。

## 2026-10-10 Core H26/CO-35 事实边界注释批次

- `kiana-core/src/clarification.rs`：补充 Runner pending question → TTY/Web Human Inbox → `ClarificationAnswer` → Core commit → EventLog → 同一 Runner continuation 的数据流；说明普通回答不消费 approval、不创建 capability、没有 grant/approval ID/tool args。
- 为 `ClarificationCommit`、`commit_clarification_answer` 和 `clarification_human_inbox_item` 说明调用者、绑定的 interaction/run/turn/step、request digest、过期/foreign/duplicate 失败、待提交事实和 UI 投影副作用边界。
- `kiana-core/src/closing_receipt.rs`：补充 packet/attempt/run/review/acceptance/delivery/incident/evidence → typed Company ClosingReceipt → Domain ledger/read model 的链路；说明 Success/Failure/Cancelled/Waived 条件由 Domain 维护，Core 不从聊天或一次 runtime result 推断关闭。
- 明确两个 Core 文件只做校验/投影适配：不 resume Runner、不写 EventLog/Company transition、不创建第二 ledger，不提升 durable/live/physical 证明；本批未改函数签名、ledger 行为或事实提交顺序。
- 本批只增加中文注释和清单/进度文档，未运行测试、构建、格式化或提交。

## 2026-10-10 Domain H26 澄清合同注释批次

- `kiana-domain/src/clarification.rs`：补充 Runner 提问、Human Inbox 展示、回答绑定、Core/EventLog 提交和原步骤恢复的完整数据流；明确 Domain 只定义 versioned value objects 与 fail-closed 状态转换。
- 为 `ClarificationStatus`、`ClarificationCancelPolicy`、`ClarificationSource`、`ClarificationOption` 解释 Pending/Answered/Cancelled/Expired、runtime/user 取消范围、入口标签、option ID/label 与协议上限。
- 为 `ClarificationAnswer` 和 `ClarificationRequest` 的字段、构造、validate、digest、accept_answer、cancel、expire、waiting_view、binding helper 补充调用者、输入/输出、时间边界、角色/option 检查、foreign/duplicate fence、失败传播和无 EventLog/Runner/approval 副作用。
- 为 `ClarificationResolution` 与 `ClarificationWaitView` 说明 resume_original_step/waiting_for_input 的固定方向、原始身份/摘要绑定、UI 投影边界和为什么不能把普通回答当作 approval/capability。
- 本批只增加中文 Domain 注释和清单/进度文档；未改 schema、serde 形状、状态转换、时间比较、上限、错误码、摘要输入或执行路径，未运行测试、构建、格式化或提交。

## 2026-10-10 Domain CO-35 ClosingReceipt 合同注释批次

- `kiana-domain/src/closing_receipt.rs`：补充 Company/EventLog 已提交事实 → packet/attempt/run/review/acceptance/delivery/incident/evidence 引用 → `CompanyClosingReceiptContract` → `CompanyClosingReceiptLedger` → read model 的数据流，明确 receipt 不是从聊天或一次 runtime result 推断的关闭事实。
- 为 `CompanyCloseKind`、合同字段和 `validate` 解释 Success/Failure/Cancelled/Waived 各自的 project/acceptance/delivery/stop/waiver/residual obligation 条件；说明引用列表的非空、数量、去重和角色独立检查，以及 unresolved incident 的统一拒绝。
- 为 canonical digest、`CompanyClosingReceiptLedger::record` 和 `for_project` 补充调用者、输出、幂等重放、duplicate digest/project 拒绝、借用查询和内存投影边界；强调 ledger 不替代 CompanyState/EventLog durable authority。
- 本批只增加中文 Domain 注释和清单/进度文档；未改 close kind 分支、错误码、字段、引用上限、digest 输入、幂等条件或 Company 执行路径，未运行测试、构建、格式化或提交。

## 2026-10-10 Core/Domain CO-30 基线变更合同注释批次

- `kiana-core/src/change_contract.rs` 与 `kiana-domain/src/change_contract.rs`：补充旧 baseline/ChangeRequest → `ChangeImpact` → `BaselinePublication` → Domain ledger → 上层 CAS/EventLog 的完整数据流，解释为什么变更不能只修改项目 scope 字符串。
- 为 `ChangeImpact` 说明 milestone/packet/run/review/acceptance/delivery 影响闭包、旧/新 baseline 版本、budget/schedule 引用和去重/digest；为 `BaselinePublication` 说明新 charter/plan/packet/criteria/budget/schedule 必须完整且与 impact 精确绑定，并显式列出失效引用。
- 为 `ChangePublicationLedger::publish` 和 Core `publish_change` 说明相同 digest 的幂等重放、同 ID drift、同项目版本冲突、写入顺序和“不部分写入”语义；明确 ledger 是内存 source contract，不替代 durable CAS、EventLog、旧授权失效或 Run 停止。
- 本批只增加中文注释和清单/进度文档；未改 schema、字段、版本条件、引用集合、错误码、digest 输入、幂等/冲突规则或 Company 执行路径，未运行测试、构建、格式化或提交。

## 2026-10-10 Core SC-40 容量封套注释批次

- `kiana-core/src/capacity_fault_envelope.rs`：补充 BQ-26 fault sample → sealed `CapacityBudget` → `evaluate_capacity_fault` → `CapacityFaultReport` 的数据流，明确本模块只消费观察值，不注入故障、不测量系统、不启动恢复。
- 为预算、样本和报告字段解释延迟/队列/输出字节上限、反压、故障期间 reservation/lease 泄漏、恢复是否执行、恢复后 reservation 和时钟回拨语义；说明零上限表示未声明封套，不能当作无限。
- 为 `validate_against` 和 `evaluate_capacity_fault` 解释检查顺序：先预算和时钟可信度，再容量/反压，再故障族/泄漏，最后恢复；说明 provider 429/5xx 超额、DiskFull 反压和 ClockFault 未实际回拨为何有专用拒绝原因。
- 明确 report 的 budget/sample 绑定与 limitations 证明边界：通过只代表 supplied source sample 满足封套合同，不代表真实测量、durable、live 或 physical 结果。
- 本批只增加/翻译中文注释和清单/进度文档；未改错误顺序、阈值、字段、digest 输入、恢复条件或执行路径，未运行测试、构建、格式化或提交。

## 2026-10-10 Core 多入口 parity 注释批次

- `kiana-core/src/entrypoint_parity.rs`：补充 CLI/TTY/Web/Workbench/Desktop/scheduler/swarm/connector 入口只负责标记来源，所有请求必须沿固定 `daemonhost.controlplane` route 的数据流。
- 为 `EntrypointDecision`、`EntrypointCommand`、`EntrypointParityMatrix` 字段和构造/JSON/validate/digest 方法说明 request/context/arguments/command digest、decision 状态、矩阵共享 request/command、入口去重和最多 8 条命令的原因。
- 明确 `Denied` 必须对应 `handler_calls == 0`，`AwaitingApproval`/`Unknown` 不能渲染成成功，排序只保证稳定输出而不表示执行顺序；digest 是一致性指纹而非签名/授权。
- 本批只增加中文注释和清单/进度文档；未改固定 route、字段、排序、数量上限、拒绝条件、JSON 形状、digest 输入或任何执行路径，未运行测试、构建、格式化或提交。

## 2026-10-10 Core 健康投影注释批次

- `kiana-core/src/health.rs`：按 Web health → DaemonHost → ControlPlane → EventStore `read_all()` / capabilities → operational metrics → Domain `HealthSnapshot` 梳理数据流；补充 operator evidence、entrypoint parity、replay diagnostics 对同一同步投影的复用关系。
- 为错误类型、有限 source event IDs、metrics 计数读取、限制原因、组件构造、EventStore ready 能力门槛和各 probe 状态分别解释调用者、输入、失败传播及设计原因；逐块说明 eventlog/projector/receipt/artifact/recovery 组件如何从事件视图和 adapter 声明得出。
- 明确 EventStore capability 是 adapter 声明，不是本函数现场探测；provider/broker/telemetry/daemon 在 Core 没有独立探针时保持 Unknown；Domain digest 只绑定快照内容，不是签名、授权或外部健康证明。
- 发现 `docs/roadmap/observability-audit-baseline.md` §17 将“未持久化 projector checkpoint”描述为 readiness 拒绝条件，但 `status_for_probe(Readiness)` 没有读取 `projector.checkpoint_durable`（该能力当前固定写为 `false`）；它直接检查显式/推断游标、游标缺口、unknown 和 orphan。Metrics 在未观察到 projector cursor 时会增加 `projector_cursor_inferred` 并导致降级，但若输入中有显式游标，checkpoint capability 本身不会单独阻断状态。相关测试样本还同时包含 cursor lag/unknown，不能单独证明 capability 条件。仅记录差异，没有修改逻辑或规范。
- 本批只增加或修订中文注释及清单/进度文档；未运行测试、构建、格式化或提交。

## 2026-10-10 DEP-11 typed health aggregation 注释批次

- `kiana-domain/src/health_aggregation.rs`：补充与 OA-11 EventLog projection 的差别、输入字段语义、state/phase 对应、摘要和报告的自洽校验，以及从 caller-supplied facts 到 `HealthSnapshot`/`HealthAggregationReport` 的数据流。
- `kiana-core/src/health_aggregation.rs` 与 `kiana-daemon/src/health_aggregation.rs`：说明 Core 是 Domain reducer 的薄门面，DaemonHost helper 只覆盖 probe 并重算 input digest；错误转成 `PortError::Failed`，整个路径没有 EventStore/Provider/Broker I/O、重试、日志或 admission side effect。
- 真实调用关系核对结果：`DaemonHost::{deployment_health, drain_health, maintenance_health}` → daemon `evaluate/evaluate_probe` → Core `aggregate_health` → Domain `evaluate/build_report/status_for`。目前没有发现这些 DaemonHost 方法的 CLI/UI/HTTP 或其他仓库内生产调用者；Core `validate_health_aggregation` 也没有生产调用点。Domain tests 直接构造输入，证明的是 source-level contract。
- 明确 `startup_report_digest` / `component_evidence_digest` 仅检查 `sha256:<64 hex>` 外形并进入 input digest，不读取原报告或证据；`generation` 只要求非零并纳入摘要，没有与 lease generation 做绑定；`admission_allowed` 只表示 readiness status 的派生结果，不执行接纳控制。DEP-11 仍为 partial/source，与 `CURRENT_STATUS.md` 一致。
- 本批只增加/修订中文注释及来源清单/进度文档；没有改 schema、字段、状态顺序、错误码、digest 输入或调用路径，未运行测试、构建、格式化或提交。

继续以 Git 当前修改集为导航，不从函数名推断用途。优先挑选共享工作树中尚未完成注释审阅的入口/核心模块，沿调用者、输入来源、事件/存储副作用、失败处理和下游消费者核验后再编辑；同一批其他 Codex 的功能改动只读保留，任何源码逻辑问题单独记录，不在注释任务中修复。

## 2026-10-10 DEP-10 启动协调合同注释批次

- `kiana-domain/src/deployment_startup.rs`：补充固定 Manifest → Root → Trust → Lease → Store → Migration → Projector → Capacity 阶段序列、adapter supplied facts、wire request、report 和 `derive_result()` 的输入输出及拒绝优先级；解释 `StartupStageFact`、`StartupCoordinatorInput/Request/Report` 及状态/恢复枚举的业务字段。
- 说明 Serde snake_case wire 名称、未知字段拒绝、构造器排序与反序列化不排序的差异、严格递增阶段约束、reason/remediation 256 UTF-8 字节边界、稳定错误字符串和 SHA-256 自摘要的能力边界；重点标出 Unknown 保留、Fresh pending operation 阻塞、ExplicitRecovery 不执行真实恢复。
- `kiana-core/src/deployment_startup.rs` 与 `kiana-daemon/src/startup_coordinator.rs`：解释 DaemonHost → daemon adapter → Core facade → Domain reducer 的静态委托、`String` 到 `PortError::Failed` 的错误传播以及无 I/O/lease/migration/admission 副作用的边界。
- 实际调用关系核对：源码中 DaemonHost 方法调用 daemon helper，helper 调用 Core reducer；当前未发现生产入口/进程启动路径调用公开 `DaemonHost::evaluate_startup()`，Domain 构造和 report reducer 的调用集中在 fixture。不能把 additive route 说成已接入真实启动流程。
- 只记录的合同缺口：`source_cursor` 可为零且不参与阶段折叠；`config_revision`、`journal_digest`、recovery digest 只做格式/摘要绑定，不回读对象或作为对应阶段 freshness 比较；显式恢复模式只允许 reducer 继续得出 Ready，不实施恢复；重复 stage 的专属错误分支先被严格顺序检查截住。这些属于当前源码合同边界，未修改行为。
- 本批只增加或修订中文注释及清单/进度文档；未改 schema、字段、错误码、阶段顺序、摘要输入、调用路径或执行逻辑，未运行测试、构建、格式化或提交。

## 2026-10-10 DEP-05 operation lifecycle 注释批次

- `kiana-domain/src/operation_lifecycle.rs`：补充 EventStore/JournalFrame 外部事实 → `OperationTransition` → `OperationJournal::{append,replay,validate}` → Core read-only projection 的数据流；解释 operation 状态与 phase、所有 kind 的允许迁移、终态/Unknown fence、preflight 标记和 deadline 对比。
- 为 `PhaseDeadline`、`OperationTransition`、`OperationJournal` 的字段和构造/校验/摘要方法补充输入来源、输出、稳定错误、调用关系、可变内存副作用和“不证明 EventStore 提交/外部效果”的边界；说明 `source_cursor`、sequence、revision digest、evidence refs、Unix 毫秒时间、Serde wire schema、文本/列表/transition 上限的业务含义。
- `kiana-core/src/deployment_operation.rs`：补充公开 Core facade 的预期路径、Domain 错误原样传播和无持久化/lease/supervisor/admission effect 的说明；指出 `replay_operation_journal()` 只用传入 journal 的 operation/revision 身份，不验证输入 journal，也不比较输出投影与该 snapshot，截断 transitions 可返回较早投影。该 gap 已在 roadmap 记录，本次未修复。
- 只记录的 Domain 合同缺口：空 `evidence_refs` 可带合法空列表摘要通过；`OperationJournal::validate()` 未逐条调用 `OperationTransition::validate()`，嵌套 schema/version、reason/evidence 形状、自摘要及 kind/from-state→state/phase 映射没有被完整重验；journal 顶层 reason/evidence refs 也未与最后一条 transition 比较。无密钥外层 digest 不能弥补这些绑定缺失。
- 容量错误边界：正常合法状态图无法累积到 256 条 transition；但不完整 journal 校验可放行畸形的非终态链并到达上限，随后 `append()` 会先更新内存 projection/list/digest，再由末尾校验返回容量错误，不回滚。已在方法注释标明前提与结果，未改变行为。
- DEP-05 仍为 `partial`；现有 `CURRENT_STATUS.md` 有 focused `local_behavior` 收据，但未发现生产 facade 调用点，不能称为已接入部署运行路径。本批只增加/修订中文注释和清单/进度文档；未改状态图、签名、错误码、摘要输入或执行逻辑，未运行测试、构建、格式化或提交。

## 2026-10-10 DEP-06 operation lease 注释批次

- `kiana-domain/src/operation_lease.rs`：补充 operation/instance/root/epoch/时钟/TTL → `OperationLeaseCas` → lease/revision 投影 → 后续持久化 adapter 和 effect-time fence 检查的数据流；说明 Domain 只定义确定性值合同，不持有 OS 锁、写 EventLog、跨进程持久化或启动/停止 writer。
- 为 `OperationLeaseState`、`OperationLease`、`OperationLeaseCas`、wire/schema/TTL 常量和所有方法补充字段语义、调用顺序、输入输出、稳定错误、时间边界、可变内存副作用与摘要能力边界；特别说明 `Active` 只表示未显式归档，不能单独证明此刻有效。
- 说明 CAS revision 的本地旧快照拒绝与持久层原子 CAS 的差别、过期租约必须先显式 expire、heartbeat 以调用方 now+ttl 替换期限、release 需要 owner/fence/epoch/时间匹配，以及 fence 只保留最近 token、无法证明完整历史不复用。
- `kiana-core/src/deployment_lease.rs`：补充 Core 薄门面到 Domain `validate()` 的调用、错误原样传播和无存储/日志/重试/写入副作用；源码检索未发现该 facade 的生产调用者，恢复 activation helper 目前只见于测试调用链，不能据此称为部署写入路径已接通。
- 只记录的合同边界：`OperationLease::validate()` 不接收当前时间，也不实施 24 小时 TTL 上限；CAS 递归校验因此不重复检查 TTL。`OperationLeaseCas::validate()` 对 `last_lease` 仅校验 lease 自身和 root，不强制终态、revision/epoch/fence 关系；无密钥摘要不构成身份认证或并发提交证明。跨进程竞争、历史 fence 去重和 effect-time 隔离仍需后续持久化/执行 adapter。
- 本批只增加中文注释和清单/进度文档；未改 schema、字段、状态迁移、revision/fence 规则、错误字符串、摘要输入或调用路径，未运行测试、构建、格式化或提交。

## 2026-10-10 DEP-07 storage preflight 注释批次

- `kiana-domain/src/storage_preflight.rs`：补充 ProjectTrust、StorageRoot、adapter capability/file identity/capacity 输入 → `StoragePreflightRequest` → `StoragePreflightReport::{evaluate,validate}` 的流程；逐项解释 root/project 绑定、信任、相对路径词法检查、元数据 flags、能力门槛、bytes/inodes 比较、稳定 reasons/remediation 和摘要。
- `kiana-core/src/storage_preflight.rs`：说明两个 Core API 只转调 Domain reducer/validator，分别怎样返回 Ready/Blocked 或传递结构错误，以及它们没有文件系统、锁、日志、EventStore、approval 或部署状态副作用。
- 实际调用关系核对：生产源码没有调用 `StoragePreflightReport::evaluate()` 或这两个 Core facade；直接构造/评估在 DEP-07 Domain fixtures。`DaemonHost::storage_root()` → `resolve_storage_root()` 是独立实际路径，未调用本 preflight。被其他 Codex 修改的 `kiana-daemon/src/storage.rs` 保持未编辑。
- 明确 Ready 只表示提交给纯 reducer 的输入未触发已实现 Blocked 条件，不证明目录可写、实时 bytes/inodes 充足、adapter 能力真实、硬链接身份已现场核验或 effect 边界没有 TOCTOU；request digest/report digest 也不是来源签名。
- 只记录的源代码缺口：请求允许身份快照列表为空或不覆盖全部 requested paths，因此缺少路径身份不会单独阻塞；`validate_identity_metadata()` 没有调用 `StorageFileIdentity::validate()`，且 evaluate 的属性分支未覆盖全部组合，例如 `exists = false, regular_file = true` 的摘要自洽对象没有匹配的 blocker，可能成为 Ready。`StorageSecurityCapabilities::current()` 又按编译目标声明 Unix 能力，不探测具体 mount/adapter。以上只记录，未修改判断逻辑或规范。
- 另外，`StoragePreflightReport::validate()` 只检查 `checked_paths` 的排序/重复，不重新做词法路径校验；解码报告通过该方法不能成为路径访问授权。Root 被映射成 reason 后仍需满足报告自己的非空 ID/摘要外形，因此 nil root ID 或 malformed root digest 最终会让 `evaluate()` 返回 `Err`，并非所有 root 校验失败都能生成 Blocked 诊断报告。
- 本批仅编辑两份 Git 原先干净的 Domain/Core 源文件中的注释和本进度/清单文档；未触碰已被其他 Codex 修改的 daemon storage adapter，未改 schema、字段、错误、摘要、分支或调用关系，未运行测试、构建、格式化或提交。

## 2026-10-10 DEP-09 supervisor 合同注释批次

- `kiana-domain/src/supervisor.rs`：补充部署协调器预期请求 → `SupervisorPort` → 平台 adapter → `SupervisorObservation` → Core/Domain 回绑校验的数据流；解释 backend/action/outcome/signal、operation/instance、deployment/root/lease digest、fence、epoch、generation、deadline、evidence reference 等字段的业务含义。
- 为 request/observation 构造、独立校验、跨请求校验和摘要补充调用者、输入输出、稳定错误、检查顺序与边界；说明 `target_ref` 只检查 backend 前缀/PID 文本等格式、不查询服务目录，`lease_digest`/fence 不回查真实租约，deadline 是 DTO 中声明的相对期限而非执行器强制取消。
- 明确 `SupervisorOutcome` 和 `StopReport.confirmed` 来自 adapter/DTO 声明；内容摘要无密钥，`evidence_ref` 不读取实际证据，无法证明内核已启停目标。跨请求验证不比较 deadline 与 `StopReport.waited_ms`，也不把 `StopReport.execution_id` 绑定到 request 的 operation/instance/target。
- `kiana-core/src/deployment_supervisor.rs`：说明 Core facade 只委托 `validate_against()` 并原样返回 Domain 字符串错误；不读取 lease/EventStore/时钟、不调用端口，源码未发现生产调用者。
- `kiana-daemon/src/supervisor_adapters.rs`：解释 `async_trait`/`SupervisorPort` 端口位置、支持 backend 名称并非运行时可用、adapter 仅保存 backend；无效请求为 `Failed`，backend/action 冲突为 `Conflict`，合法请求目前都会得到 `Unavailable`，没有平台启停、日志、lease 获取或重试。
- 实际调用关系核对：Domain 公开构造器和验证主要由 Domain fixtures 使用；Core facade 当前只在 crate 导出，无生产调用边；唯一 daemon adapter 实现仅被 source guard 搜索，不见生产实例化。`kiana-domain/src/process_supervisor.rs` 已有其他工作区改动且不属于本批写入范围，保留未动；其 `StopReport` 合同仅被本模块只读核对。
- 本批记录的实现边界：backend enum 与 `SUPPORTED_SUPERVISOR_BACKENDS` 不代表平台可执行；`target_ref` 不做服务注册表查找；`expected_generation` 允许为零且没有权威来源绑定；`evidence_ref` 的后缀可为空；`StopReport::validate()` 对 `confirmed` 的要求弱于 daemon 生成路径，缺少 `process_group_id` 的绑定，observation 又未将 execution ID 绑定到 request；deadline 未与 waited time 比较。只记录，没有改变逻辑或接口。
- 本批仅增加中文注释与来源清单/进度文档；未改 serde schema、字段、错误码、digest 输入、端口接口、动作分派或任何运行逻辑，未运行测试、构建、格式化或提交。

## 2026-10-10 DEP-12 admission/drain 注释批次

- `kiana-domain/src/deployment_admission.rs`：补充调用方汇总的 Health/Startup/Lease/Operation/maintenance/pause/drain 事实 → `DeploymentAdmissionInput` → `validate()` → `derive_decision()` → `DeploymentAdmissionDecision` 的数据流；逐字段解释时间、epoch、租约状态、source cursor 和两类工作许可含义。
- 为 `MaintenanceWindow`、status/input/decision 类型及构造、校验、摘要、重算绑定补充中文文档；解释 Serde wire 版本与拒绝未知字段、UTF-8 字节上限、可选维护窗口、drain deadline 组合和无密钥摘要的能力边界。
- 逐块说明状态优先级：lease/epoch → operation/health Unknown → 健康/startup → migration/backup → drain → maintenance window → intake pause → Ready。指出同时满足多个标志时，顺序会影响状态及 `allow_existing_work`；正常拒绝返回 `Ok(decision)`，调用者须消费两项许可位。
- `kiana-core/src/deployment_admission.rs`：说明 Core 两个公开函数只是 Domain reducer 与 `validate_against()` 的纯委托；输入错误原样返回 `String`，不会在 Core 实施 gate。
- `kiana-daemon/src/deployment_admission.rs`：补充 `DaemonHost::deployment_admission()` → helper → Core → Domain 静态调用路径，说明 helper 仅把 `String` 映射为 `PortError::Failed`，并不会把 Blocked/Unknown decision 改成错误。
- 实际调用关系核对：`DaemonHost` 方法是唯一生产源码引用调用，仓库没有 input builder、调用该宿主方法的生产调用者或 decision consumer。因此当前是 additive 只读决策 route，不是 ControlPlane/scheduler intake gate；`CURRENT_STATUS.md` 仍记录 DEP-12 为 `partial/source`。
- 需要维护者注意的输入边界：health digest 不绑定实际 HealthAggregationReport；lease bool/epoch 是供给值，不加载 lease；`now_unix_ms` 无可信时钟核对，maintenance revision 无当前版本/CAS 检查；source cursor 只要求非零并进摘要、不参与决策；`operation_unknown = false` 不表示没有活跃 operation。摘要可由重算字段的调用者重新生成。
- 本批只为三份 Git 原先干净的 Domain/Core/Daemon 源文件增加中文注释，并更新来源清单/进度文档；未改状态顺序、字段、摘要输入、错误码、接口、队列/operation/EventLog 行为，未运行测试、构建、格式化或提交。

## 2026-10-10 DEP-13 unified shutdown 注释批次

- `kiana-domain/src/deployment_shutdown.rs`：补充关闭协调器收集 phase ack/边界标记/计数/时间 → `ShutdownInput` → 固定阶段校验 → `build_report()` → `ShutdownReport` 的数据流；逐项解释八个阶段及每个 DTO 的字段、状态和错误边界。
- 解释 `ShutdownPhase::ALL` 与 enum `Ord` 顺序共同构成输入排序合同；说明缺少/Unknown/Pending/Failed ack、边界 bool 未确认、未解决计数、deadline 超时按固定优先级映射到 `Unknown`/`NeedsRecovery`/`Stopped`，以及计数使用饱和加法以避免溢出回绕。
- 明确所有 phase/effect/时间/布尔事实由调用方提供；source cursor 只要求非零、detail digest 只检查格式，phase `Confirmed` 与对应的八个 bool 没有逐阶段交叉绑定；无密钥摘要不证明来源、时钟新鲜度、durable flush 或真实关闭。报告 `Stopped` 仅表示 supplied snapshot 满足 Domain 合同。
- `kiana-core/src/deployment_shutdown.rs` 与 `kiana-daemon/src/deployment_shutdown.rs`：解释 DaemonHost additive route → daemon helper → Core → Domain 的委托及 `String` 到 `PortError::Failed` 的转换；Unknown/NeedsRecovery 是 `Ok(report)`，各层没有 cancel/drain/flush/close 或持久化副作用。
- 实际调用关系核对：仓库生产源码没有组装 `ShutdownInput`、调用 `DaemonHost::deployment_shutdown()`、调用 Core validator 或消费 report。另有独立的 `DaemonHost::shutdown()` 执行 workflow queue shutdown、EventStore flush/close 与 observability flush，但它没有调用 DEP-13 reducer；相关 `kiana-daemon/src/lib.rs` 正由其他 Codex 修改，本批只读检查未编辑。
- 记录一项可见性问题：`DaemonHost::shutdown()` 用 `let _ = self.workflow_service.shutdown().await?` 丢弃 `WorkflowQueueShutdownReport`，而该报告包含 `recovery_required_count`。workflow service 在 fence 返回 `ResultUnknown` 或错误时会递增该值；目前 Host shutdown 未将它纳入返回结果。本任务未改这条有副作用的路径。
- 记录一项校验分支问题：phase 邻接严格递增条件已拒绝所有重复值，因此随后 `BTreeSet` 的 `shutdown_phase_duplicate` 专用错误在当前条件顺序下不可达；已仅添加注释说明，没有调整错误码或分支。
- fixtures 使用合成 phase/input/report，Core/Daemon guards 检查源码字符串；它们不证明 acknowledgement 真实，也不证明 DaemonHost route 已被执行。`CURRENT_STATUS.md` 仍把 DEP-13 标为 `partial/source`。本批只补中文注释和清单/进度文档，未运行测试、构建、格式化或提交。

## 2026-10-10 DEP-14 observability evidence 注释批次

- `kiana-domain/src/deployment_observability.rs`：补充外部 projector/adapter（当前没有生产 bundle producer）→ `LifecycleMetricFact` → `LifecycleEvidenceBundle` → DaemonHost/Core/Domain 校验的数据流；解释 metric name/value/unit、operation/revision/cursor、trace/log/audit 引用及 bundle digest 字段。
- 为 metric/bundle 构造、校验和摘要说明调用者、上限、固定 schema、Serde 未知字段拒绝、首错顺序、嵌套 metric 自校验、operation/revision/cursor 精确相等绑定、metric name 大小写敏感去重，以及通过 `validate_against` 与真实来源校验的区别。
- 明确本合同不采集指标、不写日志/audit、不导 trace、不读 EventStore、不验证外部 effect；`structured_log_digest`/`audit_event_digest` 仅验格式，`trace_ref` 不要求 scheme/不查 store，revision/op/source cursor 只在 DTO 内部相等。
- 注释特别收紧 `secret_free` 的解释：构造器无条件设为 true，validate 只检查这个位；有限 marker 检查只覆盖 trace_ref 的控制字符、`secret` 和 `token=`，不扫描 metric name/unit/operation_ref 或 log/audit 原文。通过校验不能称为“全面脱敏”或“秘密已证明不存在”。
- `kiana-core/src/deployment_observability.rs` 与 `kiana-daemon/src/deployment_observability.rs`：说明公开 DaemonHost 路由 → Daemon helper → Core validator → Domain `bundle.validate()` 的静态链和 String→`PortError::Failed` 映射；生产代码没有 bundle producer 或 route consumer。
- Domain fixtures 使用合成 hash/metric，Core/Daemon tests 是 `include_str!` 源码守卫；只能证明 supplied DTO 的格式与绑定规则，不能证明实时 telemetry 来源、脱敏、durable facts 或 route 执行。`CURRENT_STATUS.md` 将 DEP-14 记录为 `partial/source`。
- 本批只给三份 Git 原先干净的 Domain/Core/Daemon 源文件增加中文注释，并更新清单/进度文档；未改字段、摘要输入、校验分支、错误码、采集/导出行为，未运行测试、构建、格式化或提交。

## 2026-10-13 DEP-15 ops diagnostics 注释批次

- `kiana-domain/src/ops_diagnostics.rs`：说明调用方 facts → 四轴 `OpsDiagnosticsInput` → Domain 顺序/cursor/文本/摘要校验 → `OpsDiagnosticsReport` 状态归并、退出码与渲染 DTO 的数据流；逐项解释 mode、axis、各状态、时间/cursor、evidence、limitations、复现命令和摘要字段。
- 补充构造器、校验器、状态归并、报告绑定、JSON/人类渲染、复现命令及安全文本/摘要格式 helper 的调用者、输入输出、错误传播、设计原因和副作用边界；说明 Unknown > Blocked/Missing > Degraded > Ready，退出码仅为报告字段，复现命令不会执行。
- 明确 input/report 的 SHA-256 都是无密钥内容指纹，不认证事实来源；`observed_at_ms` 仅非零并进入 input digest，没有时钟/EventLog 校验。`validate_against()` 不保存或比较 input digest/观察时间，且 limitations 只比较数量；`validate_digest_only()` 不重查四轴唯一/顺序和 fact cursor 与 report cursor 的关系。
- `kiana-core/src/ops_diagnostics.rs` 与 `kiana-daemon/src/ops_diagnostics.rs`：说明 DaemonHost 静态路由 → adapter → Core 模式检查/委托 → Domain reducer 的边界，以及 Domain `String` 错误如何变成 `PortError::Failed`；没有事实采集、EventStore/文件/网络 I/O、重试或日志。
- 为 `kiana-domain/tests/dep15_ops_diagnostics.rs` 解释合成 facts、固定测试时间/cursor、测试覆盖范围，并记录测试名包含 `wrong_mode` 但没有调用模式校验函数的差异；为 Core/Daemon `include_str!` guards 说明它们只检查源文本，不能作为路由运行证明。
- 调用关系核对：当前生产源码没有 facts 汇总器，也没有 CLI/Workbench/Web/Desktop 消费者调用这些诊断路由；三个模式共用同一 reducer。`CURRENT_STATUS.md` 将 DEP-15 标为 `partial/source`，远端测试状态不由本次源码阅读推断。
- 发现项只作记录，未改逻辑：报告 limitations 字符串内容可在长度不变时被替换并重算无密钥 digest；报告没有绑定观察时间；无 input 的渲染校验不检查四轴顺序/唯一性与 cursor 一致性；现有 Domain 测试名声称检查 wrong mode 但未实际断言。状态组合也没有由这几条用例穷举。
- 本批只在六个 Git 原先干净的 DEP-15 源码/测试文件增加中文注释，并更新来源清单/进度文档；未改字段、算法、条件、错误码、摘要输入、接口或调用顺序，未运行测试、构建、格式化或提交。

## 2026-10-13 DEP-16 reconcile 注释批次

- `kiana-domain/src/deployment_reconcile.rs`：补充四目标 facts → `ReconcileInput` → 固定状态优先级/命令决策 → `ReconcileReport` 的数据流；解释 RepairTarget、command/status、RepairFact 的 cursor/generation/epoch/lease/fence/result_unknown/evidence 字段及目标顺序合同。
- 为 fact/input/report 的构造、校验、摘要、绑定和渲染说明调用者、参数含义、稳定错误、返回值、纯函数副作用边界；说明 Inspect/PlanRepair 只返回报告，CommitRepair 最多得到 `commit_ready`，不会应用计划。
- 明确上游 facts/actor/approval/source/lease/evidence digest 均为调用方声明；approval 只验 SHA-256 外形，actor 不查身份，epoch/lease/fence 不回读权威来源，摘要无密钥且不能认证来源。未知效果优先派生 Unknown，不能自动重跑。
- 记录 generation 溢出边界：`derive_decision()` 在检查状态和命令前先执行 `expected_generation.checked_add(1)`，因此即使 Inspect 输入健康或无需修复，最大 u64 generation 也会返回 `deployment_reconcile_generation_overflow`；本批只解释现状，没有改分支。
- `kiana-core/src/deployment_reconcile.rs` 与 `kiana-daemon/src/deployment_reconcile.rs`：说明 Host `ops_reconcile()` → daemon adapter → Core → Domain 的静态委托、错误字符串到 `PortError::Failed` 的包装，以及 Unknown/Blocked 属于成功 report 状态而非 `Err`。
- 调用关系核对：仓库生产源码没有 ReconcileInput builder 或 ops_reconcile 调用者；Domain fixture 是合成 cursor/digest，Core/Daemon 是源码守卫。当前 `CURRENT_STATUS.md` 将 DEP-16 记录为 `partial/source`，并明确 reducer 不读取或重建 EventLog/ProjectionStore/index/queue/lease，不执行 approval 或 repair。
- `kiana-domain/tests/dep16_reconcile.rs` 在工作区已有其他 Codex 修改，本批只读查看，未编辑；其他 DEP-16 测试守卫与投影/索引/队列支持合同留待后续模块批次。
- 本批只给三份 Git 原先干净的 Domain/Core/Daemon 源文件增加中文注释并更新进度/来源清单；源码差异经文本检查仅含注释，未运行测试、构建、格式化或提交。

## 2026-10-13 Projection checkpoint 注释批次

- `kiana-domain/src/projection_contracts.rs`：解释 checkpoint 是可重建读模型缓存而非权威事实，说明 state/state digest/checkpoint digest、projector、source cursor、已处理 EventId 列表、Serde JSON 往返、兼容版本和构造器规范化行为。
- `kiana-core/src/projection_checkpoint.rs`：梳理 committed event slice + source cursor + fold → ReplayProjection → checkpoint 的内存数据流；解释 ProjectionDriver 的 Ready/Paused/RebuildRequired、失败保留旧 checkpoint、显式 retry/rebuild、EventId 去重、数量限制和错误前缀。
- 明确 fold 回调的确定性/无副作用要求只能由调用者遵守；helper 不持有 EventStore/ProjectionStore，不持久化 checkpoint，也不从事件本身推导全局 EventLog cursor。`from_checkpoint()` 只检查空 tail cursor 不变或非空 tail cursor 前进，尾部完整性和顺序由上游一致快照读取者负责。
- 调用图核对：Core `notification_projector.rs` 中 `NotificationProjection::apply_committed()` 调用 ReplayProjection，但全仓库没有找到该类型的生产实例化调用者，当前构造点是测试；ProjectionDriver 对外导出，仓内未见生产构造调用点。注释没有把源码级连接描述成已接通的运行请求路径。
- 为 `kiana-domain/tests/er07_projection_checkpoint.rs`、`kiana-core/tests/er07_replay_guard.rs`、`kiana-core/tests/pd09_projection_driver.rs` 说明合成数据、摘要/身份/重放/暂停覆盖和静态源码守卫的证明边界。
- 容易误解的边界：重放时重复 EventId 会直接跳过 fold，不比较重复事件 payload；EventStore 的全局唯一性及事件片段完整性属于调用者前置条件。摘要无密钥，只验证内容一致性，不认证 checkpoint 来源。仅记录，没有调整逻辑。
- 本批只修改五份 Git 原先干净的 Domain/Core 源码与测试文件中的中文注释，并更新清单/进度；未运行测试、构建、格式化或提交。

## 2026-10-13 身份与授权合同注释批次

- `kiana-domain/src/identity.rs`：梳理 Daemon local principal/project resolution → Core SessionAssignment → `session.assigned` EventLog CAS；为主体引用、canonical root identity、role/department/epoch 字段、构造/校验/digest 补充输入输出和调用责任。
- `kiana-domain/src/identity_contracts.rs`：解释 Principal/Membership 时间窗、SecretRef 的 opaque store key、ProviderAccount 与 ServiceIdentity metadata、ConfigSnapshot 非秘密字段和 AuthoritySnapshot epoch/assignment 集合；说明 IdentityMigration 仅是兼容事实 DTO。
- 调用关系核对：Daemon 调用 `AuthenticatedPrincipalRef::local()` 和 `ProjectIdentity::new()`，Core 创建/persist SessionAssignment；Provider `ConfigResolver` 构造 ConfigSnapshot；`IdentityResolver` 只有 `kiana-ports` trait，仓内没有生产实现，运行期另用 Core `SecurityAuthoritySnapshot`。Principal/Membership/ProviderAccount/ServiceIdentity/Domain AuthoritySnapshot 的构造与消费仍以 fixtures/ports contract 为主。
- 易误解边界：`local()` 只产生固定 local-user identity label，不执行 OS 登录；主体/项目/assignment digest 无密钥；validate 不等于身份认证或角色授权。SecretRef 不读取值，ConfigSnapshot 的敏感字段检查只覆盖有限 key/value 模式，不能声称完整 secret 扫描。
- 发现项只记录未修：`ProjectIdentity::validate()` 验证 self digest，但没有确认 `project_id == stable_project_id(canonical_root)`，也不重查 device/inode/ProjectTrust；`AuthenticatedPrincipalRef::validate()` 不对当前时钟检查 expires_at。`AuthoritySnapshot`/`Membership` 校验只检查 supplied DTO，不查询权威存储。
- CI-02/CP-01 baseline/status 记录提及的专用 workflow 在当前 checkout 和 `git ls-files` 中都不存在；静态 `include_str!` guard 与 Domain fixtures 不能替代 workflow 运行证据。未改 baseline/status。
- 为 CI-02/CP-01 Domain fixtures 和 Core guards 补充中文场景与证明上限；`SecretRef` 注入 `raw_value` 的用例首先覆盖 strict unknown-field 拒绝，ConfigSnapshot 用例才命中 token/bearer marker。
- 本批仅修改此前干净的两份 Domain 实现和四份 Domain/Core 测试注释，并更新清单/进度；源码差异过滤只见注释，`git diff --check` 通过。未运行测试、构建、格式化或提交。

## 2026-10-13 CM-07 Workspace snapshot 注释批次

- `kiana-domain/src/workspace_snapshot.rs`：解释 WorkspaceTrust、EntryKind、ReadDisposition、limits、identity、单文件证据和聚合快照字段；说明 Indexed 的安全位如何按声明字段重算，以及 DTO 摘要/validate 不查询磁盘或认证来源。
- `kiana-query/src/workspace_snapshot.rs`：补充 root/options → canonicalize → 排序递归扫描 → 信任/硬链接/大小筛选 → 读取前后身份比较 → UTF-8 正文与 Domain snapshot 的数据流；逐函数说明扫描限额、路径归属、错误传播、平台 metadata 和默认 Unknown 行为。
- 明确本路径目前没有生产调用者，也没有接入 ProjectTrust resolver；Query 测试直接调用。时间预算只在遍历边界检查，不会中断正在执行的系统调用；普通路径 open 加前后 metadata 比较不是完整 TOCTOU 防护。
- 记录实现边界但没有修复：Domain `WorkspaceSnapshot::new()` 用普通 `sum()` 聚合外部提供的 byte 数值，极端输入存在整数溢出可能；反序列化快照 validate 不重算 `total_bytes_read` 与 file 行之和。单文件 validate 不比较 `file_digest`、after content digest 与 bytes_read。Query 总内容预算超限时，`record_file()` 标记 limitation 后直接返回，候选路径既不进 contents 也不进 snapshot.files。
- 为 Domain 合同测试说明其使用手工构造 DTO、不触碰磁盘；为 Query tests 说明它实际读取临时目录并覆盖 Trusted/Unknown/symlink root；为 Core `include_str!` guard 说明它只检查源文本，不证明 runtime 调用或 trust authority。
- `docs/roadmap/cm07-workspace-snapshot-baseline.md` 声称远端 CI wiring，但当前 checkout 与 Git 跟踪列表中都不存在 CM-07 对应 workflow；只记录 baseline/当前 checkout 差异，没有更改 roadmap、状态账本或 workflow。
- 本批只修改 Git 原先干净的五份 Domain/Query/Core 源码与测试文件注释，并更新来源清单/进度；源码差异过滤只见注释，`git diff --check` 通过。未运行测试、构建、格式化或提交。

## 2026-10-13 CM-08 Chunk provenance 注释批次

- `kiana-domain/src/chunk_provenance.rs`：说明 ChunkRange 的左闭右开 byte range、SourceChunk 的处理版本/内容摘要/parent 分组、ChunkSet 的连续重建合同；补充各字段、构造、验证、错误和纯内存副作用边界。
- `kiana-query/src/chunker.rs`：梳理 source text → 扩展名 parser 选择 → 固定前缀/标题启发式 → 语义段与 UTF-8 窗口 → range/content/transformation/chunk 摘要 → Domain ChunkSet 校验；解释默认字节预算、max chunk 失败、换行优先和 UTF-8 边界。
- 调用关系核对：`chunk_text()` 仓内只由 Query tests 调用，没有生产调用者把 CM-07 `WorkspaceReadContent` 接进分块/index 构建。输出是派生上下文证据，不是 AST、编译器事实、authority 或权限。
- 实现边界只记录、不修复：Domain range 校验无法重算 line number；ChunkSet 不验证 source digest 对应实际 source，也不限制 `line_end <= source_lines`；`transformation_digest` 与 parent id 只做摘要外形检查；`reconstruct()` 自身不先 validate，外部 DTO 使用者应先显式校验。
- 为 Domain tests 说明手工构造的连续/重叠 range fixture；为 Query tests 说明内存 Rust/Markdown/Unicode 文本覆盖；为 Core `include_str!` guard 说明它只检查源码标记，不执行算法或证明生产接入。
- `docs/roadmap/cm08-chunker-baseline.md` 记录了远端 CI wiring，但当前 checkout 的 workflow 目录没有 CM-08 专用 workflow；只记录这一现状差异，不更改 baseline 或状态账本。
- 本批仅修改此前干净的五份 Domain/Query/Core 源码和测试文件注释，并更新来源清单/进度；源码差异过滤只见注释，`git diff --check` 通过。未运行测试、构建、格式化或提交。

## 2026-10-13 P4-J7-22 图片准入注释批次

- `kiana-domain/src/image_input.rs`：解释 Artifact bytes → ImageInputAdmission → Provider 编译器图片块的预期调用链；逐字段说明 ArtifactRef、ProcessingGrant、policy/data epoch、purpose/class、route/capabilities 和短期 bytes 的来源与去向。
- 补充 raw/source、base64 payload、最终 request body 三层预算及默认数值；解释 URL/path 拒绝、Artifact/grant/hash/scope/MIME/retention/route/capability 校验顺序、稳定错误和 `ModelError` 映射。
- 调用图核对：Provider Gateway 提供 `prepare_call_with_images()` 并消费 admission，但仓内没有生产代码调用该 API/构造 admission；Domain fixtures、Provider unit fixture 和 Core `include_str!` guard 是当前可见调用。Provider request 文件正在其他 Codex 修改，本批只读未编辑。
- 发现项只记录、不修复：Provider `validate_image_admissions()` 把 `image.admitted_at_unix_ms` 作为 dispatch `now` 传入，因此 grant retention 未按真实发送时刻重验；policy revision/data epoch 只验非零/摘要格式，不查当前权威值；当前合同按 MIME label/Artifact schema 比较，不解码图片头验证 bytes 格式；admission 的 metadata 字段公开可变，构造值不是签名 capability。
- 为 Domain fixture 说明 `fixture-png` 是任意字节、只验证 hash/scope 标签，不验证真实图片格式或 wire encoding；为 Core guard 说明它只检查 Domain/Provider/source/baseline/workflow 文本，不执行网络请求。P4-J7-22 专用 workflow 当前在仓库存在。
- 本批只修改此前干净的 Domain 图片源文件、Domain fixture 和 Core guard 注释并更新清单/进度；源码差异过滤只见注释，`git diff --check` 通过。未运行测试、构建、格式化或提交。

## 2026-10-13 CM-10/CM-11/PD-19 索引注释批次

- `kiana-domain/src/index_generation.rs`、`index_invalidation.rs`：说明 generation 状态机和 Ready reader、source snapshot 差异、effective digest、rename/tombstone、cache key 的数据流；解释各状态、字段、校验错误和摘要只能校验内容一致性、不能认证来源的边界。
- `kiana-query/src/index_generation.rs`：梳理 `build_context_index()` 扫描 → source manifest → Domain begin/commit → envelope → 临时文件写入/sync/rename → read-back 校验的数据流；明确组件摘要目前由同一整个 ContextIndex 摘要分类派生，未分别构建四个物理索引。
- `kiana-query/src/index_invalidation.rs`：说明 epoch/payload read gate、治理传播计划的 metadata 判断和 Query 到 Domain plan 的委托；当前仓内未发现这些公开 helper 的生产调用者。
- 为 Domain/Query fixtures 和 Core/Query 源码守卫补充测试输入的合成性质、断言范围和证明上限。CM-10、PD-19 两个 Query 测试二进制的两个用例各自复用仅含 PID 的临时目录 helper，存在并行清理冲突可能；这里只记录，没有运行或改变测试。
- 实际调用关系核对：`build_context_index_generation()`、manifest read/write、`plan_index_invalidation()`、`index_read_allowed()`、`governed_index_invalidation()` 在仓内只由测试直接调用；导出不等于接入生产索引路径。
- 实现边界只记录、不修复：Domain `commit()` 在 `next_generation == u64::MAX` 时用饱和递增，结果会违反自身 `ready.generation < next_generation` 校验；snapshot diff 只验证 current 而不验证 previous，重复旧路径可在 map 构造时折叠；rename 仅按 effective digest 配对；manifest 自摘要/plan 摘要无密钥且校验器无法重建其原始输入。
- Query envelope 没有比较 Domain manifest 的 `source_manifest_digest` 与 source manifest 摘要，也不会根据嵌入的 index 重算组件摘要；读取时没有当前 WorkspaceSnapshot，因此不能确认磁盘 index 仍然 fresh。原子 rename 有临时文件和 sync，但没有 CAS/跨进程锁；`manifest_path()` 对绝对路径原样返回，不受 root 限制，不能作为 sandbox。
- `CURRENT_STATUS.md` 中仍列出 `.github/workflows/cm10-index-generation.yml`、`cm11-index-invalidation.yml` 和 `pd19-context-index-generation.yml`，但这些路径在当前 checkout 的 workflow 目录与 Git 跟踪文件中均不存在；该处仅记为状态账本/checkout 不一致，未编辑权威状态文档。
- 本批只修改此前干净的 12 份索引源/测试文件注释及本导读进度清单；源码变更行检查只见注释，`git diff --check` 通过。未运行测试、构建、格式化或提交。

## 2026-10-13 CM-29 projection recovery 注释批次

- `kiana-domain/src/projection_recovery.rs`：解释 ProjectionLagView 从显式 source/projection cursor、generation、data epoch 派生 CaughtUp/Pending/Unknown 的规则、读一致性 gate、原因/摘要字段，以及 UnknownMutationReconciliation 如何绑定原 mutation ID 与原 request digest。
- 为 DTO 字段、构造、validate/read_consistent/reconcile/digest 和文本/摘要 helper 补充调用者、输入输出、首错/稳定错误、副作用及真实性限制；特别说明 `reconcile()` 成功只是 ID/digest 相等，不查询幂等存储，也不把 `ResultUnknown` 解析成最终成功或失败。
- 调用点只读核对：`kiana-query::audit_projector::derive_lag_view()` 与 `kiana-core::receipts::projection_lag_from_events()` 构造 lag view，Inspector snapshot 可携带它。receipt helper 当前从事件切片长度计算 source cursor、从事件 payload 取最大 projector cursor 和末尾 generation/epoch；这依赖传入切片的范围，Domain DTO 本身不会证明它们是同一权威快照。`receipts.rs` 和 `audit_projector.rs` 正由其他 Codex 修改，本批只读、未编辑。
- 全仓未发现 `UnknownMutationReconciliation` 生产调用者；它目前由 Domain fixture 和静态源码守卫覆盖。`CURRENT_STATUS.md` 将 CM-29 记录为 implemented、proof 仍为 source 加远端 CI wiring；没有 durable/live 结论。
- 为 `kiana-domain/tests/cm29_projection_recovery.rs` 说明合成 cursor/digest 的 lag、Unknown 和原 key 核查用例；为 `kiana-core/tests/cm29_projection_recovery_guard.rs` 说明 `include_str!` 静态守卫并非运行时 EventLog/幂等恢复测试。
- 易误读边界：`read_consistent()` 只表示 DTO 有效且 cursor 没有 pending，不校验投影 state 与权威源内容一致；摘要无密钥，不认证来源。仅记录，没有改动行为。
- 本批只给一份 Git 原先干净的 Domain 源文件和两份干净测试文件增加注释并更新进度/清单；未运行测试、构建、格式化或提交。

## 2026-10-13 H14/H17 continuation 注释批次

- `kiana-domain/src/job_handle.rs`：补充长进程从 Core `process.start`、Daemon spawn、`process.started` 事实，到后续每次独立 operation 携带同一 handle 的调用图；逐字段解释 job/start request/start invocation、可选 Run/Turn、owner/session、project digest、authority epoch、process-group evidence 和 Unix 毫秒 TTL。
- 为 `JobHandle::new()`、`validate()`、`digest()` 说明构造时序、Daemon 进程监督 guard、错误映射及副作用；明确 Domain 校验不读取当前时间、不查询 authority，SHA-256 无密钥摘要仅做内容一致性检查，不是认证或授权。
- `kiana-domain/src/invocation_resume.rs`：说明 Runner 批次/Turn/Step → Core Gate 暂停 → Domain binding → 审批事实 → Core 恢复时重载请求/重新授权 → 原结果回到 `drive_run` 的数据流；逐字段解释 event request cursor、原始 Invocation、参数/catalog/authority、owner/session/project/sandbox 与 pending batch 摘要。
- 为 `from_request()`、`validate()`、`validate_against()`、`rebind_event_request()`、`digest()` 说明输入来源、稳定失败、调用责任和为什么 queue digest 由 Runner checkpoint 独立核验；强调审批通过不直接授予 dispatch 权限，恢复仍要重新 prepare/authorize/permit。
- 为 H14 Domain 合同测试、H14 Core `include_str!` 守卫、H17 Domain 合同测试和 Daemon 源码守卫补充文件级证明边界与用例目的：静态字符串检查不是运行时流程，合成摘要不是认证材料，Domain fixture 不启动进程或写审批状态。
- 调用关系只读核对：Core `capabilities.rs` 在 AwaitingApproval 时生成 binding 并暂存；Runner 在 `emit_tool_request()` 生成有序 pending queue 摘要；Core recovery 重载后调用 `validate_against()` 并在显式 resume 时重绑 event request cursor。Daemon 在 spawn 后构造 JobHandle，重启后没有 live entry 时只允许 poll 已落盘 outcome/Unknown，不按 PID 重新挂接。
- 易误改边界：不能把 `process_group_id`/PID 或无密钥 handle digest 当授权；不能让 poll 延长 TTL 或变相重新 start；不能在 resume 时替换原 request/invocation/parameter identity；不能在 Domain 复制 Runner 拥有的有序队列。
- 本批没有发现需修改的逻辑；现有 H14/H17 基线将跨进程 checkpoint/process 恢复、掉电对账、完整 output Artifact durability 和 live/physical proof 留为限制。测试目标没有运行。
- 本批仅修改六份当时 Git 状态干净的 Rust 文件注释和本导读清单；差异逐行检查为注释新增/替换，`git diff --check` 通过。未运行测试、构建、格式化或提交。

## 2026-10-13 CAP-01 / H09 / H16 工具目录与调度注释批次

- `kiana-domain/src/tool_authority.rs`：补充 Domain 文件职责与 DaemonHost → Runner/Harness → CapabilityRequest → ControlPlane → Broker 的调用图；解释 `OPERATION_SPECS` 到 `ToolSpecCollection` 的惰性投影、工具 descriptor 字段、Serde 外部格式、目录摘要、Provider wire 名和启动校验的输入/输出/错误边界。
- 逐函数说明 `ToolCatalogSnapshot::current/validate/descriptor`、目录摘要/hash、name/wire 查找和 action binding。明确目录、risk、side-effect、resource、replay 与并行度字段是映射/调度元数据，不是 capability grant；执行仍需 Core 重新授权。
- 特别解释 descriptor 默认填入的 1 MiB `output_limit` 数值、快照 validator 允许的 16 MiB 上限、只读目录并行度 4 和副作用调用并行度 1。全仓源码搜索未发现 `ToolDescriptor.output_limit` 的生产读取/截断调用点，因此注释没有把目录数值描述成已执行的输出限制。记录：快照 alias 对后续 canonical name 的碰撞检查依赖完整权威校验；通用 `validate()` 只将 operation/capability/minimum-risk/side-effecting 与 ToolSpec 对账，不将任意快照的 alias、wire、output 等全部字段同当前生成快照逐项比较。
- `kiana-domain/tests/cap01_registry.rs`：说明模型可见五项与内部 38 个 operation 的不同规模，测试独立期望 schema 和 descriptor 元数据的映射范围；不把 Domain fixture 说成运行时授权。
- `kiana-capability-broker/tests/cap01_registry.rs`：解释 `async_trait`、共享 handler、重复 operation/alias、binding version 拒绝和未注册 operation 无 shell fallback；这些用例直接验证 Broker 注册/查找边界，不代表通过 ControlPlane，也不执行真实进程。
- 为 `kiana-core/tests/p1_h01_tool_authority_guard.rs` 和 Runner 两份 H09 测试说明 `include_str!` 静态源码守卫与内存 mapper fixture 的证明上限。审阅发现 `catalog_change_cannot_reuse_old_approval` 只验证摘要失效、未消费真实 approval；wire collision fixture 因没有重算 digest 而先命中摘要错误，未单独证明重复 wire 分支；mapper/descriptor 比较也未 dispatch 到 Broker。
- `kiana-domain/src/tool_scheduling.rs`：解释有序 ModelToolCall 到 ToolBatchPlan 的数据流、相邻同资源只读项合组、独占调用形成 barrier、global cap 8 与 descriptor cap 的取小关系、计划校验错误和 Core 逐请求授权边界。
- `kiana-domain/tests/h16_tool_scheduling.rs` 与 `kiana-core/tests/h16_parallel_barriers.rs`：区分 Domain 计划结构 fixture 和 Core 源码 marker guard 的证明范围。已追到当前 Harness 将 `plan_tool_batch()` 结果映射为 `()` 后丢弃，再由 `pending_tools` 按队列逐条发出并等结果；因此 `independent_reads_overlap_but_history_is_source_ordered` 只证明计划有一个 parallel-read group 和顺序 ID，不证明真实调用重叠。H16 baseline 也将真实 bounded worker pool 列为后续工作。
- 发现的文档/实现差异只记录未改：`docs/roadmap/harness-tool-catalog-baseline.md` 称 catalog 不再以 `len == 5` 为架构上限，但当前 `validate_tool_authority()` 仍启动失败关闭地要求恰好五个模型工具；仓库 AGENTS.md 当前也将可见面锁定为五项。需要维护者协调基线与当前合同表述。
- 本批仅修改上述九份此前干净的 Rust 文件注释和本导读清单；未改 `actions.rs`、`tool_catalog.rs` 等被 action catalog digest 纳入源码字节的文件，也未碰另一个 Codex 已修改的宿主/Runner/Provider 文件。逐行差异检查只包含注释，`git diff --check` 通过；未运行测试、构建、格式化或提交。

## 2026-10-13 Capability Action 合同注释批次

- `kiana-domain/src/actions.rs` 没有加入行内注释：`capability_action_catalog_digest()` 通过 `include_str!("actions.rs")` 将完整源码字节纳入 catalog digest；同一计算还纳入 `operation_catalog.rs` 和 `tool_catalog.rs`。新增解释注释会改变执行作用域/PreparedAction 看到的 catalog digest，违背本任务零行为变化要求。
- 新增 `docs/code-guide/capability-actions.md` 作为三份 digest-sensitive 源码的外部导读入口，详细说明固定操作目录、descriptor、PreparedAction、风险 floor、操作归一化顺序、两种动作摘要、catalog digest 与 handler result 归一化的调用者、数据流、失败边界和非授权属性。
- 调用关系核对：Runner 把固定模型工具映射为 CapabilityRequest 并计算最低风险；Core 清理调用方 authority 引用、盖章、归一化、运行 pre-tool hook、二次归一化、绑定 ExecutionScope，再构造 PreparedAction 和 pin authority；通过 Core policy/Gate/approval/permit 后 Broker 才执行 handler；结果在 Broker/Core 多个结束路径再次经 Domain `normalize_capability_result()` 后记录/交付。
- 为干净的 `kiana-domain/tests/cp03_action_contract.rs`、`cap02_input.rs` 和 Core `cp03_action_guard.rs`、`cap02_input_guard.rs` 补充文件级证明边界和每个测试场景说明。Domain fixture 直接构造合同值；Core guard 用 `include_str!` 查源码文本，不是实际运行时授权或 dispatch 证明。
- 发现项只记录未修：Runner 把 `call_id` 放进 `CapabilityRequest.arguments`，而 `canonical_action_input_digest()` 对完整 arguments 求摘要，所以它排除顶层 request_id，但不会排除这个嵌套 call_id；这需要与 baseline 对“correlation ID excluded”的宽泛描述一并复核。另 `normalize_capability_result()` 的 request_id mismatch 分支在后续 evidence_refs 数量/单项长度限制前复制引用并提前返回，存在该分支未经过本函数引用限额检查的边界。
- CAP-02 基线将功能记为 `partial/source`，远程回执绑定既定 source snapshot；本批没有运行测试，不将历史目标结果推定为当前工作树证明。
- 本批仅给四份当时干净的 Rust 测试文件增加注释，并新增外部导读/更新清单；`actions.rs` 及两个被其摘要读取的实现文件没有改字节。Rust diff 逐行检查只包含注释，`git diff --check` 通过。未运行测试、构建、格式化或提交。

## 2026-10-13 CAP-30 Domain 工具发现 reducer 注释批次

- `kiana-domain/src/tool_discovery.rs`：说明该文件属于 Domain 纯数据合同，不扫描扩展包、不做密码学验签、不连接 MCP、不调 Broker，也不执行工具。区分现有 `tool_catalog.rs` / Daemon `tool.search` 的 schema 搜索与本文件尚未接入生产的 admission reducer。
- 为 Origin、Trust、Health、Candidate、Plan、Denial、Status、Report 和临时 Decision 解释 DTO 字段来源、Serde/JSON 数据形状、摘要用途、返回状态、失败码、候选到 report 的预算筛选顺序与副作用边界。特别说明 report 只保留准入名称和 schema 字节/token 估值，不包含 schema 本体，也不授予 capability 或执行权。
- 为 `new()`、`validate()`、`digest()`、`evaluate()`、`validate_against()`、`derive()`、`first_violation()`、内建冲突 helper 和 manifest effect helper 补充调用者、输入、输出、错误传播和设计边界；解释 `std::mem::take`、Serde JSON 序列化、固定拒绝优先级、预算余量和 `#[allow(clippy::too_many_arguments)]` 的实际含义。
- 当前调用扫描只发现 Domain CAP-30 合同测试构造/调用这些合同 helper，没有生产代码构造 `ToolDiscoveryCandidate` 或调用 `ToolDiscoveryReport::evaluate()`。Domain 合同测试、Core extension guard 和 Broker discovery 文件已有其他 Codex 的 Git 改动，本批只读核对，不编辑它们。
- 发现项只记录未修：候选 trust/health/effect/origin/role 都是公开输入事实，reducer 不查询可信来源、tool-name-to-role allow-list、真实 project ID 或新 capability/operation 注册表；`ProjectMismatch` 条件对通过 `plan.validate()` 的 plan 不可达。重复检查只记住之前已准入项；同名拒绝映射可能互相覆盖；预算阶段再按名称查找原始列表第一项，前置的同名拒绝候选可能成为后续准入名的 schema 估值来源。预算准入多加 1 个 token margin，而 report 只记录 `ceil(bytes/4)`。
- 记录文档边界：CAP-30 baseline 的 server-owned/manifest-admission 表述是设计条件或依赖外部 adapter 的前提，不代表此 Domain reducer 已证明这些事实；`docs/roadmap/capability.md` 关于五工具“兼容基线、非永久上限”的路线图表述与仓库当前 AGENTS.md 锁定五个模型可见工具存在范围差异，未改两份文档或任何实现。
- 本批只修改此前由本任务注释的 Domain source 与 code-guide 清单/进度文档；源码执行语句、字段、签名、错误码、数值和调用顺序未改。未运行测试、构建、格式化或提交。

## 2026-10-13 SC-07 项目信任与部门快照注释批次

- `kiana-domain/src/trust_snapshots.rs`：说明项目 ProjectTrust 与部门角色清单两类快照在 Domain 的职责、Daemon 组装方式，以及 Core `SecurityAuthoritySnapshot::from_parts()` / `validate_request()` 的交叉校验位置。项目快照还作为 Domain storage preflight 和 deployment config 的输入合同；这些引用不代表单靠快照即可授权。
- 为字段解释项目 ID、canonical root digest、trust revision、trusted bool、source、修订号、部门 role IDs、authority epoch 与自摘要的业务含义；为 `from_project()`、两类 `from_json()` / `to_json()`、`validate()`、`digest()`、`contains_role()` 和 `validate_digest()` 说明调用者、错误传播、Serde 未知字段拒绝、摘要输入和副作用边界。
- 调用图已核对到 Daemon `context_from_assignment()`：它解析项目身份，调用 `project_trust_snapshot()`，由 assignment 的 department ID 查 `DepartmentSpec` 并生成 `DepartmentSnapshot`，再组合 Core authority snapshot 校验后返回 RequestContext。全仓没有找到 `context_from_assignment()` 的生产调用点，因此注释明确这是已实现的局部组合链，不宣称已接入产品入口。
- `kiana-domain/tests/sc07_trust_snapshots.rs`：说明 `/repo` 是纯字符串 fixture、信任值由测试直接传入、改 project ID 保留旧 digest 会被发现、未知 `raw_token` 由 Serde 拒绝、reviewer 不属于 executing 部门；第三项测试只解析内存 assignment directory，不持久化或调用身份服务。
- 发现项只记录未修：`ProjectTrustSnapshot::from_project()` 不调用 `ProjectIdentity::validate()`，也不绑定传入的 trusted bool 与 `project.trust_revision`；`validate()` 只校验摘要形状/自摘要，不重建 ID-root 关系或认证 source/revision。Daemon 当前组合方法先解析一次项目身份，随后快照 helper 又解析身份并单独查询 trust；跨多次读取不是原子观察，DTO 不能证明 trusted 与 revision 来自同一时点。`DepartmentSnapshot` 同样不向实时目录核对 revision/authority epoch。
- Core `sc07_authority_guard.rs`、`sc07_authority_snapshot.rs` 与 Domain `dep08_deployment_config.rs` 在开工前已有另一 Codex 的改动，本批只读，不编辑。仅修改此前干净的 Domain 快照源文件和 Domain SC-07 测试注释，以及来源清单/进度文档；未运行测试、构建、格式化或提交。

## 2026-10-13 H27 TurnOutcome 注释批次

- `kiana-domain/src/turn_outcome.rs`：补充 Runner `Completed` → Core `TurnOutcomeInput` → Domain `decide()` → Core outcome JSON annotation → `run.completed`/Receipt 的数据路径，解释 OutputContract 的顶层对象限制、required/property 关系、128 项上限、Serde JSON 限制、Unknown/取消/失败/等待/未结工作的固定优先级，以及终态/可恢复语义。
- 为 `OutputContract`、`TurnOutcomeInput`、`TurnOutcome` 字段和构造、校验、摘要、JSON type/ref/text helper 添加调用者、输入输出、稳定错误、副作用与非认证边界说明；注明 `ResultUnknown` 是 terminal 但不代表 reconcile 完成，失败/输出合同无效的 Err 与 Failed outcome 不是同一路径。
- 当前生产调用只发现 Core lifecycle 的 RunnerEvent::Completed 分支使用 `TurnOutcomeInput::completed()`。该 helper 将 output contract 设为 None、pending/取消/Unknown/failure flags 设为零/false、turn/step 和引用留空；因此 reducer 实现的可选输出合同与等待优先级目前由 Domain fixture 覆盖，不能据此宣称完成入口已采集这些事实。failure/Unknown 在 lifecycle 其他分支处理。
- `kiana-core/src/turn_outcome.rs`：说明 Core wrapper 的二次校验与 JSON 注入拒绝边界；`kiana-domain/tests/h27_turn_outcome.rs` 说明纯内存状态 fixture，`kiana-protocol/tests/h27_turn_outcome.rs` 说明 JSON wire 类型可见但不是协议 envelope 测试，`kiana-core/tests/h27_turn_outcome_guard.rs` 说明 `include_str!` 静态标记断言不运行生命周期。
- 发现项只记未修：`TurnOutcome::validate()` 无原始 output/contract body，不能重算 `output_digest`/`contract_digest`，也不重新归约或验证 blockers/reason 与 kind 的语义关系；摘要无密钥。Core 当前 `output_digest` 在加 `turn_outcome` 前、事件脱敏前计算，不是最终 EventLog payload 的字节摘要。H27 baseline/CURRENT_STATUS 将证明上限定为 source，远端聚焦夹具未等待；本批不运行测试。
- `kiana-core/src/lifecycle.rs` 当前已有另一 Codex 的工作区改动，本批仅只读核对其调用语句，不编辑；只修改当时干净的 Domain/Core/Protocol 目标源码和测试注释，以及来源清单/进度文档。

## 2026-10-13 P4-L3-01 版本治理注释批次

- `kiana-domain/src/versioning.rs`：补充 `RouteDecision` 字段来源与版本维度、兼容路径读取、缺省 `unknown`、route digest 派生、长度校验、version key 和 Serde 合同；解释这些标签用于历史指标归因，不是授权、签名或路由命令。
- 为 `DriftBucket` / `DriftReport` 的计数、EventId、固定 observed-turn/unknown-cost/禁止自动切换字段，以及构造、累计和整体校验函数说明调用关系、错误传播和边界。特别标出 `DriftBucket::record()` 多步 checked-add 不是原子更新：较后溢出会留下前序计数变化；报告仅检查桶内 EventId 去重，不验证跨桶全局唯一，也没有总量上限。
- `kiana-core/src/versioning.rs`：核对 `commands.rs` 对三个命令名的精确路由和 EventStore 端口语义；说明 `run.authorized` 以 actor 与规范化项目根构造 owned run 集合，再进入 drift、replay 或 capture。drift 从 committed `run.model_turn` 投影，不写事件、不调 ModelClient/Broker；replay 检查 trace 摘要与撤销后只折叠历史；capture 经项目 Artifact reader 建 manifest、排除 `run.snapshot`、构造 receipt 并用 request-local sequence 1 追加 `golden_trace.captured`。
- Runner `harness.rs` 只读核对 `run.model_turn` producer：错误结果会写非 null `error`，耗时写 `elapsed_ms`。Core 缺失/类型不符的耗时按 0 处理。`version.drift` 读取完整 EventStore 快照，因此存储不支持 `read_all()` 时会失败，时间和内存开销随日志增长。
- 发现项只记录、不修复：Core 为所有历史事件传当前编译包 `CARGO_PKG_VERSION`，并不从历史事件恢复生成事件时的 runtime version；capture 所有权根路径规范化，而 replay trace 查找对根路径字符串精确比较，路径别名可能查不到；函数路由以外的未知名字会落入最后的 capture 分支；Domain report 的 EventId 唯一性只在单桶内校验；计数溢出错误可能留下部分修改的临时桶。无密钥摘要/字段形状校验都不构成数据来源认证。
- `kiana-domain/tests/p4_l3_01_versioning.rs` 是内存 JSON/EventId fixture；`kiana-core/tests/p4_l3_01_versioning.rs` 是 `include_str!` 源码 marker 守卫。注释明确二者不启动 ControlPlane/EventStore、不调用 Provider，也不证明真实 route 元数据与请求一致。baseline/CURRENT_STATUS 仍将本切片 proof 记为 source，不推断 durable、online alert、自动切换或 live/physical 能力。
- 本批仅修改四份开工前干净的 Rust 文件注释及 code-guide 清单/进度；未修改命令路由、事件写入、字段、签名、错误码、数值或调用顺序。未运行测试、构建、格式化或提交。

## 2026-10-13 EQ-46 Drift metrics 注释批次

- `kiana-domain/src/quality_drift.rs`：解释 `DriftEvaluationInput` 的字段来源、调用方责任、`DriftThreshold` 的 minimum sample 与 per-mille/error/latency 限值，阈值摘要和字符串/摘要格式检查；逐字段说明版本桶转换出的 sample/error/elapsed/派生率，metrics、alert、event payload 的摘要和 validation 边界。
- 为 `evaluate_drift()` 补充输入拒绝、空样本失败、BTreeMap 顺序、阈值使用严格大于、NeedsReview/Alerted/Healthy 优先顺序、首个 breach 选桶和纯函数/append-ready 输出边界。说明 `DriftAlertEvent.validate()` 只验证 DTO/摘要，不能因此推断已持久化或已通知。
- `kiana-core/src/quality_drift.rs`：说明它只是公开 re-export 之下的 Domain 薄适配器，原样传递错误和结果；全仓调用搜索目前只找到 fixture 直接调用，没有生产命令或后台任务创建该输入。`DRIFT_ALERT_COMMAND` 常量的值实际是事件 kind `drift.alerted`，仓内没有与该名字对应的命令注册接线。
- `kiana-domain/tests/quality_drift.rs`：解释内存构造报告、阈值摘要、非零 source cursor fixture 及超限、样本不足、route/grant 前后不同、digest 和未知字段测试；这些不读取实时源或证明告警发生过。
- `kiana-core/tests/quality_drift_fixtures.rs`：逐 helper/test 说明来源值均为合成数据，以及拒绝路径、healthy/review/alert、双版本排序/重复、payload 篡改和主验收断言。`quality_drift_guard.rs` 则是 `include_str!` 文本 marker 黑名单，只守实现文件里的命名/边界字样，不运行 reducer 或产品入口。
- 发现项只记录、不修复：`evaluate_drift()` 在首个超阈值桶处 `break`，所以 BTreeMap 后续未遍历桶的样本不足不一定进入 `NeedsReview`，判定可能依赖版本 key 顺序；`DriftMetrics.validate()` 的 256 桶上限没有由 evaluator 在返回前执行，report 超过该数时可生成 validator 会拒绝的输出；threshold 的 route/grant 前后值只做输入字符串相等比较，source cursor/target 与 report 不从权威来源绑定；Alert validator 没有完整 threshold/metrics 输入，不能重算 breach，且 digest 是无密钥自摘要。极端大计数的 `saturating_mul(1000)` 也可能低估错误率。
- 本批仅修改此前干净的两份实现和三份 EQ-46 测试文件的注释，以及来源清单/进度文档；静态源码守卫检查的禁止字面标记没有写进实现注释。未运行测试、构建、格式化或提交。

## 2026-10-13 UI-05 Snapshot projector 注释批次

- `kiana-domain/src/ui_snapshot.rs`：用调用图说明 DaemonHost 单次 `persisted_events()` → Domain `project_ui_snapshot()` → `UiSnapshotPage` → Daemon 错误映射的路径；解释 Query 的 owner/session/epoch/source cursor/projection cursor/generation/data epoch/retention floor/page size/time 字段来自调用方，Domain 只校验合同，不替调用方读取身份、projection 或时钟权威。
- 为 PageCursor、Query、Page、Entry 和 entry kind 字段补充业务含义、Serde/query 作用、摘要输入与错误边界；说明 256 条 page 上限、4,096 条总条目上限、32 条 limitation、2,048 字节 cursor decode 限制、类别排序、按 `(kind,id)` 覆盖旧条目、action 最新状态折叠、retention floor 前 pending action 拒绝，以及单切片切页为什么要 cursor 对齐。
- 为 owner/session/source-position helper 和 projector 主流程说明事件字段兼容优先级、非 action 条目的 identifier 识别规则、事件 payload 保持原样、失败不返回部分页、next cursor 的 offset 语义。Domain/UI-05 测试以五条 RuntimeEvent 和合成 Query 验证排序分页、陈旧/篡改 cursor、projection lag/unknown、retention pending 与 foreign owner session；Core guard 是 `include_str!` 文本守卫，不运行 Daemon 接口。
- 调用核对：生产桥接方法 `DaemonHost::ui_snapshot_page()` 在 `kiana-daemon/src/lib.rs` 中先验证 Query，再单次读取 `persisted_events()`，unsupported 返回 `ui_snapshot_projection_unknown`，随后调用 Domain projector；全仓未找到该 facade 的下游生产调用者。该 Daemon 文件和 Protocol DTO 文件已有另一个 Codex 的改动，本批只读、不编辑。
- 发现项只记录、不修复：Domain 只按 `query.owner_id` 过滤，不认证 owner；当前 Daemon facade 将调用方 Query 直接交给 projector，方法本身未把 owner 与 `self.principal` 比较，外部入口必须先绑定身份后才能安全暴露。cursor digest 是无密钥 `json_digest`，但 UI-05 baseline 称它为 signed cursor；当前游标只覆盖 epoch/source/offset/generation，不覆盖 owner/session/page size/data epoch/retention floor，跨过滤条件复用会造成 offset 分页遗漏。`source_cursor == events.len()` 不校验每条 event sequence；`event_item()` 对 action kind 外的事件按 payload identifier 启发式识别，没有事件种类白名单；`projection_cursor > source_cursor` 分支已被 `query.validate()` 前置拒绝，故在当前调用顺序不可达。
- 本批只修改此前干净的 Domain projector、Domain UI-05 fixture 与 Core 静态守卫注释及 code-guide 清单/进度；Daemon/Protocol dirty files 保持未编辑。未运行测试、构建、格式化或提交。

## 2026-10-13 UI-04 Domain action journal 注释批次

- `kiana-domain/src/ui_action.rs`：将文件职责放入 UI intent → command → ControlPlane 快照/permit 对照 → Domain 纯内存 reducer → Core 原子事件写入 → 原幂等键查询的调用链。解释 command/record 字段、摘要覆盖范围、Serde schema、`Option` revision 的精确比较、deadline 和各状态含义。
- 为 `new()`、`validate()`、`digest()`、record 校验与摘要、单记录 transition、`accept()`、`apply()`、`reject()`、`unknown()`、`finish()` 和查询 helper 说明调用者、输入输出、失败路径、重放行为、内存副作用与容易误改的不变量。特别标明 Unknown 不等同于未执行、Applied 重放返回原 receipt、幂等接受先于新快照新鲜度检查，以及纯 Domain reducer 不执行目标 effect/持久化事件。
- `kiana-domain/tests/ui04_action_journal.rs`：解释固定身份/快照 fixture、相同 key 重放、payload 冲突、CAS 字段拒绝、ACK 丢失后的 Unknown 和 receipt-state 校验；明确这些测试只执行内存 reducer，不覆盖 EventStore/CAS、身份认证或真实 effect。
- Core facade、Daemon/Protocol bridge、source guard 和 `ui04-action-cas-baseline.md` 当前均有另一个 Codex 的工作区修改，本批只读核对，没有改动这些文件。静态 guard 对 Domain 源只要求合同标记存在，没有发现新增说明触及负向源码标记。
- 发现项只记录、不修复：permit digest 目前只做格式检查，未绑定到 command/journal digest；原 action 查询接口没有 owner/scope 参数，必须留在受控内部边界；不同 idempotency key 可以复用 command ID，`query_command()` 会按 key 字典序返回第一条；deadline 仅在 `now > deadline` 时过期，恰好等于 deadline 可接受，而已有 key 重放不重新检查过期；已 Applied 时新传入 receipt 不与原 receipt 比较。Domain 测试名提及 scope 冲突，但 fixture 未单独触发该分支，且没有 deadline 过期用例。
- 本批仅给此前干净的 Domain source/test 增加或改进注释，并更新来源清单/进度；Core 及其他 Codex 的 dirty 文件保持未编辑。未运行测试、构建、格式化或提交。

## 2026-10-13 CM-12 Unified retrieval 注释批次

- `kiana-domain/src/unified_retrieval.rs`：重写文件说明，按候选快照 → request/item 合同 → 精确词项与 BM25/dense 分路评分 → RRF 合并 → MMR 去冗余 → 可解释 result 说明数据流；明确排序只在已提供的候选中做相关性排列，不负责读权限、获取索引、生成 embedding 或执行副作用。
- 为 profile、request、item、hit、result 字段和 validator/digest 补充来源、用途、缺省值和错误边界；说明 Serde 的 JSON 编解码、`deny_unknown_fields`、optional vector、RRF k、BM25 参数、MMR lambda 和结果降级字段。为 `rank_retrieval()` 及两个数学 helper 解释输入限制、scope 检查、tokenization、允许项统计、词频与文档频率、向量维度/正分条件、稳定排序 tie-break、RRF 公式和 MMR 每轮更新方式。
- `kiana-query/src/unified_retrieval.rs`：说明 `UnifiedRetrievalPort` 固定 profile，`retrieve()` 将 Domain 的字符串错误映射为 `PortError::Failed`，便捷函数复用同一算法；adapter 不查询索引或补做 ACL。
- `kiana-domain/tests/cm12_unified_retrieval.rs`：说明伪摘要是重复 hex 的格式 fixture，不是真实内容 SHA-256；解释 denied 强匹配项、相同输入重复排序和 `acl_filtered=false` 拒绝测试，以及这些用例没有覆盖 dense 向量、融合排序、MMR 多样性和资源限制。
- `kiana-core/tests/cm12_unified_retrieval_guard.rs`：说明 `include_str!`/marker 断言是编译期源码文本检查，不运行检索、不证明产品主路径接线或权限来源。`kiana-query/tests/cm12_unified_retrieval.rs` 当前已有另一个 Codex 的修改，本批只读未编辑；其 fixture 比较便捷函数与 port adapter 的结果。
- 调用核对：Query crate 的 `rank_unified()` 和 `UnifiedRetrievalPort::retrieve()` 是 Domain 算法的薄转发；Domain golden-context 代码也按显式输入调用排序。全仓 Rust 调用搜索未找到产品主路径实际创建/调用该 Query adapter。CM-12 baseline 将独立的 Daemon ranker 和 CLI memory search 记为仍待迁移的兼容路径，因此不能把“同一算法可复用”写成“当前所有入口已统一”。
- 发现项只记录、不修复：`allowed`、`acl_filtered` 和 scope digest 是输入标签，不证明其从可信授权源产生；Denied item 在过滤出排名候选前仍会经字段校验和文本 tokenization，但不会进入文档统计/评分/命中。公开 request 可携带与 query 不一致但摘要自洽的 tokens；profile validator 允许非空的任意 algorithm_version 标签；摘要均无密钥，不能认证来源。Item 未限制正文/向量规模，也未检查 ID 唯一或来源摘要与正文绑定；Result validator 未校验 hit source digest 格式和 ID 唯一性。现有 Domain fixture 不覆盖 dense/RRF/MMR 多样性和这些边界。
- 本批仅修改此前干净的 Domain 算法/fixture、Query adapter、Core 静态守卫注释及 code-guide 清单/进度；另一个 Codex 已修改的 Query integration test、roadmap 和旧兼容实现均保持未编辑。未运行测试、构建、格式化或提交。

## 2026-10-13 BQ-02/03 Usage vector 与 accumulator 注释批次

- `kiana-domain/src/billing_usage.rs`：解释 `UsagePresence` 的 Absent/ExplicitZero/Present、所有用量维度与单位、UsageSource/Observation/Confidence 三组互不替代的枚举，以及 NormalizedUsage 中 usage/attempt/run、invocation/project/org、请求与服务模型、路由、retry 序号、basis/raw digest/usage digest 的业务来源。
- 为 `UsageVector::zero()`、schema validator、presence helper、`NormalizedUsage::new/validate/digest` 说明默认值和填充顺序、Delta 序号约束、confidence/reason 关系、摘要覆盖范围及错误边界。强调向量 `Default` 留下空 schema；None 不是零；改 public 字段后必须重算 usage digest；该摘要不是来源签名。
- `kiana-domain/src/billing_usage_accumulator.rs`：说明 attempt-local 内存状态、sequence→digest 去重表、Snapshot/Delta/Final 三种折叠方式、组件级单调包含关系、可选数值未知基线、checked arithmetic 及只有整条计算成功后才更新状态。
- `kiana-domain/tests/bq02_usage_vector.rs` 和 `bq03_usage_accumulator.rs`：为伪摘要/观测 helper 和各测试解释输入与断言；指出 BQ-02 只校验单条记录，BQ-03 fixture 每次新建 RunId、只以同一 AttemptId 累计，没有持久化/跨进程恢复或生产调用证明。两份 Core guard 解释 `include_str!` 的源码 marker/负向依赖断言边界。
- 调用核对：Provider 的两个归一化函数会构造 `NormalizedUsage`，文件/导出/fixture 可见，但本次 Rust 调用搜索未找到产品生产 caller；attempt lifecycle、cost 和 settlement Domain 合同可接收该结构。`UsageAccumulator` 的引用目前仅见 Domain 实现与 fixture，未发现 attempt EventLog 投影调用。旧 `kiana-domain/src/usage.rs::CostLedger/RuntimeBudget/ProjectBudget/Quota` 与 BQ-02/03 的 `billing_usage.rs` 不是同一套数据合同；旧文件尚未在本批编辑。
- 发现项只记录、不修复：`UsageVector::validate()` 只检查 schema；`NormalizedUsage::validate()` 不核验 confidence 是否与向量字段完整度相符，也不绑定 raw digest 的真实原文、不检查可选关联 ID 和 retry 序号的边界。Accumulator 只绑定 attempt，不绑定 run/provider/model；sequence 允许跳号；Final guard 在 seen 去重之前，所以相同 Final 重投递也返回 after-final；seen map 没有容量上限。fixture 未覆盖同一 attempt 跨不同 RunId、重复 Final、sequence 缺口和持久恢复。
- 本批只修改此前干净的两份 BQ Domain source、两份 Domain fixture、两份 Core 静态守卫注释和 code-guide 清单/进度；Provider normalizer、baseline 与其他 Codex 的工作区变动保持未编辑。基线的 proof_level 仍为 source；未运行测试、构建、格式化或提交。

## 2026-10-13 旧 Runtime usage 与预算合同注释批次

- `kiana-domain/src/usage.rs`：用文件级数据流区分从 RuntimeEvent 投影来的 `UsageRecord`/旧 `CostLedger` 与 Company policy 的 `RuntimeBudget`、`ProjectBudget`、`Quota`；明确它们不是 BQ-02 的 `NormalizedUsage`，也不是按 RateCard 逐笔计价的财务 CostLedger。
- 为 UsageRecord 和汇总字段说明 event/request/run/step/provider/model/token/耗时来源及 Option 含义；为 `CostLedger::from_records()` 解释输入/输出 token 分开折叠、任一记录缺值或 checked_add 溢出就让对应总数为 None、空记录不报零，以及 cost_micros 在版本化价格与完整 usage 前保持未知。
- 为三种预算对象及 validate/check_reservation 说明调用者、预留前的输入、运行/项目/scope 上限比较、加法溢出拒绝、恰好达到 token 上限可通过、以及这些纯合同不做原子占额或并发同步。
- 调用核对：Core `cost_ledger_from_events()` 从 attempted 的 `run.model_turn` 建 UsageRecord，随后调用本文件的 fold；Core Company/run budget 和 Domain Company/Swarm transition 读取 RuntimeBudget、ProjectBudget、Quota。相关 Core/Company 调用文件目前都有另一个 Codex 的 Git 改动，本批只读，没有编辑。
- 发现项只记录、不修复：Receipt 投影把缺失 step 默认成 0、把超出 u32 的 step 截到 u32 上限，并把缺失 elapsed_ms 默认成 0；对后两类数值，这会把“未提供”与“显式为 0”折叠成相同 UsageRecord。`ProjectBudget` 与 `Quota` helper 不储存或原子预留已用量，调用者必须在并发状态迁移中保证传入总额可信且占额无竞态。
- 本批只修改此前干净的 `kiana-domain/src/usage.rs` 中文注释及 code-guide 清单/进度；Core Receipt、Company、model budget 和 Domain Company/Model dirty files 保持未编辑。未运行测试、构建、格式化或提交。

## 2026-10-13 AUT-11 Workflow Event Ingress 注释批次

- `kiana-domain/src/workflow_event_ingress.rs`：说明外部 wire envelope → Domain 字段/JSON/摘要检查 → Daemon source policy 与 HMAC → 不含原始 payload 的 occurrence → `AutomationCommand::Fire` 和 Core 已提交 EventLog evidence 校验的数据流；解释 Serde、schema、source/key/project/trigger/event 标签、payload 与各层无密钥 digest 的语义边界。
- 为 ingress、policy、occurrence 的字段、构造/验证/摘要/转换函数补充调用方、输入输出、错误与安全边界；说明 64 KiB body、深度 8、每容器 256 项、4,096 字节字符串、各标识长度、24 小时时钟偏差上限和 512 字节 event reference。标记 `from_verified()` 的“verified”只是调用者前置条件，Domain 不验 HMAC，直接传入未验签 envelope 仍可构造结构有效 occurrence。
- `kiana-daemon/src/workflow_ingress.rs`：说明 server-owned policy/key map、`ring` HMAC-SHA256 解码/验证顺序、source+event 去重 key、clone 共享 `Arc<Mutex<...>>` 的并发临界区、PoisonError 恢复含义，以及 seen map 无容量上限、仅进程内、不跨重启的状态限制。仓内 `WorkflowEventVerifier` 目前只有实现、公开 re-export 与测试引用，未找到生产入口构造/调用点。
- `kiana-domain/tests/aut11_workflow_event_ingress.rs` 只覆盖合成 DTO 的字段筛选、摘要一致性、occurrence 和 Fire 形状；占位 signature 不会在 Domain 测试中验 HMAC。`kiana-daemon/tests/aut11_workflow_ingress.rs` 用固定合成 key 真实生成 HMAC，覆盖首次接受、同实例同摘要重放和若干拒绝输入，但拒绝断言只检查 `is_err()`，不证明具体错误分支；没有覆盖并发、跨进程重启或 durable dedupe。`kiana-core/tests/aut11_workflow_ingress_guard.rs` 仅用 `include_str!` 检查源码 marker/禁止标记，不运行 Core 路径。
- `kiana-ports/tests/int07_connector_ports.rs` 当前干净，仅在 AUT-11 的 `FakeWebhook` 子路径补充说明：它直接调用 Domain `from_verified()`，输入签名是普通占位文本；测试只证明 Port wrapper 形状与序列化结果不回显该文本，不能证明 Webhook HMAC 认证。仓内 `WebhookVerifier` 只有默认 unsupported 实现和 fake fixture，尚未找到生产 verifier adapter。
- 调用核对：Daemon verifier 返回 occurrence，不写 EventLog、不触发 Fire；上层必须先持久化事件，再用引用进入 Core evidence 检查。Port `VerifiedWebhook` 自摘要只证明 DTO 自洽，不是签名证明。Core/Ports 实现及 AUT-11 baseline 有另一个 Codex 的 Git 改动，本批只读没有编辑。
- 发现项只记录、不修复：`WorkflowEventSourcePolicy::validate()` 只比较 required/allowed key 数量，不检查 required 是否为 allowed 子集，因此互不相交的配置能构造但无法匹配事件；allowed payload key 集合没有数量上限。Daemon `seen` map 无上限且重启后清空。`WorkflowEventOccurrence::from_verified()` 是公开调用者前置 API，Port fake 已展示其可在无 HMAC 的情况下使用；下游必须以已提交 EventLog evidence 和受控 verifier 为信任来源，不能信任 wrapper 名称/摘要本身。静态守卫和 fixtures 不证明生产入口接线、跨重启去重或真实来源身份。
- 本批仅修改开工时 Git 干净的 Domain/Daemon 源码与 fixture、Core guard 注释、INT-07 fixture 的 Webhook 子路径及 code-guide 清单/进度；保持 Ports/Core 实现、DaemonHost 与 baseline 的其他 Codex 改动未编辑。未运行测试、构建、格式化或提交。

## 2026-10-13 AUT-07/08/09 + CP-24 Workflow Queue 注释批次

- `kiana-domain/src/workflow_queue_claim.rs`：说明 WorkPacket → readiness → Claim → Store Lease → Core gate 的合同位置；逐字段解释 parent/item scope、budget、path-lock digest、依赖/循环/active-count/duplicate 观察值、owner、deadline、status 与无密钥摘要。为 `from_work_packet()`、`validate_packet_binding()`、`new()`、`validate()`、`digest()` 说明真实调用、数据来源、拒绝路径与原子占额边界。特别说明 `active_claim_count == max_parallel_claims` 可通过且该 helper 本身不预留并发槽。
- `kiana-domain/src/workflow_queue_lease.rs`：解释 LeaseStatus 与 EffectState 两个状态维度、sequence/previous digest/fence/authority epoch/Unix 毫秒时间字段，Serde wire 边界，以及 issue/dispatch/renew/effect/fence/reclaim/actor/validator 的顺序和状态转换。说明 300,000ms（5 分钟）TTL、到 expiry 即过期、Unknown 不得重发、Store 必须原子提交、digest 链在 Domain 只校验形状而不读取历史。
- `kiana-core/src/workflow_queue.rs`：说明 Domain `ready_packets()` 投影、Claim 稳定排序、ReadyView 摘要、snapshot/packet/parent/claim/lease/owner/fence/epoch/deadline handoff gate 和 Unknown recovery 判定。标出 projection 固定 `active_claim_count=0`，所以 `max_parallel_claims` 在此只是数据字段，不执行真实容量限制；当前 projector/transaction 仅见源码与 fixture 调用，没有生产调用点。
- `kiana-daemon/src/workflow_service.rs`：解释 Tokio bounded mpsc（64 个待处理命令）、oneshot 回执、watch shutdown、Arc/AsyncMutex 共享生命周期、单 worker 顺序调用 Store、显式 tick 而无内置 timer，以及 active lease 内存索引的范围。为每个 command facade、请求回执、shutdown drain/fence/recovery report 和 worker handler 补充错误/副作用说明；标出 ack-lost 可能发生在 Store 已改状态后，以及 shutdown 先取走 running state 导致并发 start/shutdown 窗口。
- Fixture：`kiana-domain/tests/aut08_workflow_queue_lease.rs` 覆盖 owner/fence、Running 不可 reclaim、Unknown、单调 fence reclaim、TTL；`kiana-core/tests/aut07_workflow_queue_ready.rs` 覆盖 parent-child ready 顺序、dependency 阻塞与 scope/budget 扩大；`kiana-daemon/tests/aut09_workflow_service.rs` 使用 MemoryStore 覆盖显式 tick/claim/heartbeat/effect/shutdown 和 worker restart 后保留 Unknown。明确这些不是 durable store/OS 进程重启证明。`kiana-domain/tests/cp24_scheduler_transaction.rs` 与 `kiana-core/tests/cp24_scheduler_transaction.rs` 说明 WorkPacket rebind、digest/parent/scope/budget 拒绝和当前 lease gate 成功/失败 fixture；两份都不调用真实 QueueStore。
- `kiana-core/tests/aut08_workflow_queue_guard.rs` 的 `include_str!` 静态文本守卫已说明为源码 marker 检查，不运行租约或证明磁盘/跨进程能力。
- 调用核对：`WorkflowQueueStore` 当前 workspace 唯一实现是 `MemoryWorkflowQueueStore`；DaemonHost 只公开显式启动包装，仓内未找到生产入口调用，service operation 也只有 fixture 使用。`project_workflow_queue_ready()` 与 `validate_workflow_queue_transaction()` 仅被 tests 调用；transaction 仍是纯 gate，没有 QueueStore 原子 CAS。AUT-07 roadmap 行 276 仍标 `🔄`/partial/source；AUT-08/09 baseline 把 MemoryStore 与 service 记为 source/local-fixture，未声称 durable history、自动 scheduler tick 或外部 effect recovery。
- 发现项只记录、不修复：`validate_packet_binding()` 不单独比较 `self.item_id == packet.id`；Core gate 当前会要求 claim 与 ready snapshot 项完全相同，但 Domain helper 单独使用时不能保证该绑定。`record_effect()` 未比较新观察时间与上一 heartbeat，只由 validator 要求不早于 issued，因而可以接受 issued 之后但早于最近 heartbeat 的时间回退；`fence()` 保留原 expiry 又将 heartbeat 推至观察时间，观察时刻达到 expiry 时下一份 Lease 会因 `expiry <= heartbeat` 被拒，Daemon shutdown 将该 fence 错误计入 recovery-required。`renew()` 允许新 expiry 早于旧 expiry。Core ready projection 把 active count 固定为 0，不能用于真实并发容量裁决；Claim 的 digest 与 QueueStore/Lease 的 digest 都是无密钥摘要。
- 发现项只记录、不修复：`WorkflowQueueService::request()` 的发送/回执错误可能和 Store 已提交状态分离，收到 ack-lost 不能推出操作未发生；`shutdown_at()` 把 running 先从共享状态取走，另一个 handle 可在旧 worker drain 完前 start 新 worker，并发 shutdown 可观察到空状态而返回零报告。当前测试没有覆盖这些并发时序。
- 另一 Codex 正在改动的 `kiana-daemon/src/lib.rs`、`kiana-domain/tests/aut07_workflow_queue_claim.rs`、`kiana-core/tests/aut07_workflow_queue_claim_guard.rs`、`kiana-daemon/tests/aut09_workflow_service_guard.rs`、`kiana-core/tests/cp24_scheduler_transaction_guard.rs`、`kiana-eventlog/src/workflow_queue.rs` 和 `docs/roadmap/aut07-workflow-queue-claim-baseline.md` 均只读未编辑；清单已标出需在其稳定后重审的文件。
- 本批仅修改此前确认干净的 Domain/Core/Daemon queue 源码与相应 Domain/Core/Daemon/CP-24 behavior fixture 注释，以及 code-guide 清单/进度；没有改函数体、表达式、状态顺序、字段、类型、接口或依赖。未运行测试、构建、格式化或提交。

## 2026-10-13 PD-22/23 备份清单与恢复验证注释批次

- `kiana-domain/src/snapshot_manifest.rs`：说明备份元数据从 adapter 进入 `SnapshotManifest::new()`、排序和计算摘要、通过 `validate()` 后交由 Daemon 绑定恢复 fact 的预期数据流；当前未找到生产构造者。逐项解释 snapshot/owner/store/instance/root/cursor/epoch/mode/state/file seal/digest 字段，说明 Full/Incremental 前序规则、根路径的字符串比较、相对路径递增与摘要检查。明确 `size_bytes` 和 `content_hash` 是调用方声明，Domain 不读文件、不核验字节或签名。
- `kiana-domain/src/restore_verification.rs`：解释恢复 fact 的各身份、投影、epoch、计数、围栏、对账、状态和 digest 字段，以及 Blocked / NeedsReconciliation / Verified 的校验条件、固定错误码、文本与 digest 格式限制、无 I/O 和无密钥摘要边界。核对当前工作区引用：Domain fixture 和 Daemon verifier 有调用，未找到生产构造或消费该 fact 的路径。
- `kiana-daemon/src/restore_verifier.rs`：说明公开 `verify_restore()` 先校验 manifest、再透传 fact 校验错误，最后比较 snapshot digest、owner、store、previous instance、source cursor、previous data epoch 和新 data epoch；manifest 错误统一收敛为 `restore_manifest_invalid`。当前函数只有 fixture 调用，不复制/读取备份、不拿锁、不实际 fence、不写 EventLog、不激活 root。
- `kiana-domain/tests/pd22_snapshot_manifest.rs` 与 `kiana-daemon/tests/pd23_restore_verifier.rs`：标明 hash/path/instance 都是合成 fixture，不访问文件系统、存储、租约或外部 effect 系统。PD-23 测试改动 owner 后不重算 `verification_digest`，因此断言命中 Domain 自摘要无效路径，并未单独覆盖 `restore_manifest_fact_binding_invalid`。
- 发现项只记录、不修复：Daemon `verify_restore()` 没有要求 `fact.state == Verified`，所以结构有效且绑定一致的 Blocked 或 NeedsReconciliation fact 也可能返回 `Ok(())`；若上游把 Ok 直接解释为就绪/可激活，会越过状态门。当前未发现产品调用者，尚不能证明已造成运行时越权。Daemon 绑定检查不比较 `authority_epoch` 或 `previous_authority_epoch` 与 manifest（manifest 没有对应授权 epoch）；fact validator 只检查 fact 内的单调关系。测试 owner 漂移断言因旧 digest 未更新而先失败在自摘要校验，缺少重算摘要后验证 Daemon 绑定错误的 fixture。
- 另一 Codex 的 `kiana-domain/tests/restore_verification.rs` 和 `kiana-daemon/tests/pd23_restore_verifier_guard.rs` 在本批开始时已有 Git 改动，保持只读并在清单标记稳定后复审。baseline 仍将 PD-22/23 记为 source/partial；测试文件的注释不提升 proof level。
- 本批只改此前确认干净的 Domain/Daemon 实现与两个 fixture 的注释，以及 code-guide 清单/进度；没有修改字段、函数体表达式、签名、错误码、依赖或调用顺序。未运行测试、构建、格式化或提交。

## 2026-10-13 CM-13 RepoMap 任务视图注释批次

- `kiana-domain/src/repo_map_task.rs`：解释 `RepoMapSymbol`、`RepoMapDependencyEdge`、`RepoMapTaskCandidate`、`RepoMapTaskSelection` 的字段业务含义、schema/version/摘要、Heuristic 与 Declared 证据等级、错误码及数据流。说明 constructor 先逐候选验证，再按 task_score 降序、path/content_digest 升序稳定排序，按估算 token 贪心选择并记录 omitted，最终生成任务与选择摘要。
- `kiana-query/src/repo_map_task.rs`：说明它消费现有 RepoMap，不扫描文件；`memory_tokens()` 产生顺序词项，适配器将 path+symbols 作子串匹配并按词项顺序加权，把符号标为 Heuristic、构造空依赖边候选后交给 Domain。正常 RepoMap scanner 会提供源码摘要；没有 `sha256:` 前缀时才退化为 path+symbols 摘要，带此前缀但形状错误的摘要会由 Domain 拒绝。当前直接 caller 只有 Query fixture；`build_repo_map()` 的其他产品入口未使用这个 task-lens selector。
- `kiana-domain/tests/cm13_repo_map_task.rs`：说明候选/路径/digest 都是内存合成值，覆盖候选顺序不影响选择、预算遗漏和 Heuristic 标签；不运行 parser 或验证真实依赖。`kiana-query/tests/cm13_repo_map_task.rs` 手工构造单文件 RepoMap，仅验证 adapter 到 Domain 的转换和预算。`kiana-core/tests/cm13_repo_map_task_guard.rs` 用 `include_str!` 静态检查 marker 与禁止的执行边界，不运行 scanner 或 selector。
- 发现项只记录、不修复：`RepoMapTaskSelection::new()` 检查输入候选数最多 4,096 并产生稳定顺序，但 `validate()` 不复核反序列化报告的 selected+omitted 总数上限或 canonical 排序；因此不能将自摘要通过等同于资源有界或唯一规范表示。Domain 合同使用 `saturating_add()`，当 budget 为 `u64::MAX` 且累计 token 真实溢出时，饱和值仍可与预算相等。Domain `new()` 不拒绝空 task；空任务会使所有候选 score 为 0 并按 path/digest 排序。Query 的文本匹配是子串打分，不是语义或语法分析；task 选择只能在上游 RepoMap 已纳入的文件集合内选，无法恢复 scanner 阶段已因预算省略的文件。
- `kiana-query/src/lib.rs` 在本批开始前已有另一 Codex 的 Git 改动，只读确认模块注册与 re-export，没有编辑。CM-13 baseline/CURRENT_STATUS 的 proof ceiling 仍为 source/CI wiring，不因注释或历史 CI 记录升级。
- 本批只修改此前确认干净的 Domain/Query 源文件、Domain/Query fixture、Core 静态守卫的中文注释及 code-guide 清单/进度；未改候选算法、字段、接口、依赖或执行顺序。未运行测试、构建、格式化或提交。

## 2026-10-13 RepoMap 扫描器注释批次

- `kiana-query/src/repo_map.rs`：重写文件说明并沿真实调用点描述 Core 已授权的 Daemon `RepoMapHandler`、Direct Connect bearer 路由和 `context_manifest` 三类入口；区分 Direct Connect 不会因为复用此扫描器而自动经过 ControlPlane。说明路径收集/排序 → 文件读取与 UTF-8/NUL 筛选 → 符号、原始文本 SHA-256、字节数和估算 → 预算内 map 的数据流。
- 为 `RepoMapOptions`、`RepoMap`、`RepoMapFile` 解释预算、截断、遗漏、语言、文件 digest、serde default 与字段来源；为 build/collect/map/estimate/language/extract/各轻量语言识别器/ignore rules/wildcard/路径展示 helper 补充调用者、输入输出、错误传播、读取副作用、特殊数值和设计边界。
- 修正原有 `estimate_entry_tokens()` 注释把 `String::len()` 说成字符数的错误：实现实际按 UTF-8 字节数计算 `(path + language + symbols)/4 + 6`，下限 4。说明 map 的估算基数固定为 8，默认预算 4,000，每文件最多收录 12 个匹配名称；这些值都不是精确 tokenizer/计费数据。
- `kiana-query/src/repo_map.rs` 内嵌的两个测试和 `kiana-query/tests/p1_l4_01_code_intelligence.rs` 已标注 fixture 文件系统副作用、断言范围和静态 `include_str!` 守卫的证明上限；P1 guard 不运行 RepoMap/索引/HTTP handler。
- 发现项只记录、不修复：`max_tokens` 接受 1..=7，但结果估算固定从 8 开始，因此 `estimated_tokens` 可大于预算；无文件时 `truncated` 仍为 false。输出预算只截断结果，不限制预先收集路径、扫描文件数或 `fs::read()` 的单文件大小；超预算文件也会先完整读取。根 `.gitignore` 不支持否定规则、嵌套规则或完整 Git 语义；`src/generated/` 被按包含斜杠的目录名存储、后续却与单个 path component 比较，因而匹配不到该嵌套目录。语言扩展名匹配区分大小写，Rust/Python/JS/TS 识别是行前缀启发式、没有作用域/关键字边界检查，其他已识别语言只走 Markdown 标题提取。P1 静态守卫对 `canonical_root` 的同一 `contains` 断言重复一次，未增加额外覆盖。
- `kiana-daemon/src/context_query.rs`、`kiana-entrypoints/src/cli.rs` 和 `kiana-query/src/lib.rs` 在本批开始前已有其他 Codex 的 Git 改动；仅只读核对调用/导出，没有编辑。注释未改变 Direct Connect 授权实现、handler 数据流或扫描器行为；未运行测试、构建、格式化或提交。

## 2026-10-13 PD-20 上下文材料清单注释批次

- `kiana-query/src/context_manifest.rs`：补充模块位置与 RepoMap/artifact store → 组合清单 → JSON 原子写入/读回的数据流；解释 Serde 的双向转换、`deny_unknown_fields`、`source_cursor` 的调用方来源、工具版本字节上限、路径与 SHA-256 文本形状、自摘要排除自身，以及 build/validate/digest/read/write 的输入、输出、文件读取/写入副作用和失败传播。明确无密钥 digest 不是认证，也不会在读回时重扫源文件。
- `kiana-query/tests/pd20_context_material_manifest.rs`：说明两组 fixture 如何构造临时项目并覆盖根绑定、清单写读往返、文件摘要格式拒绝；标明不经过生产路由、不证明游标来源、不核对磁盘内容新鲜性或崩溃/多进程原子性。临时目录仅按进程 ID 命名，两个测试并行运行可能共用并互相删除同一目录，这是静态代码风险，本批未改测试逻辑。
- `kiana-query/tests/pd20_context_material_manifest_guard.rs`：说明 `include_str!` 与字符串 marker 断言只是静态源码约束，不证明代码可达、生产接入或运行行为。新增注释保留既有 marker 和禁止词检查所依赖的源码文本合同。
- 调用核对：当前仓内 `ContextMaterialManifest::build()`、读写函数的直接调用仅见 PD-20 fixture；`kiana-query/src/lib.rs` 有公开 re-export，但没有找到生产请求链路调用。外部 crate 仍可调用这些公开 API，仓内搜索不能证明全局无调用。
- 发现项只记录、不修复：`validate()` 绑定清单、RepoMap 和 artifact store 的顶层 root，却未单独核对 `artifact_store.dependency_graph.root`；也未验证依赖图节点/边引用、计数与角色汇总之间的一致性。它只检查列出的 RepoMap/artifact 文件摘要格式和 manifest 自摘要，不重算源文件内容 hash；即使重新计算无密钥 manifest digest，也不构成来源认证。构建 RepoMap 与 artifact store 时会分开读取项目，目录并发变化可能让两份派生报告来自不同时间点；顶层 root 相同不能消除该窗口。
- 本批开始前 `kiana-query/src/context_manifest.rs` 与两个 PD-20 fixture 为 Git 干净状态；`kiana-query/src/index_generation.rs` 已有另一 Codex 的未提交修改，只读参考其中 `atomic_write_json()`，没有编辑该文件。更新了代码导览清单与进度；只新增/改进中文注释和导览文档，未运行测试、构建、格式化或提交。

## 2026-10-13 PD-21 缓存策略注释批次

- `kiana-query/src/cache_policy.rs`：补充模块与真实调用位置、持久索引差异报告和治理传播计划两条输入路径；逐项说明缓存状态及序列化形式、reason/两个布尔字段/自摘要、`from_report()` 的状态映射、`validate()` 的拒绝条件、`from_data_propagation()` 的 epoch/Cache receipt/payload 三条件，以及方法不读取/清理/重建缓存的边界。
- `kiana-query/tests/pd21_cache_policy.rs`：解释 `created`、变化后的 `updated`、`recovered` 和有 unchanged 文件的 `updated` 代表哪些合成输入；明确仅测 DTO 映射，不读取缓存或运行治理构造器。
- `kiana-query/tests/pd21_cache_policy_guard.rs`、`kiana-daemon/tests/pd21_cache_policy_guard.rs`、`kiana-query/tests/sc24_query_data_boundary_guard.rs`：分别说明 `include_str!` 静态 marker/禁止词检查覆盖的源码范围，及其不能证明请求执行、统一调用链或生产撤销行为。
- 调用核对：`ContextCacheDecision::from_report()` 由 Query `index.rs::cache_report()` 调用后放入 `ContextIndexCacheReport.decision`；`from_data_propagation()` 当前未找到仓内调用。没有找到业务逻辑读取 `business_result_from_cache` 并据此返回结果的生产调用。
- 发现项只记录、不修复：`build_persistent_context_index()` 先完整调用 `build_context_index()` 扫描当前文件，再读取旧索引并按 path/hash 生成差异报告；它没有从旧索引恢复命中内容。因此 `from_report()` 把未变化文件计数映射成 `cache_read_used=true` / `business_result_from_cache=true`，与当前实际构建顺序不一致。空索引没有变化时 `reused==0`，会落入 `Stale/cache_source_changed`。`validate()` 允许 status 为 Hit 且两个缓存布尔值均 false 的自洽对象通过；也不限制 reason 长度或原因词集合。当前没有发现这些旗标影响生产业务结果，故记录为 DTO 合同/未来接线风险，不声称已发生越权或错误返回。
- 本批目标源码和 Query/Daemon guard 在开工时为干净状态；`kiana-core/tests/cm37_memory_maintenance_guard.rs` 与 `kiana-query/src/index_invalidation.rs` 已有另一 Codex 的未提交修改，仅只读确认关系，没有编辑。只修改中文注释与 code-guide 文档，没有改状态映射、字段、API 或调用顺序；未运行测试、构建、格式化或提交。

## 2026-10-13 H30 RepoMap 检索候选 adapter 注释批次

- `kiana-query/src/context_inputs.rs`：说明 RepoMapFile → SourceRef → SourceSnapshot → RetrievalCandidate → Domain ContextPlan 候选流程；解释输入字段来源、hash fallback、WorkspaceFile/RepoMap 类型绑定、Attributed 与 Current 的实际含义、固定 `relevance_milli=500`、`permission_scope + data_epoch` 的摘要用途、错误向上传播及无文件/网络/授权副作用。
- `kiana-query/tests/h30_retrieval_context.rs`：标明 RepoMapFile 和摘要均为合成 fixture，只验证候选合同字段，不运行扫描器、ContextPlan 或生产路由。
- `kiana-core/tests/cm35_external_resource_guard.rs` 与 `kiana-core/tests/h30_retrieval_guard.rs`：说明 include_str! 的 marker 可分散在多文件中、分别使用 OR/静态字符串检查，因此只能约束文本存在，不能证明 connector 执行、权限决策或完整生产调用链。
- 调用核对：`repo_map_candidate()` 在仓内只有 H30 fixture 直接调用，`kiana-query/src/lib.rs` 对外 re-export；没有找到当前生产入口调用。Domain `RetrievalCandidate::new()` 会进一步验证 source kind、摘要形状、候选字节上限与 evidence ref 合同，`ContextPlan` 仍由后续 Domain 流程构造。
- 发现项只记录、不修复：`repo_map_candidate()` 把 Freshness 固定为 Current，但不回看扫描后的文件；Evidence 固定为 Attributed 只标注来源，不构成真实性证明。若 RepoMapFile 无 `sha256:` 前缀，fallback 只摘要 path+symbols，不覆盖正文、语言或大小；本函数也不验证调用方传入的 path 仍位于 root 之下。`permission_scope` 与 epoch 的 JSON 摘要不是 ACL 检查或授权回执，不能被后续调用方当作访问许可。
- `kiana-core/tests/cm07_workspace_snapshot_guard.rs` 在本批开工前已有另一 Codex 的修改，虽也静态包含 `context_inputs.rs`，本批仅确认它仍检查 `SourceSnapshot` marker，没有编辑；稳定后应复审其注释是否覆盖本次 adapter 新边界。其余目标文件在开工时为干净状态。本批只新增中文注释与导览记录，未改变转换字段或函数接口；未运行测试、构建、格式化或提交。

## 2026-10-13 SC-24 Query 数据边界注释批次

- `kiana-query/src/data_boundary.rs`：说明治理布尔值/scope/epoch → Memory/Index/Cache/Export disposition → billing/audit 局部 gate 的数据流；为 enum、DTO 字段、Serde/拒绝未知字段、自摘要、`derive()`、`validate()`、`digest()` 补充语义、调用关系、错误和无副作用说明。明确构造器接收调用方已判定的治理输入，不查询账本；Allowed 不是签名、授权回执或来源证明。
- `kiana-query/tests/sc24_query_data_boundary.rs`：说明纯合成输入覆盖“允许且请求导出”与“撤销/不允许保留时拒绝所有派生视图”，摘要样例仅满足文本格式，不代表真实 scope。
- 调用核对：Query `billing_api::query_billing_projection()` 和 `audit_projector` 的访问推导会消费边界 DTO；当前仓内构造调用位于 SC-24、SC-32、BQ-23 fixtures，没有找到这些 Query API 的生产路由调用。`kiana-query/tests/sc24_query_data_boundary_guard.rs` 已在前一批注释，只检查静态 marker。
- 发现项只记录、不修复：`derive()` 检查 scope_digest 的 `sha256:` 形状，`validate()` 不重复检查；`validate()` 只强制 revoked/retention-blocked 时全部拒绝，不重算未撤销情况下 memory/index/cache 与 export 的完整映射，因此带正确自摘要的手工构造 DTO 可表达非 canonical 的 disposition 组合。项目根只检查非空，不规范化或绑定真实项目路径。无密钥 digest 不能证明边界来自可信治理 authority；当前也没有生产路由构造/消费链可据此断言实际越权或拒绝行为。
- `kiana-query/src/audit_projector.rs` 与 `kiana-query/tests/sc32_audit_projector.rs` 当时已有另一 Codex 的未提交修改，仅只读核对其 boundary 消费调用，没有编辑。`kiana-query/src/data_boundary.rs` 和 SC-24 行为 fixture 开工时为干净；本批只增加中文注释和导览记录，未改状态推导、字段、签名或调用顺序；未运行测试、构建、格式化或提交。

## 2026-10-13 INT-09/17 连接器只读投影注释批次

- `kiana-query/src/connector_health.rs`：解释 Daemon 已提交 binding/health 事件 → Query 单 binding 最新事实 → binding revision stale 标记 → Daemon registry/result 的调用链；逐字段说明 cursor、revision、可选 credential generation/data epoch、固定 `source` proof 和 limitations。为 entry/projection 校验及 event projection 流程说明 unknown event 处理、错误传播、固定 256/16 上限、空流 cursor 占位值和纯只读边界。
- `kiana-query/src/connector_quota.rs`：解释 reservation/claimed/settled/released 事件如何变成按 ID 排序的当前状态，settled 事实如何绑定 reservation，source cursor 如何聚合；说明 512 条输出上限、Unknown/released 保留、proof_level/replayable 含义以及不计算容量、不授权、不写 Store。
- `kiana-query/tests/int09_connector_health.rs`：说明合成事件 helper、健康 revision drift 和孤立/畸形 fact fixture；这些测试数据不来自真实 adapter/EventStore，不证明 live health。
- `kiana-query/tests/int17_connector_quota.rs`：说明 `include_str!` 只做源码 marker 和禁止词检查，不执行 RuntimeEvent 重放或产品路由。
- 调用核对：Daemon `connectors.rs` 的健康探测路径在 append health event 后调用 `project_connector_health()`；列表路径从 stream history 投影健康视图。`project_connector_quota()` 仓内当前没有找到直接 caller；INT-17 Query 文件是静态守卫而非行为测试。
- 发现项只记录、不修复：两个 projector 对同 ID 的覆盖都依赖输入切片顺序，不显式选择最大 cursor；调用方应保持 EventStore stream 顺序，乱序会使后来遍历到的旧事实覆盖较新事实。健康 stale 只比较 binding revision，不根据健康检查时间或凭证代次判过期；全输入没有正 cursor 时外层 cursor 被设为 1，entry 自身仍需真实非零 cursor。健康 handler 先持久化事件再投影，投影错误时请求可能失败但事件已经追加。Quota projector 的输入事件数没有预先限制，512 上限只在输出 validate 时检查；projection validator 不限制 limitations 总数，也不绑定 entry cursor 与外层 cursor。上述均为源码边界观察，不是本批执行验证。
- 两个 Query 源文件和两个 Query fixture 在开工时为干净状态；只补充中文注释与代码导览，没有改业务语句或字段。未运行测试、构建、格式化或提交。

## 2026-10-13 BQ-19 Allocation Query 投影注释批次

- `kiana-query/src/allocation_projector.rs`：补充 BQ-19 文件职责、Run allocation 事件到 Query projection 的校验/汇总路径；解释 Error 分类、Estimated/Measured/Unknown 分槽、Money 同币种与 checked-add 溢出、usage leaf 去重、事件 envelope 身份绑定、source sequence 顺序、五种 totals 的平行视图及唯一 ledger total。为 projection validator、维度 getter、单数委托别名、只读静态标记和未调用的 ID 类型 helper 加注释。
- `kiana-query/tests/bq19_allocation_projector.rs`：说明合成 CostAllocation/RuntimeEvent helpers，覆盖同一 leaf 在 run 与各维度分别汇总、重复 usage 和跨 run 拒绝、estimated/measured/unknown 分离以及相同输入确定性；不连接 EventStore 或验证账本持久性。
- `kiana-query/tests/bq19_allocation_projector_guard.rs`：解释 include_str! 对维度 marker 和禁止词的单文件文本检查，不运行投影或替代 workspace 调用图审计。
- 调用核对：`project_cost_allocations()` / singular alias 只有 Query fixture 和 crate re-export，未找到 Core、Daemon、entrypoint 的生产调用。
- 发现项只记录、不修复：`CostAllocationProjection::validate()` 会验证 allocation run/usage 唯一性和 totals 各自合法，但不从 `allocations` 重算并对比组织/项目/workflow/cell/run totals，也不检查 `source_event_ids` 本身唯一；builder 路径有更强的输入事件去重与现场累加，因此主要是 public mutable projection 被手工构造后，单独 validate 不足以证明汇总与明细绑定。当前未发现生产调用，不能据此声称已发生账目错误。
- 本批三个目标文件在开工时为 Git 干净状态；只增补中文注释与导览记录，没有修改金额计算、错误分支、字段或接口；未运行测试、构建、格式化或提交。

## 2026-10-13 BQ-13 Receipt Cost Query 投影注释批次

- `kiana-query/src/cost_projector.rs`：说明调用方 RuntimeEvent → Domain `project_receipt_cost_breakdown()` → Query envelope 的职责边界；解释 nil Run、空来源与 Unknown 的区别、Error 分类、外层 Run/cursor/event IDs 与 breakdown 一致性、Unknown 原因借用接口，以及只读无副作用行为。强调成本状态迁移和金额计算由 Domain 负责，Query 不读取 provider 价格、不写账本。
- `kiana-query/tests/bq13_cost_projector.rs`：解释临时 RateCard、已知 usage、合成 hash 和成本事件 fixture；覆盖 estimate 保留、source cursor、空来源不合成零金额，以及源码静态守卫的范围。
- 调用核对：Query wrapper 当前只有 BQ-13 fixture 直接调用，crate 根模块有 re-export；未找到 Core/Daemon/entrypoint 生产调用。底层 reducer 位于 Domain，并负责按 attempt 校验 cost kind、revision 连续性、状态迁移、usage digest 和总额。
- 本批源码和 fixture 在开工时为 Git 干净状态；只新增中文注释与代码导览记录，没有改函数签名、事件筛选或 reducer 行为；未运行测试、构建、格式化或提交。

## 2026-10-13 BQ-14 Append-only Cost Ledger Query 投影注释批次

- `kiana-query/src/cost_ledger_projector.rs`：解释 committed ledger entry/correction 事件 → 事件身份与顺序检查 → 内存 `CostCorrectionLedger` → 每条原 entry 的 `CostLedgerView` 数据流。说明 correction 事实还原成 reducer command 是适配其 append API，审批字段来自既有不可变事件，Domain reducer 仍执行目标/审批/幂等校验；Query 不创建新审批、不写 EventLog、不修改原始 entry。
- `kiana-query/tests/bq14_cost_ledger_projector.rs`：说明合成原始 entry、draft/approval/correction、事件顺序和重复来源/foreign Run fixture；结果中的 900 估算值减 100 后只影响 view，原 entry digest 保持原样。它不连接真实 EventStore 或审批服务。
- `kiana-query/tests/bq14_cost_ledger_projector_guard.rs`：说明 `include_str!` 对 `append_correction`/cursor 与禁止词的单文件静态检查不运行 ledger reducer。
- 调用核对：`project_cost_ledger()` 当前只有 Query fixture 直接调用，crate 根模块公开导出，未发现生产 route caller。原始 reducer 位于 Domain `CostCorrectionLedger`，其 `append_correction()` 检查 target、批准状态、command/idempotency 重放及 correction ID。
- 发现项只记录、不修复：entry envelope 的 stream version 与 entry revision 精确比较；correction envelope 当前只要求 stream version 存在且非零，没有在 Query projector 中显式校验 correction event 的 aggregate stream version 连续递增。总体 `event.sequence` 会对所选 ledger 事件严格递增，但不等价于 per-entry aggregate stream version 验证。外层 `CostLedgerProjection::validate()` 也不重放 ledger、不校验来源 ID 列表与内部事实数量/内容一一对应；builder 路径进行更强的转换校验。当前未找到生产调用，不能据此断言已有账本事实损坏。
- 本批目标源码与两个 BQ-14 fixture 在开工时为干净状态；只新增中文注释/导览，没有改 reducer、事件顺序、校验条件或接口；未运行测试、构建、格式化或提交。

## 2026-10-13 BQ-23 Billing Query API 注释批次

- `kiana-query/src/billing_api.rs`：说明 request → snapshot → boundary 的有序验证、digest/disposition 拒绝、source/after/projection cursor 检查、freshness、四类 summary 映射、row/quarantine 计数和 Domain response digest；解释错误分类与无写入副作用，标明 `limit` 只参与摘要响应的 cursor fence，不会在本 adapter 切分明细行。
- `kiana-query/tests/bq23_billing_api.rs`：说明空的合成 snapshot、request/boundary helpers，覆盖 cursor/freshness、显式 export gate 和 projection digest mismatch；不连接 EventLog 或产品 route。
- `kiana-query/tests/bq23_billing_api_guard.rs`：说明 `include_str!` marker/禁止词仅检查单一 API 源文件，不执行 Query 流程或证明路由接线。
- 调用核对：`query_billing_projection()` 当前仓内调用仅在 BQ-23 fixture，`kiana-query/src/lib.rs` 公开 re-export；未找到生产路由 caller。输入 snapshot 在 Domain 代表完整 billing read model，含 usage/ledger/allocation rollups 和 projection source fence。
- 发现项只记录、不修复：API 将 `request.project_id` 放入响应，但没有从 snapshot 按项目筛选或比较项目身份；`BillingProjectionSnapshot` 本身不含 project ID，因此 caller 必须提供已按该请求范围准备的可信 snapshot。边界只做结构/自摘要验证，不证明 boundary 的可信来源。incoming cursor 的 source_cursor 在 Domain request 中与请求互绑，API 再比较 cursor.projection_digest 与 snapshot digest，但没有直接核对 cursor.source_cursor 是否等于 snapshot 当前 source cursor；自洽但人为构造的旧 source cursor + 当前 projection digest 可能得到非直观 freshness。当前仓内没有生产 API caller，不能据此断言发生跨项目泄露。
- 本批三个目标文件在开工时 Git 干净；仅添加中文注释和导览说明，没有修改校验顺序、summary 选择、cursor 算法或字段；未运行测试、构建、格式化或提交。

## 2026-10-13 BQ-20 Billing Ledger Query 投影注释批次

- `kiana-query/src/billing_ledger_projector.rs`：补充模块级职责与“已提交事件历史 → 连续页/全量重放 → Domain 快照 → 查询适配器”的数据流；说明 Query 层只折叠给定历史，仓内直接调用当前仅见于 BQ-20 测试，crate re-export 不代表产品接线。
- 为错误 enum、来源事件配对类型、内部事实索引、状态型 projector 和每个重要方法补充中文说明；解释 epoch/全局 cursor、事件 ID 去重、候选副本整页发布、账单事实隔离、usage revision 转移、账本/修正目标绑定、allocation 去重、快照全量重算、Unknown 与零金额区别、UTC 日/小时分桶及无密钥 replay fence。
- `kiana-query/tests/bq20_ledger_projector_guard.rs`：说明 `include_str!` 只检查目标文件的必要 marker 和禁止依赖文本，不解析 Rust、不重放事件，也不证明产品路由调用。
- 行为测试 `kiana-query/tests/bq20_ledger_projector.rs` 开工时已由另一 Codex 修改；本批只读其 fixture 和断言，没有编辑，待该写入稳定后再按最新源码补注释。
- 调用核对：`project_billing_ledger()` 仓内直接调用只见测试，根模块 re-export；`BillingSourceEvent` 与 `billing_source_is_rebuildable()` 目前只见定义/re-export，未发现生产消费者。
- 发现项只记录、不修复：修正事件 envelope 的 stream version 只要求非零，未显式核对同一聚合版本连续性；usage revision 用 `saturating_add(1)`，在前值为 `u64::MAX` 时可能接受同号后继；`UsageFact`/`LedgerFact`/`AllocationFact`/`CorrectionFact` 保存的来源 cursor 当前不参与 rollup 重算；`BillingSourceEvent` 目前无仓内调用者。`billing_source_is_rebuildable()` 仅验证健康结构和正数 durable cursor，不证明账单历史完整或实际可重建。以上为源码观察，未证明存在生产故障。
- projector 源码和静态 guard 在开工时干净；本批仅增加中文注释及导览记录，没有改折叠、校验、汇总、字段、接口或调用顺序。未运行测试、构建、格式化或提交。

## 2026-10-13 Query Runtime Config 注释批次

- `kiana-query/src/config.rs`：说明环境变量 → `QueryConfig` / `QueryGates` → `transition_query_state()` → 状态与 effect 的数据流；分别解释四个 bool 的 true/false 含义、环境变量名称、缺省值、大小写规则、精确匹配和无 trim 行为。
- 解释 serde `Serialize` / `Deserialize` 派生只提供数据转换能力、不自动执行 I/O；配置快照不包含授权、ProjectTrust 或工具执行结果，环境变量不能替代控制面授权。
- 调用核对：`QueryConfig` 被 transitions API 与其测试引用、并在 `kiana-query` 根重导出；全仓搜索未找到 `QueryConfig::from_env()` 产品调用。`transition_query_state()` 当前只读取 session ID，未消费四个 gates。
- 发现项只记录、不修复：环境开关目前没有在 Query reducer 中形成行为分支，且环境快照构造方法未发现生产调用；因此配置名称/真值不能作为功能已接线的证据。此项是实现接线观察，不据此判定产品故障。
- `config.rs` 开工前为 Git 干净；只增加中文注释，没有改变量、默认值、环境读取、接口或状态转移。未运行测试、构建、格式化或提交。

## 2026-10-13 Query Dependency Container 注释批次

- `kiana-query/src/deps.rs`：说明 QueryDeps 的窄职责、预期组合层到 UUID 消费者的数据流，以及生产随机 v4 实现和可替换工厂的区别；逐项解释 `Arc` 共享所有权、`dyn Fn` trait object、`Send + Sync` 线程边界、panic 传播、闭包副作用和候选 ID 不等于持久化事实。
- 为 `production()`、`new_uuid()`、`Default` 和手写 `Debug` 添加调用/输入/输出/副作用说明；没有虚构 Serde 能力，该类型实际没有 Serde derive。
- 调用核对：仓内搜索只找到 QueryDeps 定义与 `kiana-query` crate root re-export，没有生产代码或测试消费者。
- 发现项只记录、不修复：这是尚未接入当前仓内查询调用图的公开依赖容器，不能仅凭其默认 UUID 工厂存在就认为查询逻辑已改为依赖注入。
- `deps.rs` 开工前为 Git 干净；只补充注释，没有改字段、trait bound、闭包、返回值或默认实现；未运行测试、构建、格式化或提交。

## 2026-10-13 Query Token Budget 注释批次

- `kiana-query/src/token_budget.rs`：说明累计 token → checker → `TokenBudgetDecision` → transitions reducer → 有序 QueryEffect 的预期链路，并标明仓内直接 checker 调用目前只有本文件单测，尚未找到产品循环调用。
- 解释 90% 严格比较、展示百分比四舍五入、最近两个增量各低于 500 token 且至少续跑 3 次的收益递减条件；说明 budget 缺失/为零和子代理的快速停止、tracker 仅在 Continue 更新、恢复后 Instant 耗时为 0、提示文本不是硬配额。
- 为 tracker、completion 摘要、tagged Serde decision、纯函数和每个内联测试补充面向初学者的职责/字段/边界说明。
- 发现项只记录、不修复：该模块只返回建议，不限制模型实际 token；仓内没有生产调用 `check_token_budget()` 的证据。累计 token 必须来自同一会话；输入倒退会由 saturating subtraction 视为 0 增长，可能影响收益递减判定。
- `token_budget.rs` 开工前为 Git 干净；只增改中文注释，没有改阈值、分支、计数、消息文本、序列化字段或状态更新；未运行测试、构建、格式化或提交。

## 2026-10-13 Query State Reducer 注释批次

- `kiana-query/src/transitions.rs`：补充 reducer 作为“状态 + 事件 → 新状态 + 有序 effect”的文件说明，区分局部 QueryState 与 EventLog 权威状态，并逐项解释阶段、事件、停止原因、effect 和 transition 输出。
- 解释 Serde tagged enum、phase/event 白名单、终态优先、token 累计饱和加法、工具 pending 数量、停止钩子错误与 prevent flag 的优先级、预算 Continue 的消息/模型 effect 顺序、Fail/Abort 展示字段及无自动重试/无 I/O 边界；为内联单测逐项说明覆盖场景。
- 调用核对：`transition_query_state()` 的直接调用当前只有文件内单测，crate 根重导出；没有找到生产执行循环调用。QueryConfig 只读取 session ID，四个 gate 没有参与 reducer 分支。
- 发现项只记录、不修复：`ToolResultsSubmitted` 只收计数而无 tool-use ID，且 `saturating_sub` 会把超额/重复数量压成 0 并继续请求模型；预算 Stop 的 completion_event 统计在 reducer 中被丢弃，仅保留是否存在以映射停止原因；Complete 可携带任何 QueryStopReason 并直接设 phase=Completed；hook 同时有 blocking_errors 和 prevent_continuation 时前者优先。以上是当前局部 API 行为，未证明产品调用已发生。
- `transitions.rs` 开工前为 Git 干净；只增加/改写中文注释，没有改状态分支、phase 规则、计数、effect 内容/顺序或字段；未运行测试、构建、格式化或提交。

## 2026-10-13 Hook Cancellation Helper 注释批次

- `kiana-query/src/hook_cancellation.rs`：说明预期的 run/hook/invocation 身份 → 递归 parent/visited 检查 → 外层观察取消 → 终态/observer 重试建议链路；解释 HookRunRole、HookRunTerminal、公开身份字段、Serde unknown-field 边界和摘要不等于认证。
- 为 `prepare_hook_invocation()`、`cancellation_terminal()`、`retry_allowed()` 解释输入、输出、拒绝原因、深度 0..8、饱和递增、BTreeSet 副本、键格式和“建议不执行”边界。
- `kiana-core/tests/ext17_hook_cancellation_guard.rs`：说明 `include_str!` 只检查单一 Query 源文件中的关键文本 marker，不调用 helper 或证明产品路由接线。
- 调用核对：仓内只发现定义、Query crate root 重导出和 Core 静态 guard，没有生产消费者。
- 发现项只记录、不修复：ID 校验允许冒号，但递归/幂等键使用未转义冒号拼接，不同三元组可能碰撞；`parent` 的 schema/digest/depth/visited 大小不校验且 visited 会整体复制；`identity_digest` 是空 digest 字段参与 JSON 后的无密钥摘要，没有验证函数；`cancellation_terminal()` 除 stop_confirmed=true 外一律返回 Unknown，role 对结果无影响；observer Unknown 的 retry helper 不接收幂等键，也不执行重试。当前没有生产消费者，不能据此声称存在运行中的重试问题。
- 源码和静态 guard 开工时干净；只增加中文注释，没有改 ID 规则、字符串键、递归边界、返回分支或调用行为；未运行测试、构建、格式化或提交。

## 2026-10-13 Hook Lifecycle / Outcome / Receipt 注释批次

- `kiana-query/src/hook_lifecycle_dispatch.rs`：解释 lifecycle kind 到身份绑定事件 DTO 的预期数据流、PostTool result_committed 前置条件、payload/event digest、固定 capability false，以及 dispatcher 不发事件、不跟踪跨调用 sequence 的边界。
- `kiana-query/src/hook_outcome.rs`：说明 JSON object、字段 allowlist、decision 映射、字段别名、Ask 审批引用、UpdateInput 重新授权要求、context/patch/time bounds、Serde tagged enum 和错误传播；注明它当前没有 `stop_hooks`/产品调用者。
- `kiana-query/src/hook_receipt.rs`：解释 receipt 字段来源、schema/digest 前缀检查、cleanup/outcome Unknown fence、控制面决策标记以及固定 `executed=false`；说明 replay view 不运行 hook。
- `kiana-core/tests/ext14_hook_outcome_guard.rs`、`ext16_hook_lifecycle_guard.rs`、`ext18_hook_receipt_guard.rs`：分别说明它们只通过 `include_str!` 检查单文件 marker/禁止文本，不执行解析、dispatch 或 replay。
- 调用核对：三个 Query helper 当前只见定义、crate 根重导出和 Core 静态 guard，没有生产消费者；其中 outcome parser 也未被 `stop_hooks.rs` 调用。
- 发现项只记录、不修复：`parse_hook_outcome()` 将缺失或非字符串 decision 变为 None，并在没有 `unknown` 键时默认 Allow，空对象和仅含 cancelled 标志的对象都可通过；AdditionalContext 会静默丢弃非字符串元素；raw 文本在本 parser 没有字节上限。此行为与 `docs/roadmap/skills-plugins-hooks.md` 描述的 strict/fail-closed stdout adapter 存在张力，文档为规范目标，未在本注释任务中修改实现或规范。
- lifecycle helper 只检查非空 ID 和正 sequence，不维护跨事件单调顺序；PostTool committed 是调用方声明；摘要没有 verifier。receipt helper 只检查 digest 前缀、不检查完整格式或 receipt_digest，且 `hook_receipt_digest()` 把已有 receipt_digest 一并哈希；`replay_safe=true` 当前反而被 replay helper 拒绝。均无仓内生产调用，不能据此断言已有线上故障。
- 六个目标文件开工时 Git 干净；本批只加中文注释，未改 parser 分支、校验阈值、DTO 字段、digest 算法或 guard 断言；未运行测试、构建、格式化或提交。

## 2026-10-13 Legacy Hook Process Adapter 注释批次

- `kiana-query/src/stop_hooks.rs`：补充文件职责、兼容层调用图、Hook DTO 和决定的语义、各公开入口的调用者/输入/输出/副作用/失败映射；说明 Stop 事件的有界 `mpsc`、最终 `oneshot`、取消通知、配置/插件/trust 来源、事件专属 JSON 字段、shell 环境构造、64 KiB 输出保留、timeout 与宽松 stdout parser。
- 复杂流程说明包括：同组 hook 的顺序与错误优先级、配置错误/单条运行错误后仍可能继续执行后续命令、PreToolUse 输入更新如何传给后续命令、静默执行通过 drain 消费有界事件流、上下文 hook 的严格错误聚合及提示更新传递、Stop 成功后才进入当前 TeammateIdle 分支。
- `kiana-core/tests/ext13_hook_process_guard.rs`：说明 `include_str!` 将 Query 源码作为文本做 marker/禁止模式断言；它不启动子进程，也不证明 timeout 覆盖 stdin 写入、stdout 峰值内存有界或进程后代退出。
- 调用核对：`kiana-tools/src/tool_execution.rs` 的普通兼容工具路径在 PreToolUse 后验证 hook 更新后的参数并继续权限检查；PostToolUse 在工具调用之后执行。预批准并发 helper 会验证 hook 更新的输入，但其函数体未见同一处重新执行 `permission_check_for_tool`，需结合预批准来源进一步审查。`run_session_start_hooks` / `run_user_prompt_submit_hooks` 仅由本文件测试直接调用，entrypoint 包装 helper 标为 `cfg(test)`；`handle_stop_hooks` 和通知 helper 当前未找到产品调用者。`kiana-query::hook_outcome::parse_hook_outcome` 未接入本文件 shell stdout 路径。
- 数据流：调用方的工具/提示/信任上下文 → 选择环境变量、用户/项目配置或插件命令 → 事件上下文 JSON 写入子进程 stdin → 读取退出码与 stdout → `parse_hook_output` 解析决定/上下文/更新输入 → 映射为工具决定、上下文结果或 Stop 事件与结果；该路径本身不写 EventLog，也不执行 ControlPlane 授权。
- 发现项只记录、不修复：1) 无效 JSON 或非 object stdout 返回默认 clean 解析结果，continue 缺失/非布尔也默认为 true，可能让错误脚本输出表现为无决定；2) `write_all(stdin)` 在 timeout/取消 select 之外；3) `wait_with_output()` 先完整收集 stdout/stderr，64 KiB 限制仅作用于随后保留的文本；4) Unix 创建 process group，但代码未显式终止/确认整个后代组；5) 任意正 `KIANA_HOOK_TIMEOUT_MS` 都接受，没有最大上限；6) `KIANA_HOOKS_FILE` 直接返回路径，绕过自动项目候选路径的 ProjectTrust 过滤；7) 预批准工具调用在 hook 更新参数后未见同一函数内再次权限检查；8) 配置/运行错误可能与有效命令并存，执行器报告错误后仍运行这些有效命令；9) 事件发送失败被忽略，接收端关闭不取消执行；10) 上下文 hook 忽略 `continue=false`，且不使用 warning 非零策略；11) PostToolUse Block 无法回滚已发生的工具副作用。以上是静态实现边界，不表示已经发生产品故障。
- 测试夹具注释说明环境变量锁、临时配置、trust 矩阵和本地 shell 用例的用途；局部测试不证明 ControlPlane、EventLog、生产接线或进程树清理。另一 Codex 的 dirty 测试文件未修改。
- 两个目标源码文件在本批开始前 Git 干净；只增加中文注释/测试导航与本台账记录，没有修改 executable line、字段、签名、环境变量解析分支、事件顺序、超时范围或输出算法。未运行测试、构建、格式化或提交。

## 2026-10-13 ExecutionScope Domain 合同注释批次

- `kiana-domain/src/execution_scope.rs`：补充 Domain 值对象在 Core 构造、Domain 自检、策略/审批和 Broker 派发前重新绑定中的位置；说明它是服务端执行范围快照，不是 permit、签名或权限授予。
- 为 `ExecutionScope` 所有字段解释主体/项目/session/run/turn/cell/grant/budget/work packet、环境与 workspace、permission ScopeSet、读写根与 deny、memory/server/network 声明、authority/trust/data/cancellation 版本、deadline/fencing 和四类 digest 的来源及缺省语义。
- 为 digest/list 上限、`validate()`、`canonical_bytes()`、`validate_for_request()`、`is_subset_of()` 与 `digest()` 补充输入输出、失败边界和算法意图；说明 Serde 的序列化/反序列化与 `deny_unknown_fields`。
- 调用核对：Core `build_execution_scope()` 由请求上下文和有效 ScopeSet 构造快照并调用 `validate()`；Broker `validate_action()` 在 handler 前调用 `validate_for_request()`，该校验要求 catalog digest 与 action digest 匹配。`canonical_bytes()` 与 `ExecutionScope::is_subset_of()` 未找到直接生产调用点。
- 发现项只记录、不修复：Core 当前构造器将 authority/data/cancellation epoch 与 fencing 固定为 1，并以 `u64::MAX` 表示未设置有限 deadline；Domain 只检查非零和摘要形状/内部一致性，不查询权威账本、当前时间或最新 fencing。`grant_refs.windows(2)` 只拒绝相邻重复项；`validate_for_request()` 只比较 grant 首项，Run/Turn 仅在参数成功解析时检查；`is_subset_of()` 不逐字段比较 environment、根路径/deny、memory/server/network 列表、trust/catalog/action digest 或 budget lease。字符串列表上限只校验文本形状，不做路径 canonicalize 或实际权限阻断。以上均描述现有代码边界，不据此断言已发生外部故障。
- 源文件在本批开工时 Git 干净；只增加中文注释，没有改变字段、签名、常量、分支、摘要或比较逻辑。未运行测试、构建、格式化或提交。

## 2026-10-13 ScopeSet 多维授权范围注释批次

- `kiana-domain/src/scope.rs`：补充 ScopeSet 在 Core 动作/上下文 scope 投影、Policy 多层 GrantScope 收窄和 Broker discovery 过滤中的位置；说明 Domain 只表达/比较范围，不直接拦截文件/网络、不验证 grant 签发者、不授予能力。
- 解释 `ScopeDimension::NotApplicable` 与空 `Restricted`、`ScopeLimit` 缺省、Serde tagged enum、默认值、256 项/4,096 字节上限、trim/排序/重复拒绝、路径词法层级、预算/深度取小值、稳定摘要和空层拒绝等设计理由。
- 为 `new/validate/intersect/intersect_all/is_subset_of/allows_operation/allows_path/digest` 及内部 normalize/intersection/subset helper 补充输入输出、错误和边界说明。`intersect_all()` 当前无直接调用者；Policy 的 `GrantScope::allows_request()` 会对请求 `path` 调用 `ScopeSet::allows_path()`，请求键名约定为 `path`。
- 发现项只记录、不修复：`intersect_path_dimension()` 与 `path_subset()` 将 `.` 和 `*` 作为根范围标记；`allows_path()` 只做精确字符串或 `value/` 前缀判断、没有同样特判，而 Policy 会调用后者判断请求路径。可能造成不同路径 helper 对这些值解释不一致，需确认预期合同；注释不推断已发生越权或拒绝事故。路径 helper 是词法字符串规则，不处理 symlink、大小写和真实 OS canonicalization。
- 源文件开工前 Git 干净；只增加中文注释，没有改变 ScopeSet 字段、默认值、比较分支、交集算法、digest 或签名。未运行测试、构建、格式化或提交。

## 2026-10-13 Capability Domain 合同注释批次

- `kiana-domain/src/capabilities.rs`：补充文件职责和跨 crate 调用图，说明 Runner 请求进入 Core 规范化/授权、Broker 返回结果、Core/EventLog 生成回执、Harness 将有界观察交还模型的主数据流。
- 为 CapabilityRequest、CapabilityGrant、ExecutionScope envelope、Policy/Gate 决定、审批挑战与恢复快照、SupervisionLease、CapabilityResult、CapabilityResultReceipt、ToolObservation 等关键类型和方法补充中文职责、字段语义、调用者、输入输出、失败边界和授权/效果信任边界。
- 解释进程、停止与外部效果分开分类；字符串错误仅用于诊断；结果摘要和自摘要不等于 EventLog 提交证明、签名或现实效果证明；模型修复提示不授予下一次请求权限。
- 调用核对：Runner `tools.rs` 构造模型工具请求；Core 规范化并完成 policy/gate/approval 后创建授权 envelope，Broker 执行并返回结果；Core `capabilities.rs` 构造结果回执并写入事件，invocation projection 重新读取/比较回执；Harness 先拒绝 effect 未知结果，再创建 ToolObservation 并决定是否继续模型循环。`CapabilityGrant::allows_request()` 是局部 operation/path 匹配，CellRegistry 外层仍需做身份、有效期及完整 grant 校验。
- 发现项只记录、不修复：`ToolObservation::from_result()` 以字符数截取摘要，而 `validate()` 以 UTF-8 字节数限长；多字节文本可导致自身校验失败。摘要不附加截断标记。CapabilityResultReceipt 的 `committed` 是调用方字段，Domain 自校验不证明 EventLog 已提交；`result_digest` 自身形状/回执摘要校验也不重算原始结果。`ExecutionScope`/授权 ID 都是普通 DTO 字段，不能单靠类型构造证明授权。
- `capabilities.rs` 开工前 Git 干净；本批只增改中文注释，`git diff --unified=0` 的非注释行分类为空且 `git diff --check` 通过。未运行测试、构建、格式化或提交。

## 2026-10-13 Chrome MCP 边缘模块注释批次

- `kiana-chrome-mcp/src/lib.rs`、`src/main.rs`：说明独立 JSON-RPC/MCP stdin/stdout 服务、`main -> run_stdio`、CLI 直达入口、MCP/Chrome DevTools Protocol/Serde/Tokio 的职责；解释 ChromeClient 的惰性启动与共享页面锁、浏览器工具输入/输出、未知工具与参数错误的分层、任意 JavaScript 的边界及截图 Base64 编码路径。
- `src/native_host.rs`：解释 Chrome Native Messaging 的 4 字节 little-endian 长度帧与 1 MiB payload 上限、stdio 读写、Unix socket listener、容量 32 的 client queue、原子 client ID、扩展与 MCP client 之间的广播、权限位及 Drop 清理范围。
- `src/native_install.rs`：解释 CLI 配置 → 安装计划 → wrapper/manifest 写入 → report 的数据流；逐项说明目标平台、home、binary path、dev origin、browser manifest 目录和 Windows registry argv。强调计划生成不写文件，installer 会产生本机写入副作用。
- 调用核对：README 提供 standalone MCP server 使用方式；workspace manifest 收录该 crate；`kiana-entrypoints/Cargo.toml` 将其作为非 optional 依赖，`cli.rs` 的 `--claude-in-chrome-mcp`/`--chrome-native-host` 直达两个入口，`chrome install/plan` 路径调用安装器。该 direct server 源码未调用 DaemonHost、ControlPlane 或 CapabilityBroker；`docs/coding-pack-matrix.md` 将 SURF-CHROME 标记为非 Harness/冻结面。
- 发现项只记录、不修复：AGENTS.md 将 Chrome MCP 称为 feature-gated，而目前 workspace 确有 crate 成员、entrypoints 依赖不是 optional，也没有 chrome feature；运行命令由 CLI 参数分支选择。`NavigateParams`、`ClickParams`、`TypeParams`、`EvaluateParams`、`ScreenshotRequest` 当前仓内只有定义，dispatcher 读取原始 `serde_json::Value`。MCP JSON-RPC request ID 在 native socket→extension 转发时没有带入，扩展响应被广播给所有连接 client。NativeHost header 读取遇到 `UnexpectedEof` 一律返回 None，无法区分空流和不完整的部分 header；socket 清理按 `.sock` 文件名 PID 调用 `/proc/<pid>`，在无 `/proc` 的 Unix 系统上会误判活动 PID。socket path 存在时无探测就删除；guard 不追踪/等待已 spawn 的 client task，删除 socket 错误被忽略。
- 安装边界发现：Windows `reg add/query/delete` argv 只生成并由 CLI 展示，`install_native_host()` 实际只写 wrapper/manifest；多文件中途失败不回滚。wrapper 内容相同时 installer 提前跳过，不会修复 executable bit；Windows batch wrapper 仅给 binary path 加双引号，没有对路径内双引号/批处理元字符做单独转义。浏览器 `navigate` 每次打开新 page 且未显式关闭先前 page，`ChromeMcpServer` 锁覆盖整个调用会串行化同一实例的工具操作。以上为静态行为观察，不表示已经造成生产事故。
- 四个源文件开工前 Git 均干净；本批只加中文注释，非注释差异分类为空且 `git diff --check` 通过。未运行测试、构建、格式化或提交。

## 2026-10-13 Computer-use MCP 边缘模块注释批次

- `kiana-computer-mcp/src/lib.rs`：补充文件职责和 direct CLI→MCP→工具目录/调用→可选 native backend→OS 桌面的数据流，区分默认构建与 `native` feature，并解释 Tokio、Serde、MCP、Enigo/Xcap/Arboard 在当前路径中的作用。
- 为 ToolDef、截图 DTO、权限 DTO、AppInfo、MCP content/result 和 ComputerMcpServer 说明字段来源/去向、Option 缺省映射、工具目录与 `call_tool()` 分支关系；为 stdio loop、JSON-RPC handler、MCP result conversion、两种 cfg 下的 call_tool 补充调用、输入输出、失败传播和副作用说明。
- 复杂操作块说明 PNG 截图/Base64、绝对坐标窄化、点击缺一个坐标时的当前位置行为、滚动默认值、按键首字符 fallback、系统剪贴板读写、鼠标按下/释放分离，以及 OS 错误如何变成 MCP tool error。
- 调用核对：`kiana-entrypoints` 的 `native-computer-use` feature 才启用可选 desktop dependencies；默认 feature 集为空。CLI aliases 仍直接调用 `kiana_computer_mcp::run_stdio()`，不经 DaemonHost/ControlPlane/CapabilityBroker。`kiana-computer-mcp` 没有 crate-local README，行为边界参考根 AGENTS 与 coding-pack matrix 的 feature-gated/non-product/frozen 描述。
- 发现项只记录、不修复：启用 native 后 `request_access` 固定返回“granted”、`list_granted_applications` 固定返回“No restrictions”，没有系统权限弹窗、应用 allowlist 或授权状态存储；`ToolDef` 全部 `input_schema=None`，对外 MCP schema 因而都降为没有 properties 的空 object，未表达实际参数。`CuPermissionRequest`/`CuPermissionResponse`/`AppInfo`/`ScreenshotDims` 在仓内只有定义；截图结果忽略宽高。鼠标/滚动 i64 值窄化为 i32 且没有坐标/范围约束；click 只在 x/y 都有效时移动，否则点击当前位置；key fallback 只取首字符，空字符串映为空格；down/up 没有跨调用配对 guard。JSON-RPC 入口跳过无效 JSON 行，不校验版本字段。以上为源码观察，未宣称已经造成生产故障。
- `lib.rs` 开工时 Git 干净；只增加中文注释，非注释差异分类为空且 `git diff --check` 通过。未运行测试、构建、格式化或提交。

## 2026-10-13 Modifier Polling 辅助模块注释批次

- `kiana-modifiers/src/lib.rs`：说明只轮询当前 modifier、不注入按键/订阅事件；区分 `get_modifier_status()` 的平台/后端/可用性/错误信息和 `get_modifiers()` 丢弃失败状态后的歧义。调用核对只找到旧兼容命令 `kiana-commands/src/doctor.rs` 使用完整状态接口。
- 解释 macOS Core Graphics event flags 的映射；Linux 动态加载 Xlib、打开默认 X11 display、32 字节 keymap 位图、keysym→keycode、左右物理键合并、Wayland-only 不回退；Windows `GetAsyncKeyState` 读取当前高位按下状态；unsupported cfg 明确返回 unavailable。为 unsafe FFI、空指针检查、display 关闭、keycode 位索引及大小写匹配写说明。
- 发现项只记录、不修复：Linux backend 仅调用 XOpenDisplay，Wayland 环境必须有 X11/XWayland `DISPLAY`；`get_modifiers()` 会把读取失败折叠为空列表。Windows helper 总是标记 available，未表示系统 API 无法提供状态的可能性。`prewarm()` 当前为空；`test_get_modifiers` 使用永真断言，不能证明结果正确。最重要的是：Cargo.lock 中 core-graphics 锁定为 0.23.2，其 `CGEventSource::new(CGEventSourceStateID) -> Result<_, ()>` 和 `CGEvent::new(CGEventSource) -> Result<_, ()>` 与此文件传入整数 0、使用 `let Some(...)` 两处存在类型签名不匹配，疑似 macOS 目标编译失败；未运行跨平台构建，记录为需确认项，不在注释任务内修改。
- 源文件开工时 Git 干净；只增加中文注释，非注释差异分类为空且 `git diff --check` 通过。未运行测试、构建、格式化或提交。

## 2026-10-13 Parked TUI 屏幕模块注释批次

- `kiana-screens/src/lib.rs`、`app.rs`、`doctor.rs`、`settings.rs`：说明旧 TUI crate 与 entrypoints adapter 的调用关系、screen state/action 分层、键盘路由、terminal 主循环和外部 callback、Doctor/Settings 数据回填与渲染边界；解释 16 ms 轮询、action 一次性取走、窗口高度驱动的滚动裁剪及诊断原文优先级。
- `history.rs`、`resume_conversation.rs`：分别说明 prompt 历史与搜索词统计是两种数据、从 AppAction/SDK adapter 到状态列表再回到 REPL 草稿或 session resume 的数据流；解释过滤结果索引到原列表的映射、搜索态 Esc 的两段行为、空结果下 Enter 不产生恢复事件，以及会话 ID 与列表位置的区别。
- `search_history.rs`：说明按规范化 query + context 合并记录、频率加新近度（frecency）排序、时间档位、JSONL 逐行恢复/坏行跳过、dirty 标志、整文件重写及 Drop 尽力保存；明确它不保存 prompt、不参与授权，也不是 EventLog。
- 调用核对：`/history` 由 `App` 发 `LoadPromptHistory`，entrypoints 从 prompt-history 文件加载并回填；Enter 只恢复 REPL 草稿。Resume 列表由旧 SDK `list_sessions()` 产生，adapter 有 cwd 元数据时过滤，没有任何 cwd 元数据时保留全部列表；screen 沿用 adapter 顺序，Enter 把 session ID 交给 adapter 再异步取会话详情/消息。该 TUI 在产品规划中 parked，不能当作 DaemonHost 主路径。
- 发现项只记录、不修复：`HistoryScreen` 帮助文案显示 Ctrl+R，但当前处理分支仅在 `search_active` 时可达；历史查询过滤用 `to_ascii_lowercase()`，只能折叠 ASCII 英文字母大小写。搜索 manager 初始化错误被 `HistoryState::default()` 丢弃，Mutex poison 会被 `unwrap()` 变成 panic；manager 构造时将磁盘读取错误降级为空集合，坏 JSON 行只打印警告并跳过，保存通过 `File::create` 覆盖而非原子替换，析构时保存失败会被丢弃。Resume 文件此前声称按 mtime 排序，实际 screen 不排序，已更正注释；其 title/session ID/timestamp 搜索也只做 ASCII 大小写折叠。App 启用 mouse capture 但只分发 key event；启用 raw mode 后，初始化或循环中的 `?` 错误都可能在终端恢复代码前退出，当前没有 Drop cleanup guard。Doctor 有诊断文本时会提前绘制并跳过结构化字段。Settings 的 PageUp/PageDown 单次只移动一个 row offset。以上都是静态实现边界，不据此推断已发生用户故障。
- 以上七个 `kiana-screens` 文件在各自开工前均为 Git 干净；本批只加/改中文注释和本进度文档，`git diff --check` 通过，新增源码行的差异分类未发现非注释；未运行测试、构建、格式化或提交。`repl.rs` 在下一条批次中单独审阅与注释。

## 2026-10-13 Parked TUI REPL 注释批次

- `kiana-screens/src/repl.rs`：补充旧 TUI REPL 与 `App`/entrypoints adapter 的输入、事件、异步 stream 回填链路；解释 transcript 与 EventLog 的边界、UI history 与持久化 prompt history 的区别、工具权限面板只是摘要、全局快捷键先由 App 截获。
- 为消息/角色/权限展示 DTO、ReplEvent、ReplState 字段和状态方法补充业务语义；逐段说明 Enter/Shift/Alt+Enter、提交与排队、slash command、UTF-8 字节光标、Ctrl-U/Ctrl-W、历史 draft 往返、消息滚动、spinner、usage 字段和权限面板更新。
- 为 Ratatui renderer 与换行 helper 解释视觉行窗口、CJK 列宽、逻辑换行与显示前缀、控件高度边界、cursor 边框偏移、输入/消息预览不改原文、空宽度及超宽单词行为。将旧注释中“ISO-8601 时间戳”和“Ctrl-U 清空整行”的不准确描述改为与实现一致的说明。
- 调用核对：`App::handle_key()` 首先截获审批与取消快捷键，之后才调用本模块；`ReplEvent` 再由 App 映射成 `AppAction`，`TuiRuntime` 才启动 SDK prompt/command 或回复 permission oneshot。Prompt 的 stream delta/usage/tool events 在 adapter 中回填本 state。此为 parked 兼容 UI 路径，不是 Harness 主路径。
- 发现项只记录、不修复：adapter 当前未给 `session_cost` 赋值，状态栏成本保留默认 `0.0`；普通 Enter 在发出 action 前已清空输入，后续请求失败时本模块不自动恢复草稿；加载期间输入不在 allowlist 的 slash 命令时本函数不发事件也不清空输入，因此没有本地拒绝提示。`wrap_text()` 按空白折行并合并连续空白，不拆分没有空白的超宽单词。以上只描述当前实现，不代表用户已遇到这些情况。
- `repl.rs` 在开工前 Git 干净；只增改中文注释，新增源码行差异分类为空且 `git diff --check` 通过。未运行测试、构建、格式化或提交。至此 `kiana-screens/src` 八个源文件均已完成本阶段注释，状态仍标为待最终复审。

## 2026-10-13 OAuth Account Query 注释批次

- `kiana-query/src/oauth_accounts.rs`：补充其在 OAuth 账号记录到只读脱敏查询页之间的位置；解释 Domain `OAuthAccountProjection::from_record()` 的校验/脱敏职责、BTreeMap 按账号 ID 聚合和稳定排序、同 ID 不同 digest 冲突，以及 schema、projection version、source cursor、stale 和 limitations 字段的含义。
- 调用核对：Query crate root 重导出函数和 DTO；workspace 直接调用 `project_oauth_accounts()` 仅见 INT-12 query projection fixture，没有找到生产查询入口。Domain/Ports/Daemon 分别提供账号记录、CAS 存储端口和当前 in-memory adapter；这些是跨文件数据流背景，本批只改 Query 文件。
- 发现项只记录、不修复：`source_cursor` 仅检查非零，来自调用方且未关联实际 EventStore 读取；`stale` 在此构造器中固定为 false，没有 freshness 对比。因而该页不能证明源游标完整、投影新鲜、OAuth 凭证仍可用或 connector effect 已执行。`deny_unknown_fields` 只约束 Serde 反序列化的字段集合，不替代 DTO 业务关系校验。
- `oauth_accounts.rs` 开工前 Git 干净；只增加中文注释，新增/删除源码差异分类均未发现非注释行，`git diff --check` 通过。未运行测试、构建、格式化或提交。

## 2026-10-13 既有授权注释边界复核

- `kiana-policy/src/grant_scope.rs` 已有较完整中文说明；本次依据 `kiana-core/src/capabilities.rs` 的真实调用点复核 `GrantScope::intersect_all()`：父级、模板、部门、项目、工作包和审批层依次收窄后形成子授权。
- 修正原注释“求交集的顺序不影响结果”的过宽说法：有效权限集合的交集语义可交换，但每次 `intersect()` 都生成新 `GrantId`，并把当前累加器 ID 记录为父级；Vec 字段顺序和错误首先暴露的位置也受输入顺序影响。没有改变交集、ID 生成或错误顺序实现。
- `kiana-gates/src/lib.rs` 已核对 `DaemonHost::with_default_components()` 装配 `DefaultGateEngine`，Core 在策略决定之后调用 Gate；`kiana-query/src/index.rs` 已核对 CLI 与 Daemon 查询入口。两文件源码已有较多中文说明，本次只更新盘点状态，不为增加数量重复注释，仍待最终准确性复审。
- `grant_scope.rs` 开工时 Git 干净；仅修改文档注释和盘点文档。未运行测试、构建、格式化或提交。

## 2026-10-13 ProjectTrust 注释准确性复核

- `kiana-policy/src/project_trust.rs` 开工时 Git 干净，已有文件级流程说明及公开类型/方法说明；本批清理 schema 常量、字符串校验、摘要校验和 scope priority 的重复注释，并补齐决定枚举与 Resolution 字段含义。
- 按真实实现修正几个容易被误读的保证：scope priority 在 `resolve()` 先筛到同一 scope 后不会区分候选；摘要是无密钥 JSON hash，只能发现未同步的内容变化，不能证明签发者身份；`audit_ref` 的脱敏要求由写入者承担，当前 validator 不检查是否包含真实路径。
- 重新说明数据流：调用方提供项目/资源摘要与记录 → `resolve()` 校验并按项目摘要和 scope 过滤 → 按 revision 与 root_digest 排序 → 计算结论和摘要 → 返回 DTO。当前没有生产调用者；真实加载链使用 `kiana_types::ProjectTrust` 与 `kiana-skills::SourceTrust`，不能把本 DTO 说成已写入 EventLog。
- `allows_loading()` 只读 decision；从 `resolve()` 得到的对象先经过 validate，但外部反序列化或手工构造的数据仍需调用方显式校验。注释同时限定 validate 的实际边界，并记录它未验证候选项目摘要与头字段相等、selected_scope 与所选候选相符、decision 与 selected state 一致。
- 发现项仅记录、不改逻辑：若调用方把可控/反序列化 DTO 当可信结果，当前 `validate()` 的上述关系缺口可能允许自洽摘要包裹语义不一致的结果；由于当前无生产调用者，本批不推断为已发生产品问题。Root/Resolution 的摘要都不是签名或 MAC。
- `grant_scope.rs`、`project_trust.rs` 在相应编辑前均为 Git 干净；源码差异仅为注释增删，更新了库存状态。未运行测试、构建、格式化或提交。
