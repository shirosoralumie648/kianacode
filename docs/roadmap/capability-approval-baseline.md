# CAP-06 Approval 最终计划与重校验基线

> 快照日期：2026-09-17。本页记录审批 preview 与执行 material 的边界。Preview 是给人看的
> redacted projection，不是可执行 payload，也不授予 grant/permit。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CAP-06`](capability.md#step-cap-06) |
| feature_status | `implemented`（strict final-plan preview + approval revalidation） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | ApprovalStore protected material + ControlPlane policy/gate/permit CAS |
| this step does | 统一 pending `ApprovalPlanPreview`（operation/capability/risk、redacted arguments、payload/preview/scope/environment digest、expiry、availability），approve/consume 前重做 exact action/scope/role/authority/expiry/cancel 检查 |
| this step does not | 不执行 preview、不把 UI argument 当原始 material、不允许 actor/role/project/epoch/environment drift，不自动批准/重试，不声明外部 effect |

## 1. Contract

`ApprovalPlanPreview` 使用 strict `kiana.approval-plan-preview.v1`，只保存 redacted preview 与
digest。`list_pending_approvals` 从 server-owned pending scope 和 `payload_available` 生成同一
projection；unknown fields、raw secret、preview digest drift 和过大 preview fail-closed。

批准路径先校验 request hash/nonce/expected version、当前 permission profile、run terminal/cancel
状态，再从 `pending_with_proof` 重新取得 protected execution material，重跑
`prepare_capability_action` 与 `authorize_capability_action`，确认 prepared request 与原 subject
完全相等，最后才由 ApprovalStore 写 Approved/Denied fact。Dispatch 仍以 CAP-05 的 permit CAS
消费一次；preview、challenge 和 decision 都不会直接调用 handler。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `approval_preview_binds_final_redacted_plan` | preview 绑定 approval/request/risk/operation，strict round-trip 和 digest |
| `approval_preview_never_becomes_executable_or_accepts_drift` | payload unavailable、raw/unknown field 和 redaction drift 拒绝 |
| `cap06_approval_is_bound_to_final_plan_and_revalidated` | pending preview、protected material、action/policy/gate/expiry/version revalidation source guard |

## 3. Proof ceiling and handoff

CAP-06 proof ceiling 为 `source`：审批展示、material 和执行前重校验已由 CI-only fixtures/source
guard 固化，未运行本地测试。真实 Human Inbox/authn、跨进程 material persistence、approval
recovery/lease/permit atomicity、OS/provider effect 和 live/physical proof 仍留待 CP/ER/PD/INT。
