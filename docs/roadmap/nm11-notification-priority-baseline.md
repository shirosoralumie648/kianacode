# NM-11 notification urgency/read/ACK/snooze/digest baseline

> 快照日期：2026-09-25。测试只由 GitHub Actions 执行；本步骤不在本地运行测试、build、check、
> clippy 或 smoke，且不等待 CI。

NM-11 extends the in-process `NotificationStore` projection with deterministic server-derived
urgency, due time, source cursor and digest-group ordering. `NotificationUrgency` maps only the
server-owned Human Inbox kind; client clocks and presenter text cannot reprioritize items. Page
items expose read/ack/snooze projection metadata without copying HumanTask/Approval authority.

Snooze is bounded projection state and does not remove critical or Unknown items. Read/ACK/snooze
are idempotent, clock-fenced projection mutations; none is approval, retry, cancel, resume, EventLog
append or Broker effect. `feature_status=implemented`; `proof_level=source`.

Known limits: no durable read/snooze state, digest delivery worker, retention/withdraw, action
command, Web/SSE/Desktop surface, cross-process rebuild or live/physical delivery evidence is
claimed. CI fixtures and source guards cover ordering and no-authority boundaries only.

## Completion slice (2026-10-17, CI-only)

The 2026-09 slice shipped urgency/due/source ordering plus projection-only read/ACK/snooze state.
This slice closes the card's exit conditions in source and wires them into CI; no local test, build,
check, clippy or smoke run happened, and no CI receipt is claimed.

Deny side:

- **Client time / pre-selected answers cannot reprioritize or approve.** `NotificationListRequest`
  keeps `deny_unknown_fields`, so a request carrying `urgency`, `priority`, `client_time_unix_ms`,
  `server_time_unix_ms`, `preselected_action`, `approved`, `decision`, `actions`, `read`,
  `acknowledged`, `snoozed_until_unix_ms` or a client `recipient_scope` fails to decode.
  `classify_notification` refuses a committed item whose `detail` carries any of
  `NOTIFICATION_PRIORITY_FORBIDDEN_DETAIL_HINTS` (`notification_priority_client_hint`) before a
  priority forms, and urgency still comes only from the server-owned `HumanInboxKind`.
- **Silence cannot hide critical or Unknown items.** `NotificationStore::snooze` refuses an item
  whose server-classified urgency is critical or whose committed notification status is `unknown`
  with `NotificationStoreError::SnoozeNotHideable`; both stay listed, readable and queryable. The
  snooze window cannot end at or before the bound server clock (`snooze_window`) and is bounded by
  `NOTIFICATION_STORE_MAX_SNOOZE_MS` (`snooze_horizon`); a backwards snooze or a mutation dated
  before the bound server clock is `ClockRegression`.
- **Ordering never crosses the recipient scope.** `list`, `inbox_summary`, `digest_folds` and
  `apply_read_state_facts` answer only the server-bound recipient; a foreign recipient is
  `RecipientMismatch`, and a foreign item id cannot be probed as a continuation cursor
  (`CursorInvalid`) or used as a read-state fact (`ItemNotFound`).
- **Rows and pages are self-verifying.** `NotificationPageItem::validate` recomputes the
  server-derived ordering key (`notification_priority_digest`, `DigestMismatch`), the committed
  `due_at_unix_ms`/`source_cursor` binding (`page_row_binding`) and the committed `unknown` flag
  (`page_row_unknown`); `NotificationPage::validate` recomputes `due_state`/`snooze_active` against
  the page's `server_time_unix_ms` (`page_row_server_time`, zero clock → `server_time_unix_ms`).
  `NotificationInboxCounts`/`NotificationInboxSummary` refuse incoherent counts
  (`summary_counts`), an impossible digest group count (`summary_digest_groups`) and a forged
  digest; `NotificationDigestFold::validate` recomputes every aggregate from its members
  (`digest_fold_urgency`/`due`/`cursor`/`critical_count`/`unknown_count`/`unread_count`/
  `acknowledged_count`, member order/duplicates), so a fold can never lower a critical or Unknown
  member's severity.

Success side:

- **urgency → due → committed source sequence.** `compare_notification_priority` orders by urgency
  rank, then due time, then `source_cursor`, then item identity; the order is a total order that
  does not depend on caller array order, read/ACK/snooze state, or the server clock.
- **Server time drives due/urgency presentation.** `NotificationStore::bind_server_clock` is
  monotonic and only the composition root calls it (`kiana-daemon`'s `notification_page` binds it
  before ingest/list). Pages stamp `server_time_unix_ms` and per-row `due_state`
  (`Scheduled`/`Due`/`Overdue`); `inbox_summary` reports unread/read/ACK/snooze/overdue buckets plus
  `digest_group_count` at that reading.
- **Critical items stay queryable.** Because urgency dominates the order, a critical item is the
  first row at every page size, and no read/ACK/snooze mutation removes it.
- **Read state survives a restart as facts.** `NotificationStore::read_state_facts` exports each
  presentation mutation as a `NotificationReadStateFact` bound to the item's committed source event
  and cursor (never to a receipt-supplied id), and `apply_read_state_facts` folds them back into a
  fresh store with the same refusals as the durable projector
  (`notification_store_read_state_cursor_ahead`/`conflict`/`regression`). The same facts rebuild the
  NM-12 `NotificationProjector` inbox snapshot, so both projections agree without either owning a
  task status.

Fixtures: `kiana-core/tests/nm11_notification_priority.rs` keeps six deny-first cases —
`client_clocks_and_preselected_answers_cannot_reprioritize_or_approve`,
`silence_cannot_hide_critical_or_unknown_items_or_approve_anything`,
`ordering_is_urgency_due_source_sequence_and_never_crosses_the_recipient_scope`,
`read_state_facts_rebuild_the_same_unread_state_and_never_become_authority`,
`summary_and_digest_folds_are_server_time_scoped_to_the_bound_recipient`,
`page_rows_recompute_priority_due_state_and_snooze_activation`; the source guard
`kiana-core/tests/nm11_notification_priority_guard.rs` binds the boundary markers, the refusal
matrix, the fixture names, `scripts/ci/test-shards.json` and the manual lane
`.github/workflows/nm11-notification-priority.yml`.

Known limits: only `proof_level=source` is claimed. The read/ACK/snooze projection is still
in-process; the durable journal, cross-process rewrite and delivery/receipt settlement are NM-12,
NM-13 and NM-18 work. There is no digest delivery worker, reminder/escalation scheduler, retention
or Web/SSE/Desktop surface, and no live/physical evidence.
