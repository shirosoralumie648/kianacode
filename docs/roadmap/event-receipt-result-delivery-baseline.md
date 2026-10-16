# ER-13 result commit 与 result delivery 基线

> 快照日期：2026-10-08。本页记录三路径 result finalizer/delivery 的 source/local 行为边界；
> delivery acknowledgement 不表示外部 handler/provider effect 已 exactly-once。

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-13`](event-receipt-recovery.md#step-er-13) |
| feature_status | `implemented`（统一 finalizer、committed result、single delivery claim） |
| proof_level | `source`（整卡）；当前 committed stop metadata deny fixture 有 `local_behavior` 回执，外部交付仍未证明 |
| authority | ControlPlane EventLog result commit/delivery claim；Runner 只消费已提交 result |
| this step does | direct/Harness/approval-resume shared finalizer、execution.result_committed before delivery、result.delivery_claimed CAS、run terminal/cancel fence、same-result command digest idempotency、callback uncertainty→Unknown |
| this step does not | 不从 delivery claim 重做工具/模型、不把 Runner callback 当 EventLog fact、不证明 callback/external effect exactly-once、不实现 provider receipt/reconcile |

## 1. Contract

`finalize_capability_action` 是三条能力路径共用的 result normalizer/receipt boundary；
`execution.result_committed` 先由 dispatch permit execution stream CAS 提交带 receipt 的
结果，只有该事实成功后 `deliver_capability_result` 才能在 run 未终态/未取消的情况下写
`result.delivery_claimed`。claim 同时绑定 result delivery aggregate、run aggregate、request
ID 和 result digest；同 command/digest 重放不得新增 claim，digest drift/conflict、terminal
run、callback 失败和未知 effect 保守返回 Unknown。delivery 还必须验证顶层
`stop_requested`/`stop_state`/`stop_confirmed` 与 `CapabilityResultReceipt.stop_confirmed` 一致；
伪造停止元数据直接进入 `result_unknown`。Runner 对已消费/错配 result 只产生结构化
失败，不启动第二模型 loop。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `er13_result_delivery_is_committed_before_runner_and_shared_by_three_paths` | source guard 固定 finalizer、execution result commit、delivery CAS→Runner callback 顺序 |
| `contradictory_committed_payloads_or_tampered_receipts_never_deliver` | 12 个 committed source drift 变体（含 forged stop metadata）均不写 delivery claim 且不调用 Runner |

## 3. Proof ceiling and handoff

ER-13 proof ceiling 为 `source`：提交后 delivery 屏障、重复 claim/终态 fence 和三路径共用
入口已固化；stop metadata consistency 已取得 bounded local behavior。真实 Runner callback crash/replay、ER-14 provider receipts、
ER-16 terminal shutdown、跨进程 delivery recovery 与 external/live/physical proof 仍未宣称。
