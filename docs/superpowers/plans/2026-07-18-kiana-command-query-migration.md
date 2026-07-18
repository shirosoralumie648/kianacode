# Kiana Command Query Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Execution rule:** 本项目不采用 TDD。每个任务先实现已批准的控制平面合同，再添加并运行 focused、adversarial、integration 和 review 验证。

**Goal:** 将 `kiana context` 的 Query 能力按只读到本地写入的顺序迁入 `Client -> Protocol -> Daemon -> Core -> CapabilityBroker -> Query adapter`，最终删除 `kiana-commands -> kiana-query` 直接依赖并把 legacy edge 从 10 降到 9。

**Architecture:** `kiana-commands` 只解析命令并返回结构化 `CommandRoute`；`kiana-entrypoints` 的统一 dispatcher 根据 route 调用本地 command 或 `kiana-client`。`kiana-core` 只识别稳定 command intent、确定风险并发出 `CapabilityRequest`，`kiana-daemon` 组合 `kiana-query` concrete handler，所有 Query 执行都经过 policy、gates、broker 和 EventLog。

**Tech Stack:** Rust 2021、Tokio、Serde/Serde JSON、async-trait、Cargo workspace、现有 `kiana-query` API、现有 control-plane crates。

---

## 文件结构

```text
kiana-commands/src/types.rs
  CommandRoute 与 Command::route 默认合同。

kiana-commands/src/context.rs
  只保留 context 参数解析、本地 status/json 和 surface 输出合同；逐项删除 Query 实现调用。

kiana-entrypoints/src/command_dispatch.rs
  CLI/REPL/TUI/App Server 共用的 local/control-plane dispatcher；从可信本地状态构造 RequestMetadata。

kiana-core/src/lib.rs
  context command use case、风险分类、CapabilityRequest 构造和事件顺序。

kiana-daemon/src/context_query.rs
  `kiana-query` concrete capability handlers、路径约束和兼容文本输出。

kiana-capability-broker/src/lib.rs
  组合根可在启动阶段同步注册静态 handler，运行后仍使用异步执行。

kiana-daemon/tests/control_plane.rs
  Client 到 Query adapter 的纵向测试，以及未信任/路径越界拒绝测试。

kiana-core/tests/dependency_boundaries.rs
  允许 Daemon 组合 Query adapter；最终删除 commands/query legacy exception。
```

## 迁移原则

1. 不把 `kiana-query` 类型复制到 Protocol 或 Domain；wire 参数使用小型、版本化 JSON object。
2. `project_root` 只能由 Daemon 从已校验的 `RequestMetadata` 注入，客户端 arguments 中同名字段不得生效。
3. Query handler 的 root、source、cache 和 store path 必须归一化并限制在 project root 内。
4. 只读操作可由可信 project 直接执行；缓存、ingest 和 artifact materialization 标记 `LocalWrite`，在 approval contract 完成前保持 legacy 路径，不伪装成只读。
5. 每迁移一个 subcommand，`ContextCommand::execute` 对该 subcommand 必须返回 `command_requires_control_plane`，防止旧调用路径静默绕过。
6. CLI、REPL、TUI 和 App Server 必须调用同一个 dispatcher；任何 surface 直接调用 routed command 都由测试判定为错误。

### Task 1: 建立 routed command 合同和 dispatcher

**Files:**
- Modify: `kiana-commands/src/types.rs`
- Modify: `kiana-commands/src/lib.rs`
- Create: `kiana-entrypoints/src/command_dispatch.rs`
- Modify: `kiana-entrypoints/src/lib.rs`
- Modify: `kiana-entrypoints/src/cli.rs`
- Modify: `kiana-entrypoints/src/repl.rs`
- Modify: `kiana-entrypoints/src/tui.rs`

- [ ] **Step 1: 增加结构化 route 合同**

在 `kiana-commands/src/types.rs` 增加：

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum CommandRoute {
    Local,
    ControlPlane { name: String, arguments: Value },
}
```

并给 `Command` 增加默认方法：

```rust
fn route(&self, _context: &CommandContext) -> anyhow::Result<CommandRoute> {
    Ok(CommandRoute::Local)
}
```

默认合同保证未迁移 command 行为不变。

- [ ] **Step 2: 实现统一 dispatcher**

`kiana-entrypoints/src/command_dispatch.rs` 暴露：

```rust
pub async fn execute_command(
    command: &dyn kiana_commands::Command,
    context: kiana_commands::CommandContext,
) -> anyhow::Result<kiana_commands::CommandResult>;
```

`Local` 调用现有 `Command::execute`；`ControlPlane` 必须：

- 从 `context.app_state.cwd` 获取 project root；
- 用 `kiana_types::project_trust_from_app_state` 取得 fail-closed trust；
- 从 `session_id` 或稳定的 `local-command` fallback 构造 metadata；
- 创建 `DaemonHost::local`、`KianaClient` 和 in-process transport；
- 只接受 `ExecutionStatus::Completed`；
- 将 response `output.command_result` 反序列化为 `CommandResult`；
- 对 `Denied`、`AwaitingApproval`、`Blocked`、`Failed` 和 `ResultUnknown` 返回包含 daemon reason 的错误。

- [ ] **Step 3: 切换通用 surface 调用点**

将以下通用路径改用 `execute_command`：

```text
kiana-entrypoints/src/cli.rs::run_local_command
kiana-entrypoints/src/cli.rs::direct_connect_app_command_run_payload
kiana-entrypoints/src/repl.rs::run_repl command branch
kiana-entrypoints/src/tui.rs::handle_slash_command spawned branch
```

专用于 doctor/settings/review/diff 的内部调用暂不修改，因为它们不会解析 routed context subcommand。

- [ ] **Step 4: 添加实现后 dispatcher verification**

覆盖：默认 command 仍走 local；缺 cwd fail closed；unknown trust 不会被 dispatcher 提升；非 completed response 不会转成成功文本。

- [ ] **Step 5: 运行 focused tests**

```bash
cargo test -p kiana-commands types --locked --offline
cargo test -p kiana-entrypoints command_dispatch --locked --offline
cargo fmt --all --check
```

- [ ] **Step 6: 提交 route 基础**

```bash
git add kiana-commands/src/types.rs kiana-commands/src/lib.rs \
  kiana-entrypoints/src/command_dispatch.rs kiana-entrypoints/src/lib.rs \
  kiana-entrypoints/src/cli.rs kiana-entrypoints/src/repl.rs kiana-entrypoints/src/tui.rs
git commit -m "feat: route commands through the control plane"
```

### Task 2: 迁移 `context repo-map` 纵向切片

**Files:**
- Modify: `kiana-commands/src/context.rs`
- Modify: `kiana-capability-broker/src/lib.rs`
- Modify: `kiana-core/src/lib.rs`
- Modify: `kiana-daemon/Cargo.toml`
- Create: `kiana-daemon/src/context_query.rs`
- Modify: `kiana-daemon/src/lib.rs`
- Modify: `kiana-daemon/tests/control_plane.rs`
- Modify: `kiana-core/tests/dependency_boundaries.rs`
- Modify: `kiana-entrypoints/src/cli.rs`
- Test: `kiana-commands/src/context.rs`
- Test: `kiana-entrypoints/tests/cli_architecture.rs`

- [ ] **Step 1: 让 Context parser 产生稳定 intent**

`context repo-map [--json] [--max-tokens N]` 解析为：

```json
{
  "operation": "repo_map",
  "output": "json",
  "options": { "max_tokens": 1000 }
}
```

command name 固定为 `context.query.v1`。未知 flag、零值和缺失整数继续返回现有 usage/error；`ContextCommand::execute` 遇到 repo-map 必须返回 `command_requires_control_plane`。

- [ ] **Step 2: 在 Core 中分类并授权 Query capability**

`ControlPlane::handle_command` 识别 `context.query.v1`，只接受 `operation=repo_map`，忽略客户端提供的 `project_root` 并写入 `RequestContext.project_root`，构造：

```rust
CapabilityRequest::new(
    context.request_id,
    CapabilityKind::Query,
    "context.repo_map",
    normalized_arguments,
)
.with_risk(RiskLevel::ReadOnly)
```

该路径复用 `authorize_and_execute` 的 policy/gate/broker 逻辑，但必须先把公共逻辑提取为可指定起始 sequence 的私有 helper：同一 request 只能写一次 `request.accepted`，之后依次写 `capability.decision` 和 `capability.completed/failed`。不得出现两个 `sequence=1`，也不得直接调用 Query 或 Broker implementation。

- [ ] **Step 3: 支持组合根静态注册 handler**

给 `CapabilityBroker` 增加只在 `&mut self` 上可用的同步注册方法：

```rust
pub fn register_static(
    &mut self,
    capability: CapabilityKind,
    operation: impl Into<String>,
    handler: Arc<dyn CapabilityHandler>,
) -> Result<(), PortError>;
```

它通过 `RwLock::get_mut()` 写入启动期 map，并与异步 `register` 使用同一 duplicate 检查。

- [ ] **Step 4: 实现 Daemon Query handler**

`kiana-daemon/src/context_query.rs` 实现 `CapabilityHandler`：

- 必须要求 `arguments.project_root`；
- project root 必须存在、为目录并 canonicalize；
- 调用 `build_repo_map(root, RepoMapOptions { max_tokens })`；
- `output=json` 返回现有 pretty JSON；`output=text` 返回与旧 formatter 字节一致的文本；
- 返回 `CapabilityResult::success`，output 形状为：

```json
{
  "command_result": {
    "output_type": "text",
    "value": "...",
    "metadata": null
  }
}
```

`DaemonHost::local` 注册 `(CapabilityKind::Query, "context.repo_map")`，注册失败必须返回 `Result`，不得 `unwrap`。

- [ ] **Step 5: 添加实现后 vertical/adversarial verification**

验证：

- trusted Client 能通过 Daemon/Core 得到 repo map；
- unknown/untrusted project 返回 `project_untrusted`；
- client arguments 中伪造 `project_root` 不覆盖 metadata root；
- malformed max_tokens 和 operation fail closed；
- EventLog 顺序严格为单个 request accepted、policy/gate decision、capability completed，sequence 单调且无重复；
- `ContextCommand::execute` 不能直接执行已迁移 repo-map。

- [ ] **Step 6: 切换 CLI 回归测试**

给测试 workspace 写入外部 trust record，再验证：

```bash
kiana context repo-map --json --max-tokens 1000
```

输出 schema/预算/文件和 symbol 与迁移前一致；没有 trust record 时命令失败并包含 `project_untrusted`。

- [ ] **Step 7: 运行 focused verification**

```bash
cargo test -p kiana-capability-broker -p kiana-core -p kiana-daemon --locked --offline
cargo test -p kiana-commands context_repo_map --locked --offline
cargo test -p kiana-entrypoints context_repo_map --locked --offline
cargo test -p kiana-core --test dependency_boundaries --locked --offline -- --nocapture
cargo fmt --all --check
```

- [ ] **Step 8: 提交第一条 Query 切片**

```bash
git add kiana-commands/src/context.rs kiana-capability-broker/src/lib.rs \
  kiana-core/src/lib.rs kiana-daemon/Cargo.toml kiana-daemon/src/context_query.rs \
  kiana-daemon/src/lib.rs kiana-daemon/tests/control_plane.rs \
  kiana-core/tests/dependency_boundaries.rs kiana-entrypoints/src/cli.rs \
  kiana-entrypoints/tests/cli_architecture.rs
git commit -m "feat: route context repo map through core"
```

### Task 3: 迁移其余只读 Context Query

**Files:**
- Modify: `kiana-commands/src/context.rs`
- Modify: `kiana-core/src/lib.rs`
- Modify: `kiana-daemon/src/context_query.rs`
- Modify: `kiana-daemon/src/lib.rs`
- Modify: `kiana-daemon/tests/control_plane.rs`
- Test: `kiana-commands/src/context.rs`
- Test: `kiana-entrypoints/src/cli.rs`

- [ ] **Step 1: 迁移纯只读 operation**

依次增加稳定 operation：

```text
context.index.read
context.artifacts.read
context.artifact_graph.read
context.artifact_store.read
context.artifact_readiness.read
context.search
context.vector_search
context.pack
```

`index/artifacts/artifact-store` 只有未提供 `--cache` 时进入只读 route；带 cache 的调用继续走 legacy local path。

- [ ] **Step 2: 对每个 operation 在 Core 固定 risk**

Core 使用闭集 match 确定 `RiskLevel::ReadOnly`，不得接受客户端 `risk` 字段。未知 operation 返回 `command_unregistered`，options 必须是 object 且数值有界。

- [ ] **Step 3: 在 Daemon 注册独立 handler key**

每个 operation 使用独立 broker key，不使用一个可以绕过风险分类的通配 `execute` handler。路径 option 统一经过 project-root confinement helper。

- [ ] **Step 4: 添加实现后 parity verification**

保留现有 JSON schema、pretty JSON 和文本 golden；增加绝对路径、`..`、symlink escape、超大 limit/max bytes 和伪造 project root 的拒绝测试。

- [ ] **Step 5: 运行 Context/read-only 集成测试并提交**

```bash
cargo test -p kiana-commands context --locked --offline
cargo test -p kiana-daemon context_query --locked --offline
cargo test -p kiana-entrypoints context --locked --offline
cargo fmt --all --check
git add kiana-commands/src/context.rs kiana-core/src/lib.rs \
  kiana-daemon/src/context_query.rs kiana-daemon/src/lib.rs \
  kiana-daemon/tests/control_plane.rs kiana-entrypoints/src/cli.rs
git commit -m "feat: route read-only context queries through core"
```

### Task 4: 建立本地写入 approval/resume 合同

**Files:**
- Modify: `kiana-domain/src/lib.rs`
- Modify: `kiana-protocol/src/lib.rs`
- Modify: `kiana-ports/src/lib.rs`
- Modify: `kiana-core/src/lib.rs`
- Modify: `kiana-daemon/src/lib.rs`
- Modify: `kiana-client/src/lib.rs`
- Modify: `kiana-entrypoints/src/command_dispatch.rs`
- Test: `kiana-daemon/tests/control_plane.rs`

- [ ] **Step 1: 定义不可由 client 自签的 approval challenge**

第一次 LocalWrite 请求返回 `AwaitingApproval` 和 daemon 生成的 `approval_id`，EventLog 绑定 request/session/actor/command hash。Protocol 增加独立 `ApprovalDecisionRequest` body，只携带 daemon challenge 和 approve/deny decision。

- [ ] **Step 2: Daemon 保存一次性 pending approval**

新增 `ApprovalStorePort` 和 Daemon 内存实现；记录 exact request hash、session、actor、expiry 和 consumed 状态。错误 session、actor、hash、过期或重放均 fail closed。

- [ ] **Step 3: Core resume 原请求**

批准后只恢复已记录的 normalized capability request；不得采用第二次 client 提供的新 arguments。拒绝写入 blocked event，批准写入 approval event 后进入 broker。

- [ ] **Step 4: Surface 接入确认交互**

CLI 非交互命令必须显式 `--approve-local-write` 才会响应 challenge；REPL/TUI 使用现有 permission UI。App Server 返回 awaiting approval 和 challenge，由后续 authenticated decision endpoint 完成，不能自动批准。

- [ ] **Step 5: 添加实现后 replay/security verification 并提交**

覆盖 approve、deny、wrong session、wrong actor、argument substitution、expiry、duplicate decision、process restart 和 event append failure；执行 focused tests 后提交：

```bash
git commit -m "feat: add resumable command approval gates"
```

### Task 5: 迁移 Context 本地写入操作

**Files:**
- Modify: `kiana-commands/src/context.rs`
- Modify: `kiana-core/src/lib.rs`
- Modify: `kiana-daemon/src/context_query.rs`
- Modify: `kiana-daemon/src/lib.rs`
- Modify: `kiana-entrypoints/src/command_dispatch.rs`
- Modify: `kiana-entrypoints/src/cli.rs`
- Test: `kiana-daemon/tests/control_plane.rs`
- Test: `kiana-entrypoints/src/cli.rs`

- [x] **Step 1: 迁移 cache materialization**

`index/artifacts/artifact-store --cache` 固定为 `RiskLevel::LocalWrite`；cache path 必须在 project root 下，写入结果保留 existing cache status contract。

- [x] **Step 2: 迁移 ingest**

`context ingest` 固定为 `RiskLevel::LocalWrite`；source 和 store 均限制在 project root，symlink escape 在执行前拒绝。

- [x] **Step 3: 验证 approval 后的字节 parity 和副作用边界**

验证未批准不创建目录/文件，批准只写允许路径，重复 approval 不重复执行，event persistence failure 返回 `result_unknown`。

- [x] **Step 4: 运行 focused tests 并提交**

```bash
cargo test -p kiana-commands --lib context --locked --offline
cargo test -p kiana-daemon --test control_plane --locked --offline
cargo test -p kiana-entrypoints context --locked --offline
cargo fmt --all --check
git commit -m "feat: gate context materialization through core"
```

### Task 6: 删除 `commands -> query` legacy edge

**Files:**
- Modify: `kiana-commands/Cargo.toml`
- Modify: `kiana-commands/src/context.rs`
- Modify: `kiana-core/src/lib.rs`
- Modify: `kiana-core/tests/dependency_boundaries.rs`
- Modify: `kiana-entrypoints/tests/cli_architecture.rs`
- Modify: `docs/superpowers/specs/2026-07-17-kiana-control-plane-architecture-design.md`

- [ ] **Step 1: 删除最后的 Query imports 和 dependency**

确认：

```bash
rg -n 'kiana_query::' kiana-commands/src kiana-commands/tests
```

必须没有输出，然后从 `kiana-commands/Cargo.toml` 删除 `kiana-query`。

- [ ] **Step 2: 收紧依赖守卫和架构状态**

从 `LEGACY_EDGES` 删除 `("kiana-commands", "kiana-query")`，将 `LEGACY_EDGES_REMAINING` 从 10 改为 9，CLI architecture contract 同步更新。

- [ ] **Step 3: 运行完整 verification**

```bash
cargo fmt --all --check
cargo test -p kiana-commands --locked --offline
cargo test -p kiana-core --test dependency_boundaries --locked --offline -- --nocapture
cargo test -p kiana-daemon -p kiana-client -p kiana-protocol --locked --offline
cargo test -p kiana-entrypoints context --locked --offline
cargo test --workspace --locked --offline --no-fail-fast
```

已知 `kiana-computer-input::tests::test_init` 平台环境失败必须单独报告，不得把 focused pass 描述为 workspace 全绿。

- [ ] **Step 4: 提交 Query edge 删除**

```bash
git add kiana-commands/Cargo.toml kiana-commands/src/context.rs \
  kiana-core/src/lib.rs kiana-core/tests/dependency_boundaries.rs \
  kiana-entrypoints/tests/cli_architecture.rs \
  docs/superpowers/specs/2026-07-17-kiana-control-plane-architecture-design.md
git commit -m "refactor: remove command query dependency"
```

## 完成标准

- `kiana context` 的 Query 子命令不再直接调用 `kiana-query`；
- 所有 surface 使用同一 command dispatcher；
- Core 独立确定 capability kind、operation 和 risk；
- Daemon 是唯一 `kiana-query` 组合根；
- Query 路径有 policy、gate、EventLog、path confinement 和 result/error contract；
- `kiana-commands/Cargo.toml` 不含 `kiana-query`；
- dependency guard 和 architecture status 均报告 9 条 legacy edge；
- focused、workspace 和已知平台失败边界被准确记录。
