# NM-02 communication lifecycle baseline

> 快照日期：2026-09-17。本页记录七类 CommunicationMessage 的生命周期事实；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`NM-02`](../roadmap.md#step-nm-02) |
| feature_status | `implemented`（typed lifecycle + ControlPlane ACK/reject/escalate commands） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | CommandIntent → ControlPlane communication handler → EventLog `communication` aggregate → notification/UI projections |
| this step does | 七类消息的发送 lifecycle、定向 Handoff ACK/reject、Incident escalation with evidence、server sender/role checks、lifecycle digest and committed aggregate facts |
| this step does not | 不从消息文本直接执行 Company/Capability 命令，不把 ACK 当 approval/grant，不启动 child/dispatch，不写 shared transcript，不实现 notification delivery worker；NM-03+ 负责 |

## 2. Lifecycle rules

`CommunicationMessage` 的七类 kind（Chat/Command/Handoff/Decision/StatusReport/Evidence/Incident）都保持 `grants_authority=false`。Chat 不能带 action/ACK；Command/Decision 的 action reference 只是后续 ControlPlane 命令的非权威提示，Evidence/StatusReport 文本不会改变 grant。常见 secret marker 在消息与 lifecycle reason/evidence 中 fail-closed。

发送时 core 先校验 trusted project、已登记 server role、actor 与 message sender 一致，再由 server 构造 `CommunicationLifecycleEvent(Sent)`，把 `message_id` 作为 `communication` EventLog aggregate key。ACK/reject 只能由具名 Handoff recipient 对 `Sent` 状态提交，且必须有 reason；状态终止后重复 ACK/相反 ACK 会被拒绝。ACK 仅写 committed fact，绝不调用 `handle_company_command` 或 `authorize_and_execute`。

Incident 可由原 sender 以 `communication.escalate` 提交带 reason/evidence_refs 的 `Escalated` fact；非 Incident 或伪造 sender 被拒绝。`communication.handoff_acknowledged`、`communication.handoff_rejected`、`communication.incident_escalated` 已登记为 required event kinds，payload 受 allow-list 约束；现有旧 communication events 继续可读。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `handoff_lifecycle_is_directed_and_ack_or_reject_is_terminal` | Handoff recipient/ACK/reject/reason/terminal 规则，缺 recipient 与重复终态拒绝 |
| `message_kinds_never_grant_authority_and_incident_escalation_is_evidenced` | Command/Decision/Evidence 无 authority，Incident escalation 必须是 Incident 且带证据 |
| `communication_lifecycle_uses_eventlog_and_never_dispatches_from_message_text` | core route 只追加 EventLog，sender/role/aggregate/source guard，无第二执行路径 |

GitHub 的统一 `.github/workflows/ci.yml` matrix 在 runner 执行 `nm02_lifecycle` domain fixture、`nm02_lifecycle_guard` core source guard，以及 workspace gates；此前的 `.github/workflows/nm02-lifecycle.yml` 已在 workflow consolidation 中删除。本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 当前 `SwarmState`/Company dispatcher 不自动消费 ACK；是否 dispatch 仍需后续 packet/company command 及独立 authorization/approval/lease 检查。
- `communication` aggregate 的新事件已按 message_id 分流，但旧 request-aggregate communication facts 不会被自动重写；跨进程 replay/projector、cursor、notification materialization 和 outbox 由 NM-03/04/07/08、ER/PD 负责。
- EventLog commit 证明的是消息事实，不证明现实收件人已读或外部 channel delivery；Unknown/ack reconciliation 仍保持 fail-closed。

## 5. 2026-10-02 source correction

远端 `ci` run `36677090825`（`c221c211`）暴露了三处本步边界问题：终态 Handoff transition 的错误分类被 ACK 专用检查遮蔽；core source guard 把注释中的禁用调用名当成执行路径；Incident escalation 的 `evidence_refs` 可缺失或静默丢弃非字符串值。修复后，transition 校验先保留终态/类型错误的稳定分类，Incident 必须至少有一个 evidence ref，ControlPlane 对 evidence 数组执行严格字符串解析，source guard 只检查去除注释后的可执行文本并保留静默丢弃的负向守卫。

本次修复仍只覆盖 NM-02 的 domain/core lifecycle 边界。统一 CI 的完整结果不作为本地证明；roadmap 状态保持 `🔄`，证明上限保持 `proof_level=source`。

## 6. Focused lane restoration (2026-10-03)

The historical NM-02 workflow was removed during unified workflow consolidation. A manual focused
lane is restored from the historical commands: it runs the domain lifecycle fixture, the Core source
guard and target-scoped `--no-run` compilation for both targets. The lane uses the current
terminal-ordering, strict Incident evidence and comment-filtered no-dispatch corrections from
`f0d47d8f`; a fresh GitHub receipt is required before any status or proof-level promotion.

## 7. Focused CI receipt (2026-10-03)

Run `37134681304` at head `b541dc13` completed successfully. Formatting, both NM-02 lifecycle
domain/core fixture targets, and both target-scoped `--no-run` compilation steps passed after the
terminal-ordering, strict Incident evidence and comment-filtered source-guard corrections. NM-02
remains `🔄` / `partial/source`; ACK does not drive Company/Swarm dispatch and notification
materialization, delivery, read state and cross-process recovery remain later scope.

## 8. Focused acceptance closeout (2026-10-04)

The scoped NM-02 acceptance lane is complete at `implemented/source`: run `37134681304` passed
the lifecycle domain target, Core guard and both target-scoped compile steps. Unified workspace CI
is not claimed green; ACK dispatch, notification materialization, delivery/read state and
cross-process recovery remain later work.
