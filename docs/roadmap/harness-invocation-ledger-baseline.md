# H13 Invocation Ledger 基线

> 快照日期：2026-09-18。本页记录 invocation declaration、dispatch、execution、outcome 和 delivery 的持久事实接缝；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H13`](harness.md#step-h13) |
| feature_status | `implemented`（复用 Core/EventLog invocation projection 与 result CAS） |
| proof_level | `source`；本地仅做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | ControlPlane/EventLog/TransitionBatch own declaration, permit, execution and outcome facts; runner only receives a committed result |
| this step does | run.tool_call/capability_requested、capability.decision、execution.prepared、invocation.dispatching/executing、execution.result_committed、result.delivery_claimed 的顺序/receipt/outcome metadata 与 projection/recovery |
| this step does not | 不把单机投影当外部 effect exactly-once，不在 EventLog 提交未知时执行 Broker，不因 dispatch 已开始而猜测结果成功 |

## 1. Contract

`broker_harness_capability` 先记录请求事实和 action digest，再走 policy/gate、DispatchPermit 与
`commit_invocation_executing`；只有提交确认后才调用 Broker。handler 结果先由
`execution.result_committed` 携带 `result_receipt`、`outcome_state`、`outcome_ready` 落账，再以
`result.delivery_claimed` 的 request/digest/CAS 单次送回 runner。EventLog commit unknown/failure
返回 `result_unknown`，不会继续副作用。

`project_invocations` 从事实源重建 invocation；只有 `execution.result_committed` 等可信终态才
形成 result，dispatch/executing 无 outcome 在恢复投影中转为 Unknown，终态 digest 冲突拒绝。结果
交付 claim 与 handler 执行分离，history materialization 可晚于 durable outcome，避免重启重复工具。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `event_append_failure_prevents_dispatch` | execution boundary commit 必须在 Broker `execute_cancellable` 之前 |
| `crash_after_effect_before_outcome_requires_reconciliation` | 仅有 executing/无 result 的投影为 Unknown |
| `persisted_outcome_is_reused_without_reexecuting_tool` | 已落账 result 投影为 Succeeded，delivery 使用单次 claim |
| `h13_invocation_ledger_commits_every_boundary_before_side_effects` | source guard 固定事件顺序、CAS、receipt 和 no-reexecute |

## 3. Proof ceiling and handoff

H13 proof ceiling 为 `source`：Core 的 invocation projection、permit/execute/result commit 和
delivery claim 已补齐 outcome metadata，CI-only fixtures/source guard 固化 crash-window 语义。
跨进程 EventLog/OS crash durability、审批暂停恢复、结果外部 effect reconciliation 与
live/physical exactly-once proof 仍留待 H14+ / CP/PD/INT。
