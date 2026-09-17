# CP-14 result receipt 与 reconciliation 基线

> 快照日期：2026-09-17。本页记录三条能力路径共用的 result dimensions/receipt source 与 CI
> 边界；不把 receipt 写成现实外部效果或 exactly-once 证明。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CP-14`](control-plane.md#step-cp-14) |
| feature_status | `implemented`（strict CapabilityResultReceipt、finalizer/delivery integration） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | ControlPlane result finalizer + EventLog execution/result delivery facts |
| this step does | result digest、process/effect/stop/unknown dimensions、committed receipt marker、三路径 finalizer 与 delivery-after-commit boundary |
| this step does not | 不声称 provider/external effect verification、automatic retry/refund、跨进程 projector、Secret/egress 或 external/live/physical proof |

## 1. Contract

`kiana-domain/src/capabilities.rs` 新增 digest-only `CapabilityResultReceipt`，将
`CapabilityResult` 的 success、process/effect/stop、effect_started/effect_known/zero_effect、
fenced、attempt、execution/invocation IDs 和 committed 标志固定为严格 schema。success 与
unknown/zero-effect、unknown 未 fencing、terminal flag contradictions 均 fail-closed；业务
output 仅通过既有 redacted event payload 保存，receipt 只携带 digest。

`kiana-core` 的 direct/Harness/approval-resume finalizer 在记录 capability result 前生成并
校验 receipt；`dispatch_authorized` 在 execution permit stream 提交
`execution.result_committed` 时一并写入带 identity 的 receipt。结果 event 写入失败或 Cell
settlement 失败仍转为 `ResultUnknown`，unknown 不退款/不重试；既有 `result.delivery_claimed`
CAS 保持 runner delivery 发生在 committed fact 之后。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `result_receipt_separates_success_from_unknown_effect_and_binds_digest` | success 与 fenced unknown 的 dimensions/receipt digest 正确分离 |
| `result_receipt_rejects_inconsistent_flags_and_unknown_fields` | zero-effect/started contradiction 与 raw/unknown field fail-closed |
| `cp14_result_finalizer_keeps_effect_unknown_and_delivery_after_commit` | source guard 固定 finalizer、result persistence/settlement unknown、delivery CAS boundary |

## 3. Proof ceiling and handoff

CP-14 proof ceiling 为 `source`：三路径 result 形状与提交后 delivery 边界已固定，但实际
handler/provider effect verification、durable budget/lease reconciliation、cancel/stop 证据、
跨进程 recovery 和 external/live/physical proof 仍由 CP-15/16/20、ER/PD/DEP 负责。
