# Kiana Control Plane Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建立可运行的 `Client -> Protocol -> Daemon -> Core -> Policy/Gates -> EventLog` 纵向切片，并用自动化依赖守卫冻结现有 RUN/CMD 越权边。

**Architecture:** 采用 strangler migration。新 crate 从纯合同和控制平面向外生长，现有 `kiana-entrypoints`、`kiana-commands`、`kiana-tools` 和 `kiana-tasks` 作为显式 legacy adapter 保留；第一阶段只接入一个只读 `system.architecture` command，后续按 M2-M5 逐步切换并删除例外。

**Tech Stack:** Rust 2021、Tokio、Serde、async-trait、Cargo workspace、Rust integration tests。

---

## 文件结构

本计划创建以下职责单一的文件：

```text
kiana-domain/src/lib.rs                 领域 ID、上下文、状态、capability 和 event
kiana-runner-protocol/src/lib.rs        RunnerCommand / RunnerEvent
kiana-runner/src/lib.rs                 只消费 runner protocol 的 fail-closed runner 基线
kiana-policy/src/lib.rs                 fail-closed policy decision
kiana-gates/src/lib.rs                  approval/verification gate decision
kiana-workflow/src/lib.rs               纯 workflow state transition
kiana-ports/src/lib.rs                  EventStore / CapabilityBroker / Runner traits
kiana-eventlog/src/lib.rs               append-only in-memory event store 基线实现
kiana-capability-broker/src/lib.rs       授权后 capability 路由
kiana-core/src/lib.rs                    唯一 command/capability 控制平面
kiana-core/tests/dependency_boundaries.rs Cargo 依赖守卫
kiana-protocol/src/lib.rs                versioned request/response envelopes
kiana-client/src/lib.rs                  transport-neutral client
kiana-daemon/src/lib.rs                  composition root 和 protocol handler
kiana-daemon/tests/control_plane.rs      完整 in-process 纵向测试
```

现有文件只做窄修改：

```text
Cargo.toml                               注册新 workspace members/dependencies
Cargo.lock                               Cargo 自动更新
kiana-entrypoints/Cargo.toml             增加 client/daemon/protocol 依赖
kiana-entrypoints/src/cli.rs             接入 `architecture status` 新路径
kiana-entrypoints/tests/cli_architecture.rs 验证入口可发现且输出稳定
```

`Cargo.lock` 在本计划开始前已有用户改动。Cargo 命令可以在原文件上追加必要 workspace package 记录，但所有 scoped commit 都必须排除 `Cargo.lock`，避免把不属于本计划的 lockfile 变更混入提交。

### Task 1: 冻结依赖边界

**Files:**
- Modify: `Cargo.toml`
- Create: `kiana-core/Cargo.toml`
- Create: `kiana-core/src/lib.rs`
- Create: `kiana-core/tests/dependency_boundaries.rs`

- [ ] **Step 1: 写失败的依赖边界测试**

创建测试，调用 `cargo metadata --no-deps --format-version 1`，提取 workspace 内部直接依赖，并定义两类规则：

```rust
const STRICT_ALLOWED: &[(&str, &[&str])] = &[
    ("kiana-domain", &[]),
    ("kiana-protocol", &["kiana-domain"]),
    ("kiana-client", &["kiana-protocol"]),
    (
        "kiana-core",
        &[
            "kiana-domain",
            "kiana-gates",
            "kiana-policy",
            "kiana-ports",
            "kiana-runner-protocol",
            "kiana-workflow",
        ],
    ),
    ("kiana-runner", &["kiana-domain", "kiana-runner-protocol"]),
    ("kiana-runner-protocol", &["kiana-domain"]),
    ("kiana-policy", &["kiana-domain"]),
    ("kiana-gates", &["kiana-domain"]),
    ("kiana-workflow", &["kiana-domain"]),
    ("kiana-ports", &["kiana-domain", "kiana-runner-protocol"]),
    ("kiana-eventlog", &["kiana-domain", "kiana-ports"]),
    ("kiana-capability-broker", &["kiana-domain", "kiana-ports"]),
    (
        "kiana-daemon",
        &[
            "kiana-capability-broker",
            "kiana-core",
            "kiana-domain",
            "kiana-eventlog",
            "kiana-gates",
            "kiana-policy",
            "kiana-ports",
            "kiana-protocol",
            "kiana-runner",
            "kiana-runner-protocol",
        ],
    ),
];

const LEGACY_EDGES: &[(&str, &str)] = &[
    ("kiana-entrypoints", "kiana-commands"),
    ("kiana-entrypoints", "kiana-query"),
    ("kiana-entrypoints", "kiana-services"),
    ("kiana-entrypoints", "kiana-tools"),
    ("kiana-commands", "kiana-query"),
    ("kiana-commands", "kiana-services"),
    ("kiana-commands", "kiana-tasks"),
    ("kiana-commands", "kiana-tools"),
    ("kiana-tools", "kiana-query"),
    ("kiana-tools", "kiana-services"),
];
```

测试必须拒绝严格 crate 的额外内部依赖，并拒绝不在 `LEGACY_EDGES` 中的新 `surface/command/runner -> implementation` 边。

- [ ] **Step 2: 运行测试并确认失败**

Run: `cargo test -p kiana-core --test dependency_boundaries -- --nocapture`

Expected: FAIL，指出 `kiana-core` 尚未成为 workspace package。

- [ ] **Step 3: 创建可解析的最小 crate 骨架并注册 workspace members**

先为下列每个 crate 创建最小 `Cargo.toml` 和仅含 crate-level 文档的 `src/lib.rs`，再修改根 `Cargo.toml`。任何时刻都不得让 workspace 指向不存在的目录：

向根 `Cargo.toml` 加入：

```toml
"kiana-capability-broker",
"kiana-client",
"kiana-core",
"kiana-daemon",
"kiana-domain",
"kiana-eventlog",
"kiana-gates",
"kiana-policy",
"kiana-ports",
"kiana-protocol",
"kiana-runner",
"kiana-runner-protocol",
"kiana-workflow",
```

同时在 `[workspace.dependencies]` 注册同名 path dependencies。

- [ ] **Step 4: 运行边界测试并确认骨架通过**

Run: `cargo test -p kiana-core --test dependency_boundaries -- --nocapture`

Expected: PASS；严格 crate 此时没有额外内部依赖，legacy edge 集合与基线一致。

- [ ] **Step 5: 提交依赖守卫基线**

```bash
git add Cargo.toml kiana-capability-broker kiana-client kiana-core kiana-daemon \
  kiana-domain kiana-eventlog kiana-gates kiana-policy kiana-ports \
  kiana-protocol kiana-runner kiana-runner-protocol kiana-workflow
git commit -m "test: freeze control plane dependency boundaries"
```

### Task 2: 建立 Domain 与 Runner Protocol

**Files:**
- Modify: `kiana-domain/Cargo.toml`
- Modify: `kiana-domain/src/lib.rs`
- Modify: `kiana-runner-protocol/Cargo.toml`
- Modify: `kiana-runner-protocol/src/lib.rs`
- Modify: `kiana-runner/Cargo.toml`
- Modify: `kiana-runner/src/lib.rs`

- [ ] **Step 1: 写 Domain 状态和 secret-redaction 测试**

测试覆盖：

```rust
#[test]
fn request_context_defaults_to_untrusted() {
    let context = RequestContext::local("session-1", "/repo");
    assert!(!context.project_trusted);
    assert_eq!(context.permission_profile, PermissionProfile::Safe);
}

#[test]
fn capability_request_serialization_contains_reference_not_secret_value() {
    let request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Secret,
        "resolve",
        serde_json::json!({"secret_ref":"provider/anthropic"}),
    );
    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("secret_ref"));
    assert!(!json.contains("secret_value"));
}
```

- [ ] **Step 2: 实现纯 Domain 合同**

实现以下公开类型及构造函数：

```rust
RequestId, SessionId, RunId, EventId
PermissionProfile::{Safe, Balanced, Autonomous}
RequestContext
CommandIntent
CapabilityKind
RiskLevel
CapabilityRequest
AuthorizedCapabilityRequest
CapabilityResult
PolicyDecision
GateDecision
ExecutionStatus
RuntimeEvent
CoreResponse
```

`AuthorizedCapabilityRequest::new` 必须要求非空 `authorization_id`；`RuntimeEvent::new` 必须要求 `sequence > 0`。

- [ ] **Step 3: 实现 Runner Protocol**

```rust
pub enum RunnerCommand {
    Start { run_id: RunId, prompt: String },
    CapabilityResult { run_id: RunId, result: CapabilityResult },
    Cancel { run_id: RunId, reason: String },
}

pub enum RunnerEvent {
    Started { run_id: RunId },
    Delta { run_id: RunId, text: String },
    CapabilityRequested { run_id: RunId, request: CapabilityRequest },
    Completed { run_id: RunId, output: Value },
    Failed { run_id: RunId, error: String },
}
```

- [ ] **Step 4: 实现 fail-closed Runner 基线**

`ProtocolRunner` 只消费 `RunnerCommand` 并产生 `RunnerEvent`。在 Model Gateway 尚未注入时，`Start` 必须返回同一 `run_id` 的 `Started` 和 `Failed { error: "model_gateway_unavailable" }`，不得伪造 completed；`Cancel` 返回确定的 cancelled failure event。该 crate 不得依赖 `kiana-tools`、`kiana-services` 或 `kiana-query`。

- [ ] **Step 5: 验证合同测试**

Run: `cargo test -p kiana-domain -p kiana-runner-protocol -p kiana-runner`

Expected: PASS。

- [ ] **Step 6: 提交合同**

```bash
git add kiana-domain kiana-runner-protocol kiana-runner
git commit -m "feat: add control plane domain contracts"
```

### Task 3: 建立 Policy、Gates 与 Workflow

**Files:**
- Modify: `kiana-policy/Cargo.toml`
- Modify: `kiana-policy/src/lib.rs`
- Modify: `kiana-gates/Cargo.toml`
- Modify: `kiana-gates/src/lib.rs`
- Modify: `kiana-workflow/Cargo.toml`
- Modify: `kiana-workflow/src/lib.rs`

- [ ] **Step 1: 写 fail-closed policy 测试**

```rust
#[test]
fn untrusted_projects_cannot_execute_capabilities() {
    let context = RequestContext::local("session-1", "/repo");
    let request = capability(CapabilityKind::Filesystem, RiskLevel::ReadOnly);
    assert!(matches!(
        DefaultPolicyEngine.evaluate(&context, &request),
        PolicyDecision::Deny { .. }
    ));
}

#[test]
fn trusted_read_only_capability_is_allowed() {
    let mut context = RequestContext::local("session-1", "/repo");
    context.project_trusted = true;
    let request = capability(CapabilityKind::Query, RiskLevel::ReadOnly);
    assert!(matches!(
        DefaultPolicyEngine.evaluate(&context, &request),
        PolicyDecision::Allow { .. }
    ));
}
```

- [ ] **Step 2: 实现 Policy 与 Gate**

`DefaultPolicyEngine` 必须：未知/不可信项目拒绝 capability；只读可信请求允许；写入和外部副作用返回 `Ask`；secret、付款、发布、删除和权限变更始终返回 `Ask` 或 `Deny`。

`DefaultGateEngine` 必须把 `PolicyDecision::Ask` 转换为 `GateDecision::AwaitingApproval`，不得自动升级为 Allow。

- [ ] **Step 3: 实现 Workflow transition**

```rust
pub enum WorkflowState {
    Requested,
    Running,
    AwaitingApproval,
    Blocked,
    Completed,
    Failed,
    ResultUnknown,
}
```

只允许：`Requested -> Running/Blocked`、`Running -> AwaitingApproval/Completed/Failed/ResultUnknown`、`AwaitingApproval -> Running/Blocked`。终态拒绝继续 transition。

- [ ] **Step 4: 运行 focused tests**

Run: `cargo test -p kiana-policy -p kiana-gates -p kiana-workflow`

Expected: PASS。

- [ ] **Step 5: 提交决策层**

```bash
git add kiana-policy kiana-gates kiana-workflow
git commit -m "feat: add fail-closed policy and workflow gates"
```

### Task 4: 建立 Ports、EventLog 与 Capability Broker

**Files:**
- Modify: `kiana-ports/Cargo.toml`
- Modify: `kiana-ports/src/lib.rs`
- Modify: `kiana-eventlog/Cargo.toml`
- Modify: `kiana-eventlog/src/lib.rs`
- Modify: `kiana-capability-broker/Cargo.toml`
- Modify: `kiana-capability-broker/src/lib.rs`

- [ ] **Step 1: 写 port contract 测试**

定义并测试：

```rust
#[async_trait]
pub trait EventStorePort: Send + Sync {
    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError>;
    async fn read_request(&self, request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError>;
}

#[async_trait]
pub trait CapabilityBrokerPort: Send + Sync {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError>;
}

#[async_trait]
pub trait RunnerPort: Send + Sync {
    async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError>;
}
```

- [ ] **Step 2: 实现 append-only MemoryEventLog**

使用 `tokio::sync::RwLock<Vec<RuntimeEvent>>`。拒绝同一 `request_id` 下非单调 sequence，按 append 顺序返回事件。

- [ ] **Step 3: 实现默认拒绝 Capability Broker**

Broker 以 `(CapabilityKind, operation)` 注册 `CapabilityHandler`。未注册 handler 返回 `PortError::Unavailable`，不得返回空成功结果。

- [ ] **Step 4: 运行 focused tests**

Run: `cargo test -p kiana-ports -p kiana-eventlog -p kiana-capability-broker`

Expected: PASS，包括 duplicate/non-monotonic event 和 unregistered capability 拒绝测试。

- [ ] **Step 5: 提交 ports 与 adapter**

```bash
git add kiana-ports kiana-eventlog kiana-capability-broker
git commit -m "feat: add capability and event store ports"
```

### Task 5: 实现 Core 控制平面

**Files:**
- Modify: `kiana-core/Cargo.toml`
- Modify: `kiana-core/src/lib.rs`
- Create: `kiana-core/tests/control_plane.rs`

- [ ] **Step 1: 写 command 纵向失败测试**

```rust
#[tokio::test]
async fn architecture_command_records_accepted_and_completed_events() {
    let harness = CoreHarness::new();
    let response = harness
        .core
        .handle_command(
            trusted_context(),
            CommandIntent::new("system.architecture", Value::Null),
        )
        .await
        .unwrap();

    assert_eq!(response.status, ExecutionStatus::Completed);
    let events = harness.events.read_request(&response.request_id).await.unwrap();
    assert_eq!(events.iter().map(|event| event.kind.as_str()).collect::<Vec<_>>(),
               ["request.accepted", "command.completed"]);
}
```

- [ ] **Step 2: 实现 ControlPlane**

```rust
pub struct ControlPlane {
    policy: Arc<dyn PolicyEngine>,
    gates: Arc<dyn GateEngine>,
    events: Arc<dyn EventStorePort>,
    capabilities: Arc<dyn CapabilityBrokerPort>,
    runner: Arc<dyn RunnerPort>,
}
```

`handle_command` 只注册 `system.architecture`，返回 crate 边界和 `legacy_edges_remaining`；未知 command 返回结构化 `blocked`，并写 `command.rejected` event。

- [ ] **Step 3: 实现 authorize_and_execute**

顺序必须固定为：写 accepted event、policy evaluate、gate evaluate、写 decision event、构造 AuthorizedCapabilityRequest、broker execute、写 result event。任何 error 都返回结构化失败且保留已经发生的事件。

- [ ] **Step 4: 运行 Core tests**

Run: `cargo test -p kiana-core -- --nocapture`

Expected: PASS，包括未可信项目拒绝、Ask 不执行 broker、未知 capability 拒绝和 event 顺序测试。

- [ ] **Step 5: 提交 Core**

```bash
git add kiana-core
git commit -m "feat: add Kiana control plane core"
```

### Task 6: 建立 Protocol、Client 与 Daemon

**Files:**
- Modify: `kiana-protocol/Cargo.toml`
- Modify: `kiana-protocol/src/lib.rs`
- Modify: `kiana-client/Cargo.toml`
- Modify: `kiana-client/src/lib.rs`
- Modify: `kiana-daemon/Cargo.toml`
- Modify: `kiana-daemon/src/lib.rs`
- Create: `kiana-daemon/tests/control_plane.rs`

- [ ] **Step 1: 写协议 round-trip 测试**

协议 envelope 必须包含 `schema = "kiana.protocol.v1"`、request id、session id、project root 和 body：

```rust
pub enum RequestBody {
    Command(CommandRequest),
}

pub struct CommandRequest {
    pub name: String,
    pub arguments: Value,
}
```

JSON round-trip 后 request id、project trust 和 command arguments 必须不变。

- [ ] **Step 2: 实现 transport-neutral Client**

```rust
#[async_trait]
pub trait ClientTransport: Send + Sync {
    async fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError>;
}

pub struct KianaClient<T> {
    transport: T,
}
```

Client 只能构造协议请求和消费响应，不能读取环境变量或实例化 Core。

- [ ] **Step 3: 实现 DaemonHost**

Daemon 负责把 `RequestEnvelope` 转为 Domain context/intent，调用 Core，再转为 `ResponseEnvelope`。schema 不匹配、空 session、空 project root 和 identity 缺失必须在进入 Core 前拒绝。

- [ ] **Step 4: 写完整 in-process 测试**

测试内定义 `InProcessTransport`，持有 `Arc<DaemonHost>` 并实现 `ClientTransport`：

```text
KianaClient
  -> RequestEnvelope
  -> InProcessTransport
  -> DaemonHost
  -> ControlPlane
  -> MemoryEventLog
  -> ResponseEnvelope
```

断言 `system.architecture` 完成、未知 command blocked、untrusted context fail closed。

- [ ] **Step 5: 运行纵向测试**

Run: `cargo test -p kiana-protocol -p kiana-client -p kiana-daemon`

Expected: PASS。

- [ ] **Step 6: 提交 Client/Daemon**

```bash
git add kiana-protocol kiana-client kiana-daemon
git commit -m "feat: add client protocol and daemon host"
```

### Task 7: 接入现有 CLI 的第一个新路径

**Files:**
- Modify: `kiana-entrypoints/Cargo.toml`
- Modify: `kiana-entrypoints/src/cli.rs`
- Create: `kiana-entrypoints/tests/cli_architecture.rs`

- [ ] **Step 1: 写 CLI 失败测试**

新增测试运行：

```bash
kiana architecture status --json
```

断言退出码为 0，并包含：

```json
{
  "schema": "kiana.architecture-status.v1",
  "control_plane": "kiana-core",
  "composition_root": "kiana-daemon",
  "legacy_edges_remaining": 10
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p kiana-entrypoints --test cli_architecture architecture_status`

Expected: FAIL，当前 CLI 报 unknown command。

- [ ] **Step 3: 实现窄入口 adapter**

在 `main_with_args` 的本地命令分派前识别 `architecture status`。构造 local `DaemonHost`、测试安全的 unavailable Runner 和 fail-closed Capability Broker，通过 `KianaClient` 发起 `system.architecture`，再输出 text 或 JSON。

不得从该函数调用 `create_default_command_registry()`、`create_default_registry()` 或 Provider/Query API。

- [ ] **Step 4: 运行 CLI 测试**

Run: `cargo test -p kiana-entrypoints --test cli_architecture architecture_status`

Expected: PASS。

- [ ] **Step 5: 提交 CLI 接入**

```bash
git add kiana-entrypoints
git commit -m "feat: route architecture status through daemon"
```

### Task 8: 全面验证与计划状态

**Files:**
- Modify: `docs/superpowers/plans/2026-07-17-kiana-control-plane-foundation.md`

- [ ] **Step 1: 运行格式检查**

Run: `cargo fmt --all --check`

Expected: PASS。

- [ ] **Step 2: 运行新架构 focused suite**

Run:

```bash
cargo test -p kiana-domain -p kiana-runner-protocol -p kiana-runner -p kiana-policy \
  -p kiana-gates -p kiana-workflow -p kiana-ports -p kiana-eventlog \
  -p kiana-capability-broker -p kiana-core -p kiana-protocol \
  -p kiana-client -p kiana-daemon -p kiana-entrypoints --locked --offline
```

Expected: PASS。

- [ ] **Step 3: 运行 dependency boundary test**

Run: `cargo test -p kiana-core --test dependency_boundaries --locked --offline -- --nocapture`

Expected: PASS，并打印 10 条已知 legacy edge，不出现额外 edge。

- [ ] **Step 4: 运行 workspace 测试**

Run: `cargo test --workspace --locked --offline --no-fail-fast`

Expected: PASS；若既有未提交代码存在与本计划无关的失败，必须记录精确 test/error，不能把 focused pass 宣称为 workspace pass。

- [ ] **Step 5: 标记计划执行结果**

将本计划 checkbox 按真实结果更新；在文件末尾追加 `Execution Record`，记录 commit、命令、通过项和未通过项。不得使用文档声明替代测试输出。

- [ ] **Step 6: 提交验证记录**

```bash
git add docs/superpowers/plans/2026-07-17-kiana-control-plane-foundation.md
git commit -m "docs: record control plane foundation verification"
```

## 后续独立计划

本计划完成 M0 与 M1。剩余目标由以下独立、可测试计划继续，不得用 foundation 完成替代总体完成：

1. M2 Command migration：逐类迁移 command use case 并删除四条 `kiana-commands` legacy edges。
2. M3 Runner migration：迁移 model loop 和 capability handshake，删除 `entrypoints -> tools/services/query`。
3. M4 Broker/persistence migration：Model、Tool、Query、Secret、Sandbox adapters 与 EventLog/Artifact/Projection 恢复合同。
4. M5 Surface migration：CLI/TUI/SDK/MCP/Remote 全部切到 Client/Protocol，删除全部 dependency exceptions。
