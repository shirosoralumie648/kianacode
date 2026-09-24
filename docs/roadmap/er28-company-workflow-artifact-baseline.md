# ER-28 CompanyOS、Workflow 与 Artifact 业务引用基线

> 快照日期：2026-09-24。ER-28 的 runtime receipt、evidence bundle、workflow incident 和拒绝夹具由 GitHub Actions 执行；本地不运行测试、构建或检查。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-28`](event-receipt-recovery.md#step-er-28) |
| source snapshot | `origin/master` 在提交前重放的 ER-28 source slice |
| feature_status | `implemented`（runtime receipt/evidence contracts、Company EvidenceBundle binding、workflow observation binding、unknown incident 与 CI wiring） |
| proof_level | `source`；不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | committed workflow reservation → ControlPlane runtime dispatch → EventLog refs → RuntimeReceiptRef / RuntimeEvidenceBundle → business review/acceptance/delivery/closing |

ER-28 保持 runtime terminal 与 CompanyOS 业务决定分离。`RuntimeReceiptRef` 绑定 execution request、terminal status、EventLog references 和 digest；`RuntimeEvidenceBundle` 在需要时再绑定 artifact refs。Company `EvidenceBundle` 现在必须携带与 event refs、artifact refs 一致的 runtime receipt/evidence（旧 bundle 缺字段会在使用时 fail closed）。Workflow node 只接受 ControlPlane 派生的 receipt，不能从 `Completed` 自行推导业务成功。`ResultUnknown` 必须有绑定 instance/node/request 的 `WorkflowIncident`，并继续等待 reconciliation。

Company command、Review、Delivery 和 ClosingReceipt 继续通过已有 Company aggregate、artifact ownership、business acceptance/review/delivery/closing checks；这些边界现在还要求可验证的 runtime receipt，`EvidenceBundle` 绑定 artifact refs 与同一 receipt。runtime receipt 只是证据输入，不替代业务验收或关闭条件。Workflow replay 通过 `load_workflows`/`plan_command` 重放已提交 facts，不重新 dispatch effect。

## 2. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `runtime_receipt_requires_terminal_status_and_bound_event_refs` | receipt 只接受 terminal status，request/status/event refs 和 digest 篡改均拒绝 |
| `evidence_bundle_and_unknown_incident_are_explicitly_bound` | EvidenceBundle 绑定 receipt 与 artifact refs；unknown incident 绑定 workflow instance、node、request 和 runtime events |
| `non_event_and_non_artifact_refs_fail_closed` | receipt 不接受 artifact ref，bundle 不接受 event ref 充当 artifact |
| `unknown_runtime_requires_bound_incident_and_receipt` | Workflow ResultUnknown 没有同一 instance/node/request 的 Incident 时拒绝；有 receipt+incident 才能落盘 |
| `er28_keeps_runtime_receipts_incidents_and_business_closeout_separate` | core/workflow/company source guard 保留 receipt、incident、business closing、replay/no-dispatch 和无第二执行循环边界 |

## 3. Durable boundary and limitations

当前切片只增加可重放的 typed references 和 ControlPlane 绑定。EventLog 仍是事实来源；workflow incidents 会进入 `AutomationState`，但跨进程 projection、独立 incident inbox、artifact 内容持久化和外部 provider reconciliation 仍不在本步骤。业务验收、Review、Delivery、ClosingReceipt 的现实效果不由 runtime `Completed` 证明；没有 EvidenceBundle、acceptance、review、delivery 或未解决 incident 时，close 继续拒绝。

GitHub Actions 运行格式检查、domain/workflow/core fixtures 以及 workspace test-target compile；CI 结果不等待。本地只允许查看 diff 和格式化目标文件，不运行测试、build、check、clippy 或 smoke。没有外部 delivery、live provider、崩溃重启、断电恢复或 physical proof。

## 4. Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```
