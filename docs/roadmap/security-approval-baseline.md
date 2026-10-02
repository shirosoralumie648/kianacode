# SC-10 Approval binding 与 Human Inbox 基线

> 快照日期：2026-10-02。本页记录 core source contract 与 GitHub CI-only fixtures；不把 approval
> DTO 或 inbox projection 写成已执行 effect、durable approval ledger 或人工认证证明。

| 项目 | 记录 |
|---|---|
| roadmap card | [`SC-10`](security-compliance.md#step-sc-10) |
| feature_status | `partial`（strict typed facade 与 CI fixtures；生产 independent-approver path 未接入） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | ControlPlane + existing ApprovalStore CAS；`ApprovalBinding`/`HumanInboxItem` 当前只是未接入的 typed value facade |
| this step does | facade 类型保存 command/target/payload/context digest、principal/session/project/GrantId/epoch/policy/expiry；同一 item 值的状态转换拒绝 self-ID 与 terminal 重复 |
| this step does not | 不实现生产独立 approver 身份/范围、durable inbox projector、外部 human identity、GrantStore、Broker dispatch、第二审批循环或自动重试 |

## 1. Contract

`kiana-core/src/approval_binding.rs` 定义 strict `ApprovalBinding`：字段包括 ApprovalId、subject
RequestId、opaque principal/session/project、command kind、target/payload/context digest、可选
GrantId、authority epoch、policy revision 和 expiry。`for_request` 保存 request 与 RequestContext
摘要；当前 `validate_request` 会比较主体/session/project/request/command/target/payload/epoch/policy/expiry，
但不重算当前 context 的 scope digest，也不接收 expected GrantScope，因此不能证明 scope 或 Grant 内容未漂移。
原始 arguments/prompt/provider error 不写入 DTO。该 facade 目前没有生产调用点。

`HumanInboxItem` 是值类型 projection，拥有 Pending→Approved/Denied→Consumed 状态转换；`decide` 拒绝
同一 principal ID，`consume(&self)` 只转换当前副本，不提供跨副本 CAS。它不生成 grant/permit。
生产 `JournalApprovalStore` 仍要求决定上下文匹配原请求主体并把该 actor 写为 `decided_by`，所以当前
ControlPlane 审批没有独立 approver identity/scope；该差距需要明确的审批身份/范围模型，不能由此 DTO 宣称已解决。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `approval_binding_is_exact_digest_scope_and_strict` | strict serde、同一 request 的精确 request digest、unknown raw fields 拒绝 |
| `approval_binding_rejects_payload_scope_epoch_and_expiry_drift` | request/epoch/expiry 不匹配拒绝；request fixture 同时改变 ID 与 payload，尚未隔离 payload 维度，也没有 context/Grant scope drift fixture |
| `human_inbox_rejects_self_approval_and_consumes_once` | value transition 拒绝同 principal ID 与 terminal 重复；不是生产 approver authorization 或跨副本 CAS 证据 |
| `human_inbox_denial_and_expiry_are_terminal_and_strict` | deny/expiry/unknown field fail-closed |
| `approval_binding_is_exact_and_stays_before_broker_dispatch` | source guard 固定 typed reason 与既有 prepare/decide/validate symbols，无 Broker/执行依赖；不证明 facade 已集成 |

`2026-10-01` GitHub Actions run `36926015057`, job `110583766753` (`Tests (kiana-core-s6/6)`, head
`ad3e664b6d060fcb332f6933b8ca91515c3ee3ca`) 实际执行上述两个 target 并失败：正例因 helper 创建新随机
RequestId 而不能复用创建 binding 的 request；source guard 期待字面量 `AUTH_PRINCIPAL_MISSING`，实现实际使用
`SecurityReasonCode::AuthPrincipalMissing.as_str()`。当前修正只改这两个 CI fixture/source guard；没有该修正的远端
成功回执。随后 `36959912924` / job `110691766266` (`Tests (kiana-core-s6/6)`) 也在该旧 head 上以
相同两项 SC-10 assertion 失败；其中 binding 的另外三个 fixture 通过。修正后的代码尚无远端 CI 回执。

## 3. Proof ceiling and handoff

SC-10 的 proof ceiling 是 `source`。SC-10 仍是 `partial`：approval DTO 未集成生产路径，context scope/Grant
内容没有在 `validate_request` 中重验；生产审批仅按原 requester context 做 ApprovalStore 决定，没有独立 approver
身份或授权范围。Human Inbox 尚未成为跨入口 durable projector；外部人类认证、GrantScope 原子交集、permit CAS、
Secret/redaction/TOCTOU、跨进程 recovery 和 external/live/physical proof 留待 SC-11+ / CP/ER/PD。
