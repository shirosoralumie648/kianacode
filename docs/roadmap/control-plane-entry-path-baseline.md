# CP-05 capability entry-path parity baseline

> 快照日期：2026-09-16。本文记录普通 capability、Harness 工具和审批恢复三条入口共享
> ControlPlane pipeline 的 source slice；本地不运行测试，运行时夹具仅由 GitHub CI 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CP-05`](control-plane.md#step-cp-05) |
| source snapshot | `b42a36c`（CP-04 ScopeSet 后的干净基线） |
| feature_status | `implemented`（shared prepare/authorize/stage-dispatch/finalize source） |
| proof_level | `source`；静态编译只证明目标可构建，行为断言由远程 CI 提供 |
| canonical path | direct command / Harness tool / approval continuation → `prepare_capability_action` → `authorize_capability_action` → `stage_capability_action` 或 `dispatch_capability_action` → `finalize_capability_action` → EventLog/Receipt |
| this step does | 固定三类入口调用同一 action normalization、policy/gate/hook、approval、Cell lease、permit/Broker、结果/Unknown/finalize helper；审批继续使用当前 decision context |
| this step does not | 不复制模型循环、不让 UI/Provider/Workflow 直接调用 Broker，不把审批缓存或 Runner transcript 当事实，不宣称持久派发/跨进程恢复已完成 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Shared core pipeline | `kiana-core/src/approvals.rs`, `kiana-core/src/capabilities.rs`, `kiana-core/src/dispatch.rs`, `kiana-core/src/lifecycle.rs`, `kiana-core/src/events.rs` | `439a602ee0463d1be33ce144d787e5b7b6cbdb19db7277de2da248aa0cf28b1e`, `f869c545b6aaf24f494bceb2352ef0a1874ff503de55e1bdf951f5414f7b435a`, `464b68ec143743580a201b01d26e7ca8d4f1dc542acb4116edb228a885b65cd0`, `101f3a4b3b0b1e5290df68a4144cacb20c790ab081dc8b380a0e8f40eaadd0c3`, `5a0348f5e1940363119d920244724428af1e1373f692f6424ccfd3b8a6dfe26e` |
| Domain/runner/daemon boundaries | `kiana-domain/src/actions.rs`, `kiana-domain/src/scope.rs`, `kiana-domain/src/capabilities.rs`, `kiana-runner/src/tools.rs`, `kiana-daemon/src/harness_capabilities.rs` | `cd170e01e581015c917fdc0ac2c419149349a5de88164c9f7369c07eecd0a020`, `6cda19bf05066994580b1e5aa434bfc718712fc33b179e053ea6f42126a2f390`, `1e8c6cb45c2875fcc35a9c876cc780bc2011913d15db7d4376a8ced1c117b522`, `2a757b5ba06622622d949cccacbf50f4caf6d38b04c8806611853e6f4498d213`, `342dcbca27709f41821872c60ab199595f51ebc6fa9dccabbaf0b040ae5c46ce` |
| Existing acceptance/source guard | `kiana-core/tests/control_plane.rs`, `kiana-core/tests/cp05_entry_paths_guard.rs`, `kiana-domain/tests/cp03_action_contract.rs`, `.github/workflows/cp05-entry-paths.yml` | `acabe2803a2183321d8138143311a77a014bee8a94600a5546cee618b8400f05`, `964b7b04309bed1acaa0ccec667697fca17d565a1b5b783b8858b9f46021b723`, `86c37f17fca8489d4818ab29f31fdaa3db62aa7fd331744882f28caf046fcaf7`, `79225f291a4c2b858cdd07c6d74bef070a2f46298d27ee59df0a6e3dde12586a` |

这些 hash 只用于本步入口接线漂移复核；静态边界不等于跨路径运行时或持久副作用等价。

## 2. 三条入口矩阵

| 入口 | 入口函数 | 共享步骤 | 入口专属职责 |
|---|---|---|---|
| Direct / Command | `ControlPlane::authorize_and_execute` → `execute_authorized_request` | `prepare_capability_action`、policy/gate、`dispatch_capability_action`、`finalize_capability_action` | 显式 Command scope；无 Run/Turn 时不生成 Harness 身份 |
| Harness tool | `broker_harness_capability`（lifecycle 驱动） | cancellable prepare、server scope/ScopeSet、hook、policy/gate、approval stage、dispatch/finalize、Runner result | 绑定当前 Run/Turn、记录 `run.tool_call`/`run.capability_requested`、处理取消并继续模型循环 |
| Approval continuation | `decide_approval_with_proof` → `resume_approved_invocation` | 重新 prepare、当前 `decision_context` authorize、approval requirements merge、同一 dispatch/finalize、Run result delivery | 读取事件/ApprovalStore 重建 pending，proof/expiry/subject 检查；不直接消费旧 context 或前端决定 |

三条路径都把 `CapabilityRequest` 作为同一个 normalized action 值传入 policy、gate、approval hash、permit 和 handler。`PreparedAction`/action digest、ScopeSet、Cell/Grant/Budget、authority/project identity 仍由服务端生成或验证；入口只决定返回形状、Run 关联和 continuation 语义。

共享 pipeline 的不变量写成：`prepare → authorize → stage/dispatch → finalize`。

## 3. 共享失败与状态口径

| 条件 | 三条入口的共同处理 |
|---|---|
| malformed/unknown operation、schema/path/scope 无效 | prepare fail-closed，追加 redacted blocked/rejected 事实；不进入 policy/Broker effect |
| policy/Gate/Hook Deny | 保留稳定 reason，绝不被其他 Allow 覆盖；不调用 handler |
| policy/Gate/Hook Ask | 合并 requirements，进入同一 Approval stage；普通 Allow 不能消除未满足 Ask |
| approval proof/expiry/subject/action digest 漂移 | continuation 重新 prepare/authorize，拒绝或标记 Unknown；不重放旧 payload |
| cancel/timeout/transport/handler mismatch | `finalize_capability_action` 统一 normalize result；无法确认 effect 保持 `result_unknown`/fenced |
| EventLog/permit/result persistence failure | 不把 handler 返回值写成成功；记录 Unknown 或结构化失败，Receipt 继续从事实投影 |
| direct command | `run_id=None` 时保持显式 Command scope，不创建假的 Run/Turn 或 Harness continuation |

## 4. CI-only fixture catalog

| Fixture | Purpose |
|---|---|
| `cp_three_entry_paths_have_identical_authorization_semantics` | source guard：三条入口共享 helper、审批当前 context 和 Broker permit verifier |
| `approval_resumes_the_stored_request_once_with_monotonic_events` | 远程审批 continuation 只恢复原请求一次，事件序列单调 |
| `mismatched_direct_capability_result_is_unknown` | direct handler 返回错误 request ID 时不伪造成功 |
| `mismatched_harness_capability_result_is_unknown` | Harness handler/result mismatch 进入 Unknown 并停止 Run |
| `cancel_after_awaiting_approval_does_not_resume_the_pending_invocation` | 取消审批中的 Run 不应再 dispatch pending invocation |
| `untrusted_and_unknown_commands_fail_closed_with_events` | direct 命令共享 server trust/operation 拒绝，Broker 调用数为零 |

`.github/workflows/cp05-entry-paths.yml` 先运行 core source guard，再串行执行上述 ControlPlane 入口夹具；只在 GitHub runner 运行测试。CI 结果按用户指示不等待。

## 5. 限制与交接

- 共享 helper 已统一，但三条入口仍有不同的事件序列、Run/Command 返回和 Runner continuation；本步不声称所有 adapter 行为等价，CP-06/CP-13/CP-14 继续收口 atomic transition、permit 和 result delivery。
- ApprovalStore、PendingInvocation、CellRegistry、cancel tracker 和 projection cache 仍有进程内部分；跨进程恢复、持久 invocation ledger、authority epoch 与 replay/reconcile 由 CP-07+、ER/PD/SC 负责。
- 远程 fake Broker/Runner 和已有 integration fixture 不能证明真实 provider/connector/OS/physical effect；没有 live receipt 时保持 `result_unknown` 或 `not_supported`。
- 本地只做 `cargo fmt`、workspace test-target 静态编译和 diff 检查；不运行测试或 smoke，不提升 `local_behavior`、`durable`、`live` 或 `physical`。
