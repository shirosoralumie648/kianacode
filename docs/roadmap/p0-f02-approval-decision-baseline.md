# P0-F-02 审批决定事件与单次消费基线

> 快照日期：2026-09-18。本页回填审批决定事实与单次消费实现；本地不运行测试，验收夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`P0-F-02`](../roadmap.md#step-p0-f-02) |
| feature_status | `implemented`（core/daemon/domain/protocol source；CI-only source guard） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | ApprovalStore/Journals own decision and consumption facts; the original capability request identity remains the execution subject |
| this step does | ControlPlane writes approved/denied facts with actor/scope/subject/expiry/proof, JournalApprovalStore validates typed decision/consumption facts and OCC, and repeated/expired/conflicting decisions fail closed |
| this step does not | 不把 decision request_id 替换原执行 request_id，不把 deny 伪装成 cancel，也不让 preview/receipt 直接执行；跨进程 restart continuation is P0-F-03/PD |

## 1. Contract

Approval decisions are durable facts separate from the original capability request. The journal
binds ApprovalId, subject request/hash/nonce, actor and scope, authority/expiry and expected version;
Approved is not Consumed until the existing ControlPlane dispatch/continuation path consumes the
single-use record. A second decision, expired proof or changed subject returns a structured refusal.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `approval_decision_is_durable_and_single_use` | approved/denied/consumed facts, original subject identity, expiry/proof and already-consumed refusal are present in the shared journal path |
| `expired_or_conflicting_decisions_fail_closed_without_reusing_execution_identity` | expiry/conflict errors are explicit and do not replace the original execution request identity |

## 3. Proof ceiling and handoff

P0-F-02 proof ceiling is `source`: typed decision/consumption facts, proof validation, expiry and
single-use OCC are explicit. Durable crash recovery, external approver authentication, pending
continuation hydration and power-loss reconciliation remain P0-F-03/SC/PD work.
