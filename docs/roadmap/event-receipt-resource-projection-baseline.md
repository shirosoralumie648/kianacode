# ER-10 Approval/Budget/Lease/Cell recovery projection 基线

> 快照日期：2026-09-17。本页记录从 EventLog 重建 pending/resource 读模型的 source/CI 边界；
> snapshot 是可丢弃的查询优化，不是授权或资源领取事实。

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-10`](event-receipt-recovery.md#step-er-10) |
| feature_status | `implemented`（RecoveryResourceSnapshot + source reducer） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | EventLog approval/budget/lease/cell facts；ControlPlane reducer 只读 |
| this step does | strict pending approval state、budget reservation/settlement linkage、resource lease issued/released/fenced set、Cell active/fenced lifecycle、source cursor/event IDs、cache-miss/empty-source distinction |
| this step does not | 不自动批准/消费、创建 permit、领取 OS lock、重建 volatile raw payload、替代 Cell/Lease durable store 或提供 external/live/physical proof |

## 1. Contract

`kiana-domain::RecoveryResourceSnapshot` 绑定 source cursor/event IDs 和 bounded sorted
approval/budget/lease/cell IDs，带 schema/version/digest；unknown fields、nil/duplicate/limit/
digest mismatch fail-closed。

`kiana-core::project_recovery_resources` 按 EventLog 顺序去重并折叠：approval 必须从 staged
开始遵循 typed transition，只有 Active 进入 pending；`model.reserved` 必须携带有效
`BudgetReservationFact`，settlement 必须匹配现存 reservation，结算后释放 reserved lease；
lease/cell 事件只更新读模型，quarantined/failed/cancel-requested cell 保持 fenced。`ControlPlane`
的 `recovery_resources` 将 unsupported source 与真实空/损坏源区分，绝不把 cache miss 当授权。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `recovery_projection_rebuilds_pending_budget_lease_and_cell_state` | pending Active approval、未结算 budget、active lease/cell 与 fenced cell 从 facts 重建 |
| `recovery_projection_settles_budget_and_rejects_orphan_settlement` | settlement 移除 reservation；无 reservation 的 settlement 拒绝 |
| `recovery_projection_duplicate_events_are_idempotent_and_unsupported_source_is_distinct` | duplicate event ID 不重复资源；空源显式 source_empty |
| `er10_resource_projection_is_read_only_and_cache_miss_safe` | source guard 固定 typed folds、missing/conflict/errors、无 Broker/执行边界 |

## 3. Proof ceiling and handoff

ER-10 proof ceiling 为 `source`：pending/budget/lease/cell 的可重建读模型合同已由 CI-only
fixtures 固化；未运行本地测试。真实 durable projector/checkpoint、OS lease 重建、跨进程
fencing、审批/预算/lease 原子消费、backup/retention 与 external/live/physical proof 留待
ER-11+、PD、CP-16/17/20。
