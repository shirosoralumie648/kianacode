# ER-07 replay projector 与 checkpoint 基线

> 快照日期：2026-09-17。本页记录可重建 projection helper 的 source/CI 边界；checkpoint
> 只优化读取，不能授予执行/授权能力，也不替代 EventLog 事实。

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-07`](event-receipt-recovery.md#step-er-07) |
| feature_status | `implemented`（strict checkpoint + generic replay fold） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | EventLog source cursor/events；ProjectionCheckpoint 是可丢弃的只读快照 |
| this step does | pure from-zero/from-checkpoint fold、source cursor/event identity、state/checkpoint digest、schema/version/limit validation、duplicate event id idempotence、checkpoint mismatch fail-closed |
| this step does not | 不让 projection 写 EventStore、签发 permit、改变 policy、触发 Broker/Runner；实际 Run/Invocation/Approval projector checkpoint 持久化和跨进程 recovery 留待 ER-08+ / PD |

## 1. Contract

`kiana-domain::ProjectionCheckpoint` 是 strict versioned DTO，绑定 projector name、完整
source cursor、排序去重的 source event IDs、serialized state、state digest 和 checkpoint
digest。未知字段、schema/version、ID limit、state/digest tamper 均拒绝。

`kiana-core::ReplayProjection` 只接受纯 fold closure。`from_zero` 从初始状态和 EventLog 事件
构造投影；`from_checkpoint` 先验证 projector/cursor/checkpoint，再只应用 cursor 之后的 tail，
重复 event id 跳过，fold 失败返回错误且不修改 source。投影结果可再次生成 checkpoint；没有
checkpoint 或校验失败时调用方必须回退从零重建，不能猜测状态。helper 不持有 EventStore 或
Broker 引用，健康/投影结果不成为 authority。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `projection_checkpoint_is_strict_and_digest_bound` | strict unknown field、schema/version、state digest 和 source identity 错误拒绝 |
| `replay_checkpoint_matches_rebuild_and_skips_duplicate_event_ids` | checkpoint+tail（含重复事件）与从零重建 state/checkpoint digest 相同 |
| `replay_checkpoint_failure_is_fail_closed_and_cannot_authorize` | projector/cursor/fold mismatch 失败，不越过只读边界 |
| `er07_replay_helper_is_read_only_and_checkpoint_driven` | source guard 固定 replay/checkpoint marker 且禁止 EventStore/Broker/授权调用 |

## 3. Proof ceiling and handoff

ER-07 proof ceiling 为 `source`：纯折叠、去重、checkpoint digest 与回退语义已由 CI-only
fixtures 固化；未运行本地测试。真实 projector runner/checkpoint store、checkpoint+tail 崩溃
恢复、Run/Turn terminal 与 Invocation/Approval 投影由 ER-08+、PD-07+ 继续接入。
