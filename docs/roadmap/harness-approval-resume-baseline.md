# H14 审批暂停与原调用恢复基线

> 快照日期：2026-09-18。本页记录 Harness 在审批等待期间的原调用绑定和同进程恢复接缝；本地不运行测试，夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H14`](harness.md#step-h14) |
| feature_status | `implemented`（同进程暂停/恢复与绑定校验；跨进程恢复仍由 H24/H25 负责） |
| proof_level | `source`；本地仅做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | ControlPlane/EventLog/ApprovalStore own challenge, decision and consumption; Runner only owns the ordered checkpoint queue |
| this step does | 将 Run/Turn/Step/Invocation、原请求参数摘要、tool catalog、authority epoch、owner/session、sandbox 和待处理批次摘要绑定到 pending invocation；审批回复带 proof，获批前重新准备/授权/permit，获批后沿原 `CapabilityResult`→`drive_run` 接缝继续 |
| this step does not | 不把 UI/transcript 当恢复事实，不把审批预览当执行载荷，不声称跨进程/掉电恢复或外部 effect exactly-once |

## 1. Contract

Runner 在声明完整工具批次后，把当前 Turn/Step 和有序 pending queue 摘要写入每个请求。Core
在 `InvocationResumeBinding` 中固化 invocation/request、参数 digest、目录 digest、authority
epoch、owner/session、project/sandbox digest 及原事件请求 ID；该绑定与 `approval.requested`、
`run.awaiting_approval` 和 `RunSnapshot` 同步。等待审批返回 `AwaitingApproval`，不会向模型伪造
失败观察。

审批决定必须继续携带 approval ID、request hash、nonce 和有效期证明。`decide_approval` 的
single-use command/version CAS 先处理重复决定；获批后 `resume_approved_invocation` 重新检查
绑定、protected material、path/scope/authority/policy/budget/cancellation，再取得同一
ControlPlane dispatch permit。绑定变化、过期、取消、拒绝或消费冲突均收口 pending/run，绝不
直接派发。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `expired_or_changed_approval_never_dispatches` | 过期、hash/nonce/authority/参数/绑定漂移在 Broker 前被拒绝 |
| `duplicate_approval_reply_executes_at_most_once` | durable decision、single-use consumption、command replay 和 pending cleanup 只允许一次 effect |
| `approval_resume_keeps_turn_step_and_invocation_ids` | 原 Turn/Step/Invocation、批次摘要与 `drive_run` 接续使用同一身份 |
| domain binding fixtures | binding digest round-trip；参数或 owner 变化 fail-closed |

## 3. Proof ceiling and handoff

H14 proof ceiling 为 `source`：同进程审批暂停、typed binding、保护材料重载、重新 admission 和
原 Runner result delivery 已接线，拒绝/重复/恢复语义由 CI-only fixtures 固化。当前
`PendingInvocation` 与 Runner checkpoint 仍由进程内 host 持有；跨进程 checkpoint hydration、
掉电后的 CAS/reconcile、外部 provider effect 观察和 live/physical proof 留待 H24/H25、PD/INT。
