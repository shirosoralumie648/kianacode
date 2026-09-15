# NM-00 Notifications / Messaging source baseline

> 快照日期：2026-09-16。本文是 `NM-00` 的 source-only inventory，不是 durable
> NotificationStore、outbox、DeliveryWorker、OS notification 或 live message delivery 的完成声明。
> 本轮不在本地运行测试；`notifications_baseline` 只由 GitHub Actions 执行。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`NM-00`](../roadmap.md#step-nm-00) |
| source snapshot | `a71179b`（AUT-01 已推送的干净基线） |
| proof ceiling | `source`；source guard/test-target 编译不提升 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical fact source | `EventLog/RuntimeEvent`、Approval/Company/Recovery facts；通知、Human Inbox、RunStream、SSE 和 transcript 都是派生视图 |
| this step does | event→recipient/channel inventory、现有入口边界、legacy/compatibility gap、NM-01..22 handoff 与 fixture catalog |
| this step does not | 不新增 Notification/Message schema、SubscriptionStore、NotificationProjector、outbox、DeliveryWorker、read state 或第二消息总线 |

## 2. Source hashes

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Human inbox domain projection | `kiana-domain/src/platform.rs` | `1980a509c73f2d5f1fc6f8a21fd3e2711e253e0d2c8b4e6066f3baf95e2eeb0d` |
| Platform/HumanInbox reducer | `kiana-core/src/platform.rs` | `9406e248bd2f2812f5e279e780a7c54baeefaf3a4792d30defc5a986bee0676e` |
| Run stream projection | `kiana-daemon/src/run_stream.rs` | `750bb78e46e5f1c42d3a773d0afcf468cc6713032ef47e30ffa100f3313db5c6` |
| Web REST/SSE adapter | `kiana-entrypoints/src/web.rs` | `9c6b356111a93d87e595cbc4a234f5b9352591d7efc2bbe00948b786e62c09ad` |
| Workbench chat adapter | `kiana-entrypoints/src/workbench_chat.rs` | `dc137747186af0bf02d8b0ec221d265c03047f669079d2f25b4fdaf55aeea957` |
| CLI compatibility surface | `kiana-entrypoints/src/cli.rs` | `6baaebf2ba9b9ca9aadebf6fe5fdd7923c6779ac8434ce4deacdc0d2663671d9` |
| NM-00 source guard/workflow | `kiana-core/tests/notifications_baseline.rs`, `.github/workflows/nm00-baseline.yml` | `5ecee2c1e0dc135d333371a83174b54d95e8b8821faf5a8d3ec186804a683a27`, `4697fac6a15f7019011def99f36bcd33fefb4805f4dc4b46a24e2f76c0e707be` |

Pre-existing hashes are review anchors. A later NM step touching a file must refresh its row in the
same commit; the guard/workflow row is bound to this formatted source.

## 3. Event → recipient → channel matrix

| Committed source fact | Current projection / recipient | Current channel | Owner and limitation |
|---|---|---|---|
| `approval.requested` / pending approval journal | `HumanInboxItem::Approval`, challenge and redacted arguments | `human.inbox`, `/api/approvals`, Workbench `/approvals` | ApprovalStore/ControlPlane owns authority; inbox is read-only and has no durable notification/read state |
| Company Review/Acceptance/Delivery/Incident facts | `HumanInboxItem::{Review,Acceptance,Incident,Reconciliation}` from `company_snapshot`/business projection | `human.inbox`, Web state/command panel, Workbench/CLI command actions | Company object remains owner; action must return to ControlPlane with revision/proof; no shared message bus |
| Failure/Recovery facts | `HumanInboxItem::Incident|Reconciliation` from `failure_incidents` | `human.inbox`, Web/Workbench details | FailureIncident is a projection; Unknown requires reconciliation and cannot be ACKed into success |
| `run.*` terminal/approval/usage/tool facts | `RunStreamEvent` and Web SSE / Workbench stream view | process-local `RunStreamBus`, `/api/events` | Committed-only wake hint plus bounded terminal replay; deltas can lag/drop and are never authority |
| transcript/chat/file-change summaries | Web `ThreadView`, Workbench `ChatMessage`, CLI output | in-process memory/UI | disposable display state; cannot create approval, completion, delivery or read-state facts |
| future message/notification event | no current typed materializer | no current durable channel | reserved for NM-01..NM-13; must consume committed cursor only |

## 4. Current implementation and explicit gaps

### Already wired (source only)

- `kiana-core::human_items` combines pending approvals, CompanyOS business inbox, acceptance,
  incident and failure/reconciliation projections. Actions carry command names and bounded arguments;
  the source objects remain authoritative.
- `RunStreamBus` uses bounded Tokio broadcast channels, per-daemon epoch/sequence, bounded terminal
  retention and gap signaling. `StreamEventStore` only projects fresh committed appends; replay or
  stream delivery does not create a second fact.
- Web checks loopback Host/token/Origin for mutations and exposes `/api/events` SSE, `/api/state`,
  `/api/receipt` and approval routes. Workbench applies deltas only as display and leaves terminal/
  Receipt reconciliation to the daemon. CLI compatibility commands route to the same product paths
  where supported.

### Missing / not supported

- No `CommunicationMessage`, `Notification`, `NotificationSubscription`, `DeliveryAttempt`,
  `NotificationStore`, `SubscriptionStore`, outbox or `DeliveryWorker` exists in the product path.
- No durable unread/read/ACK/snooze/subscription state or recipient resolver exists. Human Inbox is
  rebuilt on demand from Approval/Company/Failure facts and does not become a new authority.
- No external Email/Slack/Webhook/A2A/OS notification channel is configured. A client receiving SSE,
  HTTP 2xx or a toast is not evidence that a business action was applied.
- Cursor/epoch/terminal replay are process-local; restart, lag, gap and dropped deltas require a
  fresh state/Receipt query. There is no durable notification checkpoint or delivery reconciliation.
- `kiana-entrypoints::sdk::watch_scheduled_tasks` remains a separate compatibility API and is not a
  notification/scheduler source; its use cannot be used to claim an NM event or delivery.

## 5. Migration and safety boundary

The only valid future migration is `committed EventLog fact → typed Notification/HumanTask projection`
through server-derived owner, project, recipient, channel and data scope. A notification projector must
never infer approval/completion from transcript, model text, RunStream delta, HTTP ACK or UI state. Any
action click must return to ControlPlane and re-check actor, session/project ownership, authority/data
epoch, target revision, approval/criteria, idempotency and retention before a side effect.

Critical terminal/approval/incident/reconciliation signals must be queryable after a dropped stream;
best-effort delta loss can only mark the view stale/syncing. Unknown send/ACK or delivery-fact failure
must be fenced and reconciled, never blindly retried. Retention/withdrawal append facts and cannot erase
the canonical EventLog. The legacy SDK watcher remains compatibility-only until a versioned upcaster,
occurrence key and explicit ControlPlane command are implemented.

## 6. Fixture catalog and handoff

| Fixture | Purpose | Owner step |
|---|---|---|
| `notifications_baseline` | source-only projection/bus/gap/legacy guard | NM-00 |
| `message_kind_cannot_grant_permission` | Chat/StatusReport/Evidence text cannot authorize | NM-01/02 |
| `committed_event_only_materializes_notification` | pre-commit/unknown source never visible | NM-03/04 |
| `notification_scope_recipient_intersection` | actor/project/epoch/subscription scope only narrows | NM-05 |
| `notification_dedup_replay_single_intent` | event/recipient/policy digest OCC and replay | NM-07 |
| `critical_outbox_survives_crash_or_unknown` | lease/fence/delivery Unknown | NM-08/12/18 |
| `human_inbox_read_is_not_approval` | read/ACK/snooze action separation | NM-09/10/11 |
| `run_stream_gap_requires_snapshot` | epoch/lag/terminal recovery | NM-13/15 |
| `notifications_four_entrypoint_parity` | CLI/TTY/Web/Desktop same source projection | NM-14/15/16/21 |

NM-00 closes only the inventory and migration guard. It does not add a message bus, read state,
notification schema, external delivery or durable proof; all behavior fixtures run in GitHub Actions.
