# NM-01 notification/messaging contracts baseline

> 快照日期：2026-09-17。本页记录 domain contract 与 schema 形状；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`NM-01`](../roadmap.md#step-nm-01) |
| feature_status | `implemented`（Message/Notification/Subscription/DeliveryAttempt/ActionRef/DeliveryReceipt contracts） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical fact source | contracts are facts/intents only; EventLog/ControlPlane remains the source of authority |
| this step does | six strict schemas, stable IDs, bounded text/scope/TTL, digest/canonical bytes, notification/subscription scope intersection, status transitions and explicit v0 Message upcast |
| this step does not | 不实现 NotificationStore、recipient resolver、projector、outbox、DeliveryWorker、read-state persistence、external channel 或第二消息总线；NM-02+ 负责 |

## 2. Contract rules

`Message` 保存发送方/接收方、kind、bounded body、scope、optional `ActionRefId` 和 content digest；空 recipient、超长正文、NUL、常见 bearer/token/password marker 和 unknown schema/field fail-closed。`upcast_message` 只接受列明的 `kiana.message.v0` shape，明确重命名旧字段后再按 v1 strict DTO 解码；未知 major 不会被静默降级。

`Notification` 必须指向 typed MessageId、具名 recipient、非空 scope、channel、subscription revision 和有限 expiry。`validate_for_subscription` 只允许 notification scope 是服务端订阅 scope 的子集、项目/recipient/channel 完全匹配；客户端字段不能扩权。`Subscription` 的 scope/channel/revision/expiry/status 自带 digest，scope/channel 列表 canonical 排序。

`DeliveryAttempt` 和 `DeliveryReceipt` 仅描述投递事实/结果，带 attempt/notification/subscription/receipt IDs、authority epoch、lease/time monotonicity 和 Unknown 结果；`ActionRef` 只引用命令、目标 revision、scope 与 expiry，执行前仍必须回 ControlPlane 重做授权。所有结构体 `deny_unknown_fields`，digest 使用 domain canonical JSON。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `notification_contracts_round_trip_and_scope_stays_bounded` | strict round-trip/canonical bytes、scope 子集、状态转移和 action ref 合同 |
| `message_rejects_empty_recipient_long_body_and_secret_debug_payload` | 空 recipient、超长正文和 secret marker 在构造阶段拒绝，合法 Debug/Serialize 不含 secret |
| `delivery_attempt_receipt_status_and_ttl_transitions_are_fail_closed` | attempt/receipt 状态、epoch/TTL 和 terminal transition 拒绝 |
| `message_v0_upcast_is_explicit_and_unknown_major_or_field_is_rejected` | v0 显式 upcast；未知 major/field 和 canonical digest drift 拒绝 |
| `notification_contracts_are_domain_owned_and_do_not_create_a_delivery_loop` | schema/ownership/source guard，确认没有 DeliveryWorker 或第二消息执行路径 |

`.github/workflows/nm01-contracts.yml` 在 GitHub runner 执行 domain fixtures、core source guard、fmt 和 domain/ports/protocol/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- Notification/Subscription objects 目前是 domain contracts，不代表已有 durable materializer、cursor、recipient resolver 或 external delivery。
- Secret rejection 复用当前 bounded redaction marker set；未标记的任意高熵秘密、进程内存、provider echo 和外部 channel 仍需后续 SC/INT/PD 证据。
- status transition helpers 不是 CAS 或 lease fence；Unknown 不能被推断为 success，NM-04/07/08/PD/ER 负责提交后投影、OCC、outbox 和恢复。

## 5. Subscription revision fence correction (2026-10-02)

`Notification::validate_for_subscription` now rejects a notification whose embedded
`subscription_revision` differs from the server subscription revision before checking the
recipient, project, scope, or channel intersection. This prevents a stale notification from
being admitted through a later subscription with otherwise matching scope fields. The rejection
is typed as `notification_subscription_revision_mismatch`; no delivery effect or authority is
introduced.

```text
source_snapshot: 18e24fe5 + isolated NM-01 revision-fence patch; kiana-domain/src/notifications.rs; kiana-domain/tests/nm01_contracts.rs; kiana-core/tests/nm01_contracts_guard.rs
worktree_status: branch `step/nm01-audit-20261002`; source and CI fixture patch committed locally; no manifest or lockfile changes; push is owned by the integration agent
command_argv: gh run view 36889127184 --job 110460611632 --log; gh run view 36889127184 --job 110460611756 --log; git diff --check
cwd·environment: isolated worktree; Linux x86_64; GitHub Actions is the only test executor; no local cargo test/build/check/fmt/clippy/smoke command
fixture·cassette: prior CI `nm01_contracts` 4/4, `nm01_contracts_guard` 1/1, `notifications_baseline` 2/2; new `notification_subscription_revision_mismatch` fixture is queued by the integration push
exit_code: prior focused CI targets exit 0; local diff check exit 0; new fixture CI result not yet observed
status change: NM-01 remains partial and fail-closed; stale subscription revisions are explicitly denied
proof-level change: source only for the new fence; prior focused CI remains remote source/fixture evidence; no local_behavior, durable, live, or physical promotion
limitations: no local tests were run; notification materialization, durable subscriptions/read state, outbox/lease/CAS, recipient resolution and external delivery remain later NM/ER/PD/SC work; new CI result is intentionally unawaited
reviewer: Codex NM-01 isolated contract audit; no local runtime test reviewer
```
