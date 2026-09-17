# SC-10 Approval binding 与 Human Inbox 基线

> 快照日期：2026-09-17。本页记录 core source contract 与 GitHub CI-only fixtures；不把 approval
> DTO 或 inbox projection 写成已执行 effect、durable approval ledger 或人工认证证明。

| 项目 | 记录 |
|---|---|
| roadmap card | [`SC-10`](security-compliance.md#step-sc-10) |
| feature_status | `implemented`（exact ApprovalBinding、HumanInboxItem、strict fixtures） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | ControlPlane + existing ApprovalStore CAS；binding/inbox 仅是 typed validation/projection |
| this step does | command/target/payload/scope digest、principal/session/project/Grant/epoch/policy/expiry binding、self-approval/terminal/one-shot guards |
| this step does not | 不实现 durable inbox projector、外部 human identity、GrantStore、Broker dispatch、第二审批循环或自动重试 |

## 1. Contract

`kiana-core/src/approval_binding.rs` 新增 strict `ApprovalBinding`：绑定 ApprovalId、subject
RequestId、opaque principal/session/project、command kind、target/payload/scope digest、可选
GrantId、authority epoch、policy revision 和 expiry。`for_request` 只把 request/body/context
摘要写入 binding；`validate_request` 在批准/执行前重新计算并拒绝 payload、target、scope、主体、
project、epoch、policy 或 expiry 漂移，原始 arguments/prompt/provider error 不进入 DTO。

`HumanInboxItem` 是受控的人审 projection，拥有 Pending→Approved/Denied→Consumed 的显式状态，
拒绝 self-approval、terminal 重复决定、过期批准和无 decider 伪造；`consume` 只能消费已批准项一
次。它不生成 grant/permit，实际 ApprovalStore/ControlPlane CAS 与现有 dispatch 路径仍是 authority。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `approval_binding_is_exact_digest_scope_and_strict` | strict serde、typed refs、target/payload/scope digest 与 unknown raw fields 拒绝 |
| `approval_binding_rejects_payload_scope_epoch_and_expiry_drift` | request/epoch/policy/expiry drift 不可复用批准 |
| `human_inbox_rejects_self_approval_and_consumes_once` | self-approval 拒绝，独立 approver 一次批准/消费，重复 terminal 拒绝 |
| `human_inbox_denial_and_expiry_are_terminal_and_strict` | deny/expiry/unknown field fail-closed |
| `approval_binding_is_exact_and_stays_before_broker_dispatch` | source guard 固定现有 prepare/decide/validate path，无 Broker/执行依赖 |

## 3. Proof ceiling and handoff

SC-10 的 proof ceiling 是 `source`：精确 digest binding 与人审状态值对象已固定，但现有
ApprovalStore/Run continuation 仍有内存/JSONL兼容边界，Human Inbox 尚未作为跨入口 durable
projector；外部人类认证、GrantScope 原子交集、permit CAS、Secret/redaction/TOCTOU、跨进程
recovery 和 external/live/physical proof 留待 SC-11+ / CP/ER/PD。
