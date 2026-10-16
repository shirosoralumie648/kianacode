# ER-09 Invocation/Execution/Attempt 投影基线

> 快照日期：2026-09-17。本页记录能力执行投影的 source/CI 边界；投影只解释已提交事实，
> 不会调用 Broker、Runner 或把诊断结果升格为授权。

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-09`](event-receipt-recovery.md#step-er-09) |
| feature_status | `implemented`（InvocationProjection 与 CapabilityAttemptRecord reducer） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | EventLog invocation/execution/result/cancellation facts；projection/cache 为可重建只读视图 |
| this step does | per-request invocation state、per-attempt action/permit/executor identity、effect_known、stop_confirmed、fencing、result digest/refs、terminal precedence、unknown on dispatch-without-result、retry attempt fold |
| this step does not | 不重试 Unknown、不合并冲突 terminal、不把 Broker success 当外部 effect proof、不写 EventStore/授权；Provider/外部对账留待 ER-14/CP-20 |

## 1. Contract

`project_invocations` 按 run stream version 或 durable order 折叠 request/approval/permit/
dispatch/executing/result facts，以 request identity 建立 InvocationProjection。action/args
fingerprint、operation、approval、request payload 改变会 conflict；已 dispatch/executing
但缺 committed result 的 attempt 在重建后变为 `Unknown`，而不是成功。

`project_capability_attempts` 为每个 `(request_id, attempt)` 生成严格
`CapabilityAttemptRecord`，保留 run/turn/invocation/execution IDs、action digest、admission/
approval/effect/stop/fenced、source cursor/event IDs、bounded attributes 和 result status。
terminal digest 不同、foreign result、attempt identity/action digest mismatch 均 fail-closed；
重复 event ID 的完全相同重放保持幂等；同一 event ID 的任一 envelope/payload 字段漂移返回
`capability_attempt_event_identity_conflict`，late/duplicate delivery 不会静默选择先到内容，
也不会合并第二个 terminal。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `invocation_projection_marks_dispatch_without_result_unknown` | Prepared/dispatching/executing 缺结果重建为 Unknown |
| `invocation_projection_rejects_attempt_digest_and_terminal_conflicts` | action digest drift 与多个不同 terminal result 不合并 |
| `capability_attempt_projection_preserves_unknown_stop_and_retry_attempts` | effect/stop/fenced dimensions 保守表达，attempt 递增各自独立 |
| `capability_attempt_projection_rejects_event_id_content_drift_but_allows_exact_replay` | 同 event ID 的 exact replay 幂等，operation/payload 漂移 fail-closed |
| `er09_projection_exposes_only_typed_attempt_identity` | source guard 固定 terminal digest、foreign result、effect/stop/source IDs 和无执行边界 |

## 3. Proof ceiling and handoff

ER-09 proof ceiling 为 `source`：Invocation/Attempt reducer 的身份、终态优先级和 Unknown
语义由 CI-only fixtures 固化；本地 focused run 已验证 event identity drift denial，但整卡
仍保持 source proof。真实 handler/provider effect observation、外部 receipt/reconciliation、
持久投影 checkpoint 与跨进程 recovery 留待 ER-14/ER-20/PD。

## 4. Event identity replay fence (2026-10-08)

`project_capability_attempts` previously used only a `HashSet<EventId>` and silently ignored a
second event with the same ID, even when its operation or payload differed. The reducer now retains
the first `RuntimeEvent` for comparison: exact structural replay remains idempotent, while any
identity reuse with changed envelope or payload returns
`capability_attempt_event_identity_conflict` before state folding.

The focused RED accepted the forged operation drift (exit 101, max RSS 4,053,264 KiB, 85% CPU,
no swap). GREEN passed the new drift/exact-replay fixture 1/1 (exit 0, max RSS 1,198,548 KiB,
93% CPU, no swap); the full `er09_invocation_projection` target passed 8/8 (max RSS 85,748 KiB,
59% CPU, no swap). This selected local behavior does not establish durable projector checkpoints,
cross-process recovery, external effect reconciliation, or live/physical proof.
