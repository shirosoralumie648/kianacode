# CP-02 Run/Turn/Invocation/Execution identity baseline

> 快照日期：2026-09-16。本文记录 CP-02 的 typed identity 与 Continue/Resume source slice；本轮不在本地运行测试，domain/core fixtures 只由 GitHub CI 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CP-02`](control-plane.md#step-cp-02) |
| source snapshot | `388dfe1`（CP-01 干净基线） |
| feature status | `implemented`（Run/Turn/Invocation/Execution typed source boundary） |
| proof ceiling | `source`；本地只做格式与 workspace test-target 静态编译，不能提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | versioned wire envelope → DaemonHost → ControlPlane lifecycle → EventLog turn/invocation facts → Broker permit/handler → receipt/projection |
| this step does | typed TurnIdentity/InvocationIdentity、显式 `run.turn.v2`、legacy Continue 与 Resume 边界、完整执行阶段身份关联、拒绝优先迁移护栏 |
| this step does not | 不把 `call_id` 当主键，不让 direct command 虚构 Harness Run，不改变 v1 Continue 的既有兼容形状，不自动恢复或重试 Unknown 执行 |

## 2. Source hashes

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Execution identity contracts | `kiana-domain/src/execution_identity.rs` | `76ab2e53b86adaf8833fbf91180927b8a35c4731e85a69271c56216709142518` |
| Domain registry/exports | `kiana-domain/src/contracts.rs`, `kiana-domain/src/lib.rs` | `ae3831e84e6a5f2d2e1c3cd4ec7233d30cf5799c5ac51a79b6e38ed4ab2ae4e9`, `9ca22a4486a8eba0b9b2647d2d6eb4bc3f0aa8607532f7fc8ff139b10b2984a5` |
| Lifecycle turn boundaries | `kiana-core/src/lifecycle.rs`, `kiana-core/src/recovery.rs` | `101f3a4b3b0b1e5290df68a4144cacb20c790ab081dc8b380a0e8f40eaadd0c3`, `7282ad4c1c689b8971283f15e4f7a3234a402e66a026c8fdc3d34d4afca370ea` |
| Capability request identity references | `kiana-core/src/capabilities.rs` | `0b6a94174b25b4e78069488a73e3226c80c88646be46ed41a99339154134d816` |
| Broker execution identity binding | `kiana-core/src/dispatch.rs` | `464b68ec143743580a201b01d26e7ca8d4f1dc542acb4116edb228a885b65cd0` |
| Protocol/client version boundary | `kiana-protocol/src/lib.rs`, `kiana-client/src/lib.rs` | `46ca3fb7dce1a9c8e7cf97d07cedaac1c17817475b5986b191c1253d8260cd2f`, `c71e92d8f26bab07de92de7036319fb2f2b567436e39d263351c4c0d0923fe3b` |
| Remote fixtures/guard/workflow | `kiana-domain/tests/cp02_execution_identity.rs`, `kiana-core/tests/cp02_execution_guard.rs`, `.github/workflows/cp02-execution-identity.yml` | `09734150ddf9a4499c5cd0db16f1c790f90d3ff647ceb3eddde5c09299aa666b`, `1193df21e4962205dcb0791ee4f6306a3a996f6022fe4da48392d085f1ee9d74`, `158d641e9dd8ef04d3c5001c4580ee6dde008c0f03d3d8108c22d1f481ac47cc` |

后续 CP/迁移步骤若触碰上述文件，必须在同一提交刷新 hash。hash 是源码锚点，不是跨进程认证或现实副作用证明。

## 3. 身份与状态矩阵

| 对象 | 服务端来源 | 允许的转移 | 失败边界 |
|---|---|---|---|
| Session | 已验证 principal/project/assignment | 绑定到多个有序 Turn | wire session/actor 不能越权；跨项目 owner mismatch |
| Turn | server request id 派生 `TurnId` + `TurnIdentity` | `Start`、`NewTurn`、`LegacyContinue`、`Resume` | NewTurn 必须有不同 predecessor；Start/Resume 不得伪造 predecessor |
| Run | ControlPlane 生成/解析 `RunId` | 新 Run 启动；新用户 Continue 只建新 Run | 终态 Run 不被 `run.turn.v2` 复活；未终态新 Turn 返回 `run_not_terminal_use_resume`；Unknown 返回 reconciliation 要求 |
| Invocation | capability request 派生 `InvocationId` | Requested → policy/gate/approval → Prepared | `call_id` 仅关联字段；同一 call_id 在新 Turn 仍是不同 invocation |
| Execution | Broker permit 生成 `ExecutionId` + attempt | Prepared → Dispatching → Executing → committed result | permit/identity/scope/expiry/CAS 不一致 fail-closed；无确认结果保持 Unknown |
| Approval | 既有 approval subject/decision 账本 | Requested → Approved/Denied/Expired/Consumed | 过期、subject/action digest/authority 不匹配不得执行 |
| Lease/预算 | 既有 grant/cell/authority 版本 | reserve → consume → settle/reconcile | 不因相同参数或相同 call_id 自动重试，必须重新取得授权和资源 |

## 4. Continue / Resume 语义

### New Turn

`RequestEnvelope::new_turn` 使用显式 `run.turn.v2` command。ControlPlane 先解析并验证 predecessor，确认前一 Run 已经有终态且不是 `result_unknown`，再以同一 Session 创建新的 Run/Turn，并在 EventLog 写入 `run.predecessor`、typed `TurnIdentity` 和新的 `run.authorized`/`run.prompt`。新 Run 的终态独立投影，旧 Run 只读不变。

### Legacy Continue

`RequestEnvelope::continue_run` 仍保留 `RequestBody::Continue` v1。它写入 `TurnSemantics::LegacyContinue`，复用既有 RunId 和 Runner continuation 行为，以兼容历史调用者。该路径的每轮边界必须显式标记，不能与新 Turn 的“一 Run 一个终态”证明混写。

### Resume

`RequestBody::Resume` 继续复用同一 Run，入口仍需重新检查身份、authority、snapshot、审批和资源状态，并写入 `TurnSemantics::Resume` 的 `run.resume_prepared` 身份事实；CP-02 只固定身份/版本边界，不宣称跨进程恢复已达到 durable/live。Prepared、Dispatching、Executing 缺确认结果时仍是 fenced/Unknown，不能由 UI 或 transcript 猜测成功。

## 5. Invocation/Execution 关联

请求事实记录 `turn_id`、server-derived `invocation_id`、`attempt`、action digest 和经脱敏的参数。Broker 取得真实 `execution_id` 后，在 `execution.prepared`、`invocation.dispatching`、`invocation.executing`、`execution.result_committed` 写入完整 `InvocationIdentity`（run/turn/invocation/execution/call/attempt）。`call_id` 可被模型重复发送，但不会合并不同 Turn 或不同 Execution；projection 继续以 request/invocation 事实和 terminal digest 防止重复、乱序或冲突终态覆盖。

Direct command 没有 Harness Run/Turn，不产生假的 `InvocationIdentity`；它仍走显式 Command scope 和既有 capability action/permit 边界。

## 6. 迁移、兼容和限制

- 旧事件/API 缺少 typed TurnIdentity 时保持可读；reader 只能按确定性 request/run 边界 upcast，无法确定 predecessor 或 turn 的历史记录只能查询，不能获得执行权。
- 旧 Continue 的 v1 DTO、响应和 runner continuation 不被悄悄改写；调用方需主动选择 `run.turn.v2` 才获得新 Turn 语义。
- 旧 capability 事实可依靠 `request_id` 派生 InvocationId 进行只读折叠；真正的 ExecutionId 只信任 Broker permit/执行事实，不接受模型或 UI 提供的值。
- `result_unknown`、重复/乱序终态、缺少 request ID、未知 schema 字段、digest/attempt/身份冲突均 fail-closed；没有对账证据不得自动重试副作用。
- 仍未证明：跨机器 principal/tenant、OS credential、durable snapshot/ledger 完整恢复、真实 provider/connector effect、live/physical 运行和四入口运行时 UAT。

## 7. CI-only fixture catalog

| Fixture | Purpose |
|---|---|
| `turn_identity_distinguishes_start_new_turn_legacy_and_resume` | Turn 语义、predecessor、版本和 round-trip |
| `invocation_identity_keeps_call_id_as_correlation_only` | 重用 call_id 仍生成不同 invocation/execution，attempt/unknown-field/digest 失败关闭 |
| `execution_identity_and_turn_boundaries_are_server_owned` | lifecycle/dispatch/projection/protocol/source guard |

`.github/workflows/cp02-execution-identity.yml` 在 GitHub runner 执行上述 fixtures、`cargo fmt --all --check` 与 `cargo fetch --locked`。本地不运行测试；CI 结果不在本提交中等待或宣称为本地行为证明。
