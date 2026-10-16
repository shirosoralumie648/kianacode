# SC-10 Approval binding 与 Human Inbox 基线

> 快照日期：2026-10-08。本页记录 core source contract 与 focused local fixtures；不把 approval
> DTO 或 inbox projection 写成已执行 effect、durable approval ledger 或人工认证证明。

| 项目 | 记录 |
|---|---|
| roadmap card | [`SC-10`](security-compliance.md#step-sc-10) |
| feature_status | `partial`（strict typed facade 与 CI fixtures；生产 independent-approver path 未接入） |
| proof_level | `source`（整卡）；typed facade 的 scope-drift fixture 有 `local_behavior` 回执，生产接线仍未证明 |
| authority | ControlPlane + existing ApprovalStore CAS；`ApprovalBinding`/`HumanInboxItem` 当前只是未接入的 typed value facade |
| this step does | facade 类型保存 command/target/payload/context digest、principal/session/project/GrantId/epoch/policy/expiry；同一 item 值的状态转换拒绝 self-ID 与 terminal 重复 |
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
