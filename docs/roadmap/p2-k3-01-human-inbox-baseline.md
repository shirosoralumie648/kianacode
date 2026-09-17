# P2-K3-01 Human Inbox 基线

> 快照日期：2026-09-18。本文记录本地 Human Inbox 的统一投影与 resolve 边界；运行时验收由 GitHub Actions 负责，本地不运行测试。

## 统一投影

`kiana-core::human_items` 从受保护的同一请求上下文聚合：

- `pending_approvals` 的 ApprovalView/plan preview；
- Company business review/decision、acceptance review/decision、delivery confirm/reconcile、pending change 和 project cancel；
- Company Incident 与 EventLog-derived failure incident/reconciliation；
- feedback candidates 的 review action。

每一项统一为 `HumanInboxItem { item_id, kind, title, source_ref, run_id, detail, actions }`，kind 明确区分 Approval/Review/Acceptance/Incident/Reconciliation/Feedback。列表按 `item_id` 稳定排序，响应带 items digest revision；detail 中的 approval arguments 使用 redaction，Company/failure 数据仍来自各自 authoritative projection。

## Resolve 与事实边界

`human.resolve` 必须携带当前 `inbox_revision`、存在的 `item_id`/`action_id`、动作允许的 required fields 和 idempotency key；旧 revision、未知 item/action、额外字段、缺必填字段均 fail-closed。resolve 不直接修改 Inbox 或创建第二套决定：Approval 回 `decide_approval_with_proof`，Company 回 `handle_company_command`，failure 回 `failure.reconcile`，feedback 回 `feedback.review`，各路径继续由原有 policy/approval/CAS/EventLog 负责。Human Inbox 只是控制面只读投影和动作路由。

## CI-only 验收

`human_inbox_unifies_control_items_without_a_second_authority` source guard 覆盖六种 item kind、列表 digest/stability、stale/unknown/extra-field rejection 和原 authority routing；SC-10 approval binding fixture 继续覆盖 self-approval、expiry、one-shot consume 与 raw-field rejection。

```text
cargo fmt --all --check
cargo test -p kiana-core --test p2_k3_01_human_inbox --locked -- --test-threads=1
cargo test -p kiana-core --test sc10_approval_binding --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- HumanInboxItem/列表 revision 目前由 Core 从 Approval/Company/EventLog projection 现场重建，尚无独立 durable NotificationStore、read state、outbox、delivery worker、recipient/channel delivery 或跨进程 inbox checkpoint；这些留 NM/PD/UI。
- resolve 动作的最终授权、审批消费、Company revision、failure reconciliation 和 feedback quality gate 仍分别由原 authority 负责；Inbox revision 不等于 approval grant、business acceptance 或外部 outcome。
- 外部 human authentication、multi-tenant recipient policy、retention/withdraw/supersede、通知重试和 live/physical delivery proof 未在本步声明。

