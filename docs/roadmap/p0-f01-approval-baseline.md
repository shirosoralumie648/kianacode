# P0-F-01 审批一等请求/应答基线

> 快照日期：2026-09-18。本页回填统一审批 pending/answer 接线；本地不运行测试，验收夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`P0-F-01`](../roadmap.md#step-p0-f-01) |
| feature_status | `implemented`（protocol/core/client/daemon/entrypoints source；CI-only surface guard） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | ControlPlane/ApprovalStore own the pending challenge and available decisions; surfaces only render and submit typed envelopes |
| this step does | shared `pending_approvals` query and proof-bound `approval_decision` are reused by Workbench/TTY, Web and one-shot product command; challenge ID/hash/nonce and expiry remain server-provided |
| this step does not | 不自动批准、不让 UI 直接执行 capability、不创建第二审批或模型循环；decision durability/single-use and restart continuation are P0-F-02/F-03 |

## 1. Contract

Every surface asks the same DaemonHost for the server-owned pending approval view and submits the
same typed approval envelope. The Workbench and Web paths use the shared host helper; the one-shot
CLI constructs the same `RequestEnvelope` directly. A missing/expired challenge or unavailable
decision is surfaced as a rejection, never interpreted as consent.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `three_surfaces_list_and_answer_the_same_pending_approval` | Workbench/TTY, Web and one-shot CLI all list pending approvals and submit the same proof-bound decision path |
| `approval_surfaces_do_not_auto_approve_or_create_a_second_loop` | no surface embeds automatic approval or a second async execution loop |

## 3. Proof ceiling and handoff

P0-F-01 proof ceiling is `source`: shared pending/answer wire and three-surface routing are
explicit. Durable decision facts, single-use OCC, expiry/replay fencing and restart continuation
remain P0-F-02/P0-F-03 and later EventLog/PD work.
