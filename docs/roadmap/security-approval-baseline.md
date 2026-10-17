# SC-10 Approval binding 与 Human Inbox 基线

> 快照日期：2026-10-08。本页记录 core source contract 与 focused local fixtures；不把 approval
> DTO 或 inbox projection 写成已执行 effect、durable approval ledger 或人工认证证明。

| 项目 | 记录 |
|---|---|
| roadmap card | [`SC-10`](security-compliance.md#step-sc-10) |
| feature_status | `partial`（strict typed facade + `ApproverScope` / consumption receipt 与 CI fixtures；生产 independent-approver path 未接入） |
| proof_level | `source`（整卡）；typed facade 的 scope-drift/expiry fixture 有 `local_behavior` 回执，生产接线仍未证明 |
| authority | ControlPlane + existing ApprovalStore CAS；`ApprovalBinding`/`ApproverScope`/`HumanInboxItem`/`ApprovalConsumptionReceipt` 当前都是未接入生产路径的 typed value facade |
| this step does | facade 类型保存 command/target/payload/context digest、principal/session/project/GrantId/epoch/policy/expiry；`ApproverScope` 绑定审批人身份+项目/命令集合+epoch+scope 有效期并做自批/跨项目/越权命令/epoch 漂移/过期拒绝；同一 item 值的状态转换拒绝 self-ID 与 terminal 重复；`consume_with_receipt` 产出可校验的一次性消费回执 |
| this step does not | 不实现生产独立 approver 身份/范围、durable inbox projector、外部 human identity、GrantStore、Broker dispatch、第二审批循环或自动重试 |

## 1. Contract

`kiana-core/src/approval_binding.rs` 定义 strict `ApprovalBinding`：字段包括 ApprovalId、subject
RequestId、opaque principal/session/project、command kind、target/payload/context digest、可选
GrantId、authority epoch、policy revision 和 expiry。`for_request` 保存 request 与 RequestContext
摘要；`validate_request` 会比较主体/session/project/request/command/target/payload/epoch/policy/expiry，并重算当前
RequestContext 的 scope digest；它仍不接收 expected GrantScope，因此不能证明 Grant 内容未漂移。
原始 arguments/prompt/provider error 不写入 DTO。该 facade 目前没有生产调用点。

`HumanInboxItem` 是值类型 projection，拥有 Pending→Approved/Denied→Consumed 状态转换；`decide` 拒绝
同一 principal ID，`consume(&self, now_unix_ms)` 会在转换前重新检查 binding expiry，过期返回
`PolicyApprovalExpired`，然后才转换当前副本；它不提供跨副本 CAS，也不生成 grant/permit。
生产 `JournalApprovalStore` 仍要求决定上下文匹配原请求主体并把该 actor 写为 `decided_by`，所以当前
ControlPlane 审批没有独立 approver identity/scope；该差距需要明确的审批身份/范围模型，不能由此 DTO 宣称已解决。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `approval_binding_is_exact_digest_scope_and_strict` | strict serde、同一 request 的精确 request digest、unknown raw fields 拒绝 |
| `approval_binding_rejects_payload_scope_epoch_and_expiry_drift` | request/epoch/expiry 不匹配拒绝；request fixture 同时改变 ID 与 payload，尚未隔离 payload 维度，也没有 context/Grant scope drift fixture |
| `approval_binding_rejects_request_context_scope_drift` | 复用同一 request/context 后仅改变 `path_allow`，重算 scope digest 并返回 `PolicyApprovalBindingMismatch` |
| `human_inbox_rejects_self_approval_and_consumes_once` | value transition 拒绝同 principal ID 与 terminal 重复；不是生产 approver authorization 或跨副本 CAS 证据 |
| `human_inbox_rejects_consumption_after_approval_expiry` | approved item 在 binding expiry 后消费返回 `PolicyApprovalExpired`，不产生 Consumed projection |
| `human_inbox_denial_and_expiry_are_terminal_and_strict` | deny/expiry/unknown field fail-closed |
| `approval_binding_is_exact_and_stays_before_broker_dispatch` | source guard 固定 typed reason 与既有 prepare/decide/validate symbols，无 Broker/执行依赖；不证明 facade 已集成 |

`2026-10-01` GitHub Actions run `36926015057`, job `110583766753` (`Tests (kiana-core-s6/6)`, head
`ad3e664b6d060fcb332f6933b8ca91515c3ee3ca`) 实际执行上述两个 target 并失败：正例因 helper 创建新随机
RequestId 而不能复用创建 binding 的 request；source guard 期待字面量 `AUTH_PRINCIPAL_MISSING`，实现实际使用
`SecurityReasonCode::AuthPrincipalMissing.as_str()`。当前修正只改这两个 CI fixture/source guard；没有该修正的远端
成功回执。随后 `36959912924` / job `110691766266` (`Tests (kiana-core-s6/6)`) 也在该旧 head 上以
相同两项 SC-10 assertion 失败；其中 binding 的另外三个 fixture 通过。修正后的代码尚无远端 CI 回执。

## 3. Focused local receipt (2026-10-08)

RED fixture against the pre-fix implementation accepted a changed `RequestContext.path_allow` and exited 101
(maximum RSS 325448 KiB, 86% CPU, swaps 0). The GREEN implementation extracts `approval_scope_digest` for both
`for_request` and `validate_request`; `cargo test -p kiana-core --test sc10_approval_binding --locked --offline
-- --test-threads=1` passed 5/5 (maximum RSS 1197600 KiB, 95% CPU, swaps 0), and
`cargo test -p kiana-core --test sc10_approval_guard --locked --offline -- --test-threads=1` passed 1/1
(maximum RSS 129088 KiB, 66% CPU, swaps 0). These are bounded local behavior receipts for the typed facade only.

## 4. Proof ceiling and handoff

SC-10 的 proof ceiling 是 `source`。SC-10 仍是 `partial`：approval DTO 未集成生产路径，context scope/Grant
内容虽已在 `validate_request` 中重验 context digest，但 expected GrantScope 仍未接入；生产审批仅按原 requester context 做 ApprovalStore 决定，没有独立 approver
身份或授权范围。Human Inbox 尚未成为跨入口 durable projector；外部人类认证、GrantScope 原子交集、permit CAS、
Secret/redaction/TOCTOU、跨进程 recovery 和 external/live/physical proof 留待 SC-11+ / CP/ER/PD。

## 5. Current-head expiry recheck (2026-10-13)

approved projection 的 `consume` 现在要求调用方提供当前 Unix 毫秒，并在状态转换前执行 binding expiry
检查；这只收紧 typed facade 的过期边界，不代表生产 inbox 已有可信时钟、CAS 或独立审批身份。

```text
source_snapshot: current dirty worktree; kiana-core/src/approval_binding.rs; kiana-core/tests/sc10_approval_binding.rs; kiana-core/tests/sc10_approval_guard.rs
worktree_status: SC-10 row 121 remains 🔄 / partial/source; selected approved-after-expiry denial is local_behavior; shared approval-binding WIP preserved
command_argv: RED exact `human_inbox_rejects_consumption_after_approval_expiry`; GREEN same selector; full `sc10_approval_binding`; source guard `sc10_approval_guard`; all locked/offline through bounded serial launcher
cwd·environment: repository root Linux/bash; one Cargo/compiler worker and one Rust test thread per command, 32 MiB stack, stripped/non-incremental profile; RED max RSS 227772 KiB / CPU 46%; GREEN max RSS 1212512 KiB / CPU 95%; binding target 85452 KiB / 57%; guard 139552 KiB / 63%; all observed swap counters 0
fixture·cassette: an item approved at 100 is consumed at 1000 against expiry 500; old signature could not accept current time (RED 101), corrected method returns `PolicyApprovalExpired`; binding target 6/6 and source guard 3/3
exit_code: RED 101 expected; exact GREEN/binding/guard 0
status_change: approved HumanInbox projection cannot become Consumed at or after binding expiry; selected typed behavior source → local_behavior; no roadmap card promotion
proof-level_change: focused in-memory expiry denial source → local_behavior; no durable/live/physical promotion
limitations: no production call site, independent approver identity/scope, durable inbox projector, CAS/revision persistence, trusted clock, GrantScope intersection, broker dispatch, redaction/TOCTOU or cross-process recovery proof
reviewer: root integration agent with read-only SC-10 audit; focused approval-binding reviewer
```

## 6. Approver scope + consumption receipt (2026-10-17)

`ApproverScope`（`kiana.approver-scope.v1`）是独立于原请求主体的审批范围声明：字段包括已认证
`approver`、允许审批的 `project_ids`（按 UUID 规范排序、非空）、允许的 `command_kinds`
（排序去重、非空）、`authority_epoch` 与 scope 自身 `expires_at_unix_ms`，并带
`scope_digest`。`authorize(&binding, now)` 在状态转换前依次拒绝：scope 过期
（`POLICY_APPROVAL_EXPIRED`）、自身审批（`POLICY_SELF_APPROVAL_FORBIDDEN`）、跨项目
（`AUTH_PROJECT_MISMATCH`）、越权命令（`AUTH_ROLE_MISMATCH`）、authority epoch 漂移
（`POLICY_AUTHORITY_EPOCH_STALE`）。`HumanInboxItem::decide_with_scope` 先用该 scope 授权，
再复用原 `decide` 的自批/终态/过期规则。

`ApprovalConsumptionReceipt`（`kiana.approval-consumption-receipt.v1`）把一次已发生的审批
消费钉成可校验值：approval/request ID、binding/scope 摘要、approver、消费 revision 与消费
时刻。`HumanInboxItem::consume_with_receipt(now)` 先执行 `consume`，再从 Consumed 值构造回执；
重复消费已 Consumed 的项以 `approval_inbox_not_approved` 拒绝，篡改 binding/digest 绑定字段
或加入未知字段的回执在解码/校验阶段 fail closed。

新增/更新的 CI-only fixtures（沿用现有 `kiana-core-s6/6` shard，本地不运行）：

| Fixture | Assertion |
|---|---|
| `approval_binding_rejects_arguments_only_drift` | 相同 RequestId 与命令名、仅变化 arguments 时 `validate_request` 返回 `PolicyApprovalBindingMismatch` |
| `approver_scope_authorizes_matching_binding_and_rejects_drift` | scope 严格 serde；跨项目/越权命令/epoch 漂移/自身审批/scope 过期分别返回稳定码 |
| `approver_scope_is_strict_and_canonical` | 重复项目、空命令集合、篡改 epoch 拒绝 |
| `human_inbox_decide_with_scope_enforces_approver_scope` | scope 授权后才产生 Approved；越权/跨项目/自批/过期在状态转换前拒绝 |
| `human_inbox_consume_emits_one_shot_receipt` | 回执字段与 binding 摘要一致、round-trip、重复消费/篡改/未知字段拒绝 |

本页 proof ceiling 仍是 `source`：`ApproverScope`/回执未被生产 `JournalApprovalStore` 调用，
没有 durable inbox、跨副本 CAS、可信时钟、外部人类认证或 live/physical 证据；这些差距保持
显式开放，由 SC-11+ / CP / ER / PD 承接。
