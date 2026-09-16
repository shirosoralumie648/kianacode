# H02 Harness identity and lifecycle baseline

> 快照日期：2026-09-16。本文记录 Session→Turn→Run→Step→ModelAttempt 的身份边界；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`H02`](harness.md#step-h02) |
| source snapshot | `8d6f3a3`（CAP-04 完成后的干净基线） |
| feature_status | `implemented`（typed identity source + runner/core/protocol wiring） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | protocol Start/Continue → DaemonHost → ControlPlane TurnIdentity → Runner Start(turn) → StepIdentity/ModelAttemptIdentity → EventLog projection |
| this step does | 注册 `StepId`/`ModelAttemptId`，建立 Step/ModelAttempt typed identity，Start wire 传递 server-owned TurnId，Runner model facts 绑定 turn/step/attempt，保留 legacy Start/Continue reader 兼容 |
| this step does not | 不引入第二个 runner loop、Session queue service、durable RunSnapshot/InvocationLedger、跨进程 driver ownership 或真实 Provider/live effect |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| IDs/identity contracts | `kiana-domain/src/ids.rs`, `kiana-domain/src/contracts.rs`, `kiana-domain/src/execution_identity.rs`, `kiana-domain/src/event_contracts.rs` | `d41afd5e1402b2d85ce6e1df0b3032da185f08ae075c4f4626829f680a176529`, `faa1be7e86fbcddbda2ec5aa3f57737714b09703e0e4f80d5a5ee43feb72bd35`, `97346b02d4924fc8a415573cf929b4079308e0bacf117cb715b3d6ee928086f9`, `5ba2b23e9999bb07ea39d26fe0672a19f1e519c262ae165c222f9cb6be24f4cb` |
| Model/runner identity | `kiana-domain/src/model.rs`, `kiana-domain/src/observability.rs`, `kiana-runner/src/harness.rs`, `kiana-runner-protocol/src/lib.rs`, `kiana-core/src/lifecycle.rs`, `kiana-core/src/model_attempt_projection.rs` | `410574cb3ccbe5914d8f0fa1fdb9e46c367564980793ee3e297fd4661fdd8cf1`, `0725912e1cdfa680e6eefebc2c75c7a97b132f755ecd3a4198d0e78b25a71d34`, `774a61335c73839d470326573547efbff89fe690a605314340601bc2f7db641a`, `ba2f98f8e7bc281ea6ead356b70e6813e4fc6dffb7fc094dda99afe6ab578f6a`, `4ace9a24e885c6b91db29e20d83169a5ce1c51ac048164bdb20082c7463c2066`, `0ee33d0c6b334facd2030b93c97042b22ce0ba6b7fe3d23fd963b15ac4760a2a` |
| Protocol/adapter exports | `kiana-protocol/src/lib.rs`, `kiana-daemon/src/harness_skills.rs` | `75bd6034160454032993c50a6e16a80fd9cc3dbc76eefe533b31182f8ff5acca`, `8da70c8b160b8583936ebf15a3c297817c3343518aa08156917c8ba92d4ee783` |
| Provider compatibility fixture | `kiana-provider/tests/oa08_provider_telemetry.rs` | `4e9e70ae741766f5e1cbd89203885165f7a9b0dd4a4f6b07f97a9584bdbfe8eb` |
| Fixtures/workflow | `kiana-domain/tests/h02_identity.rs`, `kiana-core/tests/h02_lifecycle.rs`, `kiana-runner-protocol/tests/h02_wire.rs`, `.github/workflows/h02-lifecycle.yml` | `836be77c5165bc7d4cb0defa7bd70c40d36ec59f65a72bdcd00cfe3879ae2d19`, `67ad9f92caf0ee57114fac39e0ccef74ecad9373086557b697b7713e5ed4fe04`, `0427fcd25bb9719d7f56e6ea53b001b9e19e90c073f917b991d75dc30f02d9be`, `991186c2f73525b883dbfc053856494d4ca2e9891f04f1ae1ab7fffd80c91ca1` |

hash 只用于 H02 源码漂移复核，不构成运行时、持久化或 Provider 证明。

## 2. Identity contract

- `TurnIdentity` 继续区分 Start、native NewTurn、legacy Continue 和 Resume；终态 Run 只能被新 `run.turn.v2` 以 predecessor 链接，不能把旧 Run 当无限聊天容器。
- `StepIdentity` 绑定 `run_id + turn_id + step_id + step_number`，每个 Harness model step 只生成一个 StepId；同一步的 Provider retries 共享该 StepId。
- `ModelAttemptIdentity` 绑定 Step、server-owned `ModelAttemptId`、model call correlation ID 和 1-based attempt；provider `attempt_id` 仍保留为预算/兼容请求 ID，不能替代 typed identity。
- `ModelAttemptRecord` 以可选字段承载新 step/model-attempt IDs；legacy `run.model_turn` 缺失这些字段仍可只读投影，但不自动提升为新 authority。

## 3. Runner and lifecycle wiring

`RunnerCommand::Start` 新增可选 `turn_id`。ControlPlane 的 native Start/Continue path 使用带 TurnId 的 constructor；旧 serialized Start 缺字段仍解码为 `None`。Daemon skill wrapper 只传播该字段，不重造身份。Harness 将 TurnId 保存在 ActiveRun/checkpoint，在每个 model step 生成 StepIdentity，在每个 provider attempt 生成 ModelAttemptIdentity，并将其作为受限 metadata 写入 `run.model_turn`；完整 wire prompt、headers、provider raw response 仍不进入 EventLog。

Steer/Inject 继续写入同一 Run 的 bounded inbox；approval recovery/Resume 仍由 ControlPlane 重建并重验，不由 Runner 自行恢复权限。Session 级多输入排队与跨进程 driver ownership 仍是 H03/H18+ 的后续范围。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `duplicate_run_terminal_is_rejected` | 同一 Run 的冲突终态由 Run projection fail-closed |
| `late_result_cannot_complete_a_new_turn` | foreign Run 的迟到 capability result 不能污染当前 Turn/Run projection |
| `continue_closed_run_requires_new_admission` | native continue 在 closed/unknown predecessor 上要求 terminal/reconcile gate，不能盲目复活 |
| `native_continue_creates_new_run_while_legacy_contract_is_preserved` | 新语义创建 fresh Run/Turn，legacy wire/read path 保持显式兼容 |
| `step_identity_binds_session_run_turn_scope` | Step identity digest 覆盖 run/turn/step/number，篡改拒绝 |
| `model_attempt_identity_is_unique_per_step_and_attempt` | 每个 model attempt 有独立 typed ID/digest，attempt=0 拒绝 |
| `native_start_carries_optional_server_turn_and_legacy_start_stays_readable` | Start wire 可带 server TurnId，缺字段旧 payload 仍可读 |

`.github/workflows/h02-lifecycle.yml` 在 GitHub runner 执行 domain/core/runner-protocol fixtures 与 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 5. 限制与交接

- 当前 Session binding、RunSnapshot、Runner maps/inbox 仍有进程内部分；本步不宣称 durable cross-process queue/driver lease 或 power-loss recovery。
- StepId/ModelAttemptId 由可信 Runner/ControlPlane 生成并进入 EventLog metadata；provider/model `call_id` 仍只是关联，不授予 capability 权限。
- 旧事件缺 Step/ModelAttempt 字段仍可 query/read；upcast、attempt retries/claim ledger、H03 单一状态驱动器和 H10 全程稳定 invocation identity 继续后移。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
