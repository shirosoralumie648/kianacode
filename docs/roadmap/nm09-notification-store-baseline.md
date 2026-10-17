# NM-09 in-process/in-app NotificationStore query/page baseline

> 快照日期：2026-09-25（完成切片 2026-10-17，CI-only）。测试只由 GitHub Actions 执行；本步骤
> 不在本地运行测试、build、check、clippy 或 smoke，且不等待 CI。

`kiana-core::NotificationStore` consumes the committed-only `NotificationMaterializer` and exposes
bounded recipient-scoped `list` pages plus projection-only `mark_read`/`acknowledge`/`snooze`
mutations. Pages bind the materializer source cursor and digest, cap page size at 30, distinguish
unavailable projection from an empty result, and reject stale/unknown cursors or foreign recipients.
Read/ACK state is process-local presentation state. It never copies HumanTask/Approval status, does
not delete source history, does not call EventStore/Broker/Runner, and does not turn an ACK into
approval, retry, cancel or resume. `feature_status=implemented`; `proof_level=source`.

## Completion slice (2026-10-17, CI-only)

The store now binds a single server-authenticated recipient before it answers anything:

- `NotificationStore::bind_server_recipient` is set once by the composition root; rebinding to the
  same principal is idempotent and rebinding to another principal fails closed with
  `notification_store_scope_conflict`.
- Every page or mutation requires the binding (`ScopeRequired`) and refuses a foreign recipient
  (`RecipientMismatch`) instead of filtering silently; `list`, `mark_read`, `acknowledge` and
  `snooze` all go through the same check, so an entrypoint cannot widen the scope.
- `DaemonHost::notification_page` derives the scope from `self.principal` and calls
  `bind_server_recipient` before `apply_committed`/`list`; the daemon page path stays read-only.
- Page rows and receipts self-validate: `NotificationPageItem::validate` rejects impossible
  rows (acknowledged without read, zero due, oversized text/actions) and
  `NotificationPage::validate` rejects a continuation cursor that is not the last row of the page
  (`notification_store_invalid:page_next_cursor`), so a forged page cannot loop a client forever.
- `NotificationProjectionMutation::validate` keeps the receipt on the projection surface:
  an acknowledgement also reads the row, and a forged receipt carrying `approved`/`decision`
  fields fails at the `deny_unknown_fields` boundary.
- `NotificationStore::retained_source_event_count` exposes the append-only source retention used by
  the fixtures; read/ACK/snooze never shrink it and a regressed committed cursor is refused
  without touching state. `NotificationPageItem` continues to carry no task-status field.

Deny-first fixtures (`kiana-core/tests/nm09_notification_store.rs`, 5 named cases):
`unbound_and_foreign_recipient_queries_are_refused` (unbound/foreign/scope-conflict matrix, probe
cursors and expected-cursor outcomes), `empty_projection_is_not_unavailable_and_page_cap_is_enforced`
(limit 0/31/u16::MAX, schema and blank recipient, empty-vs-unavailable, forged page rows),
`bounded_pagination_terminates_without_duplicates_or_cross_recipient_leaks` (33 retained facts for two
recipients, 20 visible rows, bounded multi-page walk, same-cursor recompute and fresh rebuild
byte-equal, foreign continuation cursor refused),
`read_and_ack_never_become_an_approval_or_task_status_mutation` (idempotent read/ACK, clock
regression, forged receipts, HumanTask bridge/status untouched, rebuild returns to unread) and
`projections_only_append_and_mutations_never_delete_source_history` (exact replay idempotent,
incremental apply keeps state, regressed cursor refused). The source guard
(`kiana-core/tests/nm09_notification_store_guard.rs`) binds the markers, the forbidden
Broker/EventStore/filesystem/network/deletion surface, the daemon bind-before-page ordering, the
shard registration (`kiana-core-s4/6` already lists both targets) and the manual lane
`.github/workflows/nm09-notification-store.yml` (`workflow_dispatch`-only, `contents: read`,
`cargo fetch --locked` + fmt gate + two targets + two `--no-run`).

## Known limits

No durable NotificationStore/read state, cross-process rebuild, NM-10 action command, NM-11
urgency/read ordering, NM-12 retention/withdraw, Web/SSE/Desktop channel or live receipt evidence
is claimed. Empty/unavailable, stale/foreign, pagination and mutation-idempotency boundaries are
only covered by GitHub CI fixtures and source guards; the local run is CI-only and the receipt is
not awaited.
