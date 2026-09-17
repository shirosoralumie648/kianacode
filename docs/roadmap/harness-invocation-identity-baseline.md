# H10 Invocation Identity 基线

> 快照日期：2026-09-17。本页记录完整 assistant batch、稳定请求身份和 checkpoint/恢复接缝；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H10`](harness.md#step-h10) |
| feature_status | `implemented`（batch validation + deterministic request identity） |
| proof_level | `source`；本地仅做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | Runner derives a stable request identity; ControlPlane remains the source of InvocationId/permit/approval authority |
| this step does | whole-batch validation before dispatch、duplicate/empty ID rejection、`(run,turn,step,assistant_item,ordinal)` derived request IDs、explicit mapper identity、checkpoint identity pin |
| this step does not | 不把 provider call ID 当 invocation primary key，不在 Runner 生成 permit/approval，不承诺 external effect exactly-once 或跨进程执行恢复 |

## 1. Contract

`ModelOutput.tool_calls` 先整体通过 `validate_model_calls`，再一次性 map 为 pending batch；任一
调用 schema/工具名无效时，整个 batch 不会发出 `CapabilityRequested`。每个请求 ID 由
`harness.invocation` 派生，输入包含 `run_id`、`turn_id`、`step_id`、provider assistant item
(`call.id`) 和声明序号；同名 call ID 跨 step 不会复用同一身份。

`capability_for_tool_with_request_id` 是 checkpoint/emit 共用的确定性 mapper。恢复时重新验证
catalog、tool schema 和每项 request ID 是否等于同一 tuple 的派生值；不再因恢复重新生成随机
request ID。Core 后续将该 request ID 映射为 InvocationId/ExecutionId 并重新校验 action、permit、
approval 和 authority。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `invalid_second_call_prevents_all_batch_dispatch` | 第二个调用无效时第一个也不派发 |
| `duplicate_call_id_in_one_message_is_rejected` | 一个 assistant batch 内重复 call ID fail-closed |
| `invocation_identity_survives_queue_approval_and_restore` | queued request 在 checkpoint/restore 后保持相同身份 |
| `h10_invocation_identity_is_generated_once_per_complete_batch` | source guard 固定完整批次验证、tuple 派生和 no-regeneration |

## 3. Proof ceiling and handoff

H10 proof ceiling 为 `source`：Runner batch 与稳定 request identity 已接入，checkpoint restore
拒绝 identity/catalog 漂移，CI-only fixtures/source guard 固化。完整 Invocation declaration/dispatch
ledger、结果立即持久化、审批恢复、跨进程 CAS、provider/external/live/physical effect proof 仍留待 H11+ / H12–H14 / CP/PD/INT。
