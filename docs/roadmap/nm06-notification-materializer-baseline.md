# NM-06 Notification Materializer Baseline

## Scope

NM-06 adds a bounded in-memory `NotificationMaterializer` over committed EventLog facts.  It
requires the registered event source, an exact source event ID and cursor, redacted summary,
evidence references, action references and due/expiry window.  A `HumanTaskBridge` binds the
immutable task target and decider while deliberately omitting task status; the original
HumanTask/approval/acceptance object remains the status authority.  The materializer returns the
existing display-only `HumanInboxItem` after exact server-recipient and decider filtering and a
stable sort.

The projection rejects cursor gaps, source-event/digest conflicts, missing payload/evidence/due
metadata and untrusted event sources.  Reapplying the same event and digest at the same cursor is
idempotent.  It does not own a `NotificationStore`, outbox, delivery worker, provider, Broker or
second execution loop.

## Completion slice (2026-10-17, CI-only)

The card's exit conditions are now enforced at the contract level, not only at query time:

- `HumanInboxKind::is_actionable()` marks approval/review/acceptance/incident/reconciliation, and
  `HumanInboxKind::human_task_target_kind()` fixes the authority object each one may point at.
- `NotificationMaterialization::validate()` refuses an actionable fact without a `HumanTaskBridge`
  (`notification_materialization_human_task_required`) and refuses a bridge aimed at the wrong
  authority kind (`notification_materialization_human_task_target_kind_mismatch`); the existing
  digest/decider/due/evidence equality checks stay
  (`notification_materialization_human_task_mismatch`).
- `NotificationMaterializer::apply_committed` keeps the registry class as the classification
  authority: an `approval.*` fact may only materialize an approval task and an incident fact may
  only materialize an incident/reconciliation task
  (`notification_materializer_event_class_mismatch`), on top of the committed source, cursor,
  source-event and payload-digest checks.
- `kiana_core::materialize_notification_inbox` is the public `platform.rs` bridge used by the
  fixtures to prove the materializer errors map onto `CoreError` without widening scope.

## Evidence and limits

- `kiana-core/tests/nm06_notification_materializer.rs` covers committed source validation,
  missing source/evidence/due data, cursor gaps, replay/conflict behavior, redacted summaries,
  recipient/decider filtering and display projection.  Completion fixtures:
  `actionable_kinds_require_the_original_human_task_binding` (per-kind deny matrix over the five
  actionable kinds: no bridge, wrong target kind, wrong decider, empty evidence, zero/closed due
  window, zero cursor, nil source event),
  `materializer_never_visible_when_source_evidence_due_or_decider_is_missing` (raw committed
  payloads mutated per kind; every page is rejected with the expected stable error and the
  projection keeps its previous state and shows nothing),
  `materializer_materializes_every_kind_with_summary_actions_due_and_source_authority` (success
  matrix: redacted summary, action refs, due/expiry, source cursor, evidence refs and the exact
  HumanTask target/decider/revision/digest binding, with no copied task status in the item),
  `materializer_refuses_a_class_that_cannot_own_the_materialized_kind`,
  `bridge_identity_tampering_fails_closed` and
  `platform_bridge_projects_server_scoped_items_and_fails_closed`.
- `kiana-core/tests/nm06_notification_materializer_guard.rs` protects the projection-only,
  server-scoped boundary (including "no HumanTask status copied into the bridge"), binds the
  fixture names to the automatic `kiana-core-s4/6` shard entries and to
  `.github/workflows/nm06-notification-materializer.yml`. GitHub Actions runs fixtures; local
  tests are intentionally not executed.

## Known limits

This slice is `feature_status=implemented`, `proof_level=source`: no durable materializer,
HumanTask store, notification outbox, delivery worker, external channel or live/physical proof is
claimed.  The materializer is an in-memory projection; dedup/OCC (NM-07), the durable outbox and
delivery worker (NM-08), the durable store/page projection (NM-09), HumanTask action submission
(NM-10) and every external channel remain open.  The class/kind restriction is a classification
guard only: it does not prove that the referenced approval/incident object exists yet, which is
rechecked by the original ControlPlane authority when an action is submitted.
