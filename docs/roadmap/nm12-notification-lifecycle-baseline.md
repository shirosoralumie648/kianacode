# NM-12 durable inbox rebuild and notification lifecycle baseline

> 快照日期：2026-09-25（完成切片 2026-10-17，CI-only）。测试只由 GitHub Actions 执行；本步骤不在本地运行测试、build、check、
> clippy 或 smoke，且不等待 CI。

`NotificationLifecycleFact` models append-only withdraw, supersede and expire decisions bound to
notification id, source event/cursor, data epoch, reason and digest. `NotificationProjector` clones
the committed materializer, validates lifecycle facts, folds exact replay idempotently, rejects epoch
or cursor regressions and refuses terminal lifecycle rewrites before atomically replacing its
projection. Current inbox visibility hides only a validated lifecycle-marked item; source history is
never deleted and replacement notifications remain independent source objects.

`feature_status=implemented`; `proof_level=source`. The projector is rebuildable but its adapter is
still in-process: no durable checkpoint, cross-process retention store, legal-hold executor, artifact
deletion, provider effect or live receipt is claimed. Retention/withdraw/supersede is projection
governance, not HumanTask/Approval mutation.

## Completion slice (2026-10-17, CI-only)

The projector now carries the durable rebuild surface and the retention governance gate:

- `NotificationInboxSnapshot` binds one recipient's inbox view (display item, lifecycle visibility,
  read/ACK/snooze state) to the committed `source_cursor` and `data_epoch` with a self-validating
  digest, so a fresh `NotificationProjector::rebuild` and an incremental `apply_committed` can be
  compared byte for byte instead of by inspection.
- `NotificationReadStateFact` is the append-only fact form of the in-process
  `NotificationProjectionMutation` receipt (`from_projection_mutation` takes the committed event id
  as a parameter, never from the receipt). `apply_read_state_facts` refuses a fact that runs ahead
  of the committed cursor, names an unknown item, reuses or regresses a projection revision, un-reads
  a read row, un-acknowledges an acknowledgement or moves a snooze backwards. A fresh rebuild from
  the same read-state facts reproduces the same unread/read/ACK/snooze state.
- `NotificationRetentionDecision` is a server-authorized governance decision: `admit_retention`
  refuses a foreign principal (`RetentionUnauthorized`), a stale policy revision
  (`RetentionPolicyStale`), an active legal hold (`RetentionLegalHold`), a decision that does not
  advance exactly from the projection's current epoch (`RetentionEpochConflict`), a cursor ahead of
  the committed source, and a notification the committed projection never materialized. A successful
  admission appends exactly one lifecycle fact and never removes a source event, a materialization or
  a read-state fact.
- `NOTIFICATION_RETENTION_PAYLOAD_LAYERS` requires the `Artifact → Memory → Index → Cache`
  subsequence of PD-26's `RevocationLayer` set, each tombstoned with a receipt at the decision's data
  epoch, in order. A decision that covers only the cache/UI row, still serves the payload, is
  pending/unreachable, sits at an older epoch or tries to target the `Facts` layer is refused before
  admission, so "delete only the UI/cache" can never be reported as a completed retention.
- Audit metadata and payload references stay separate typed fields (`ReceiptAuditMetadata` vs
  `ReceiptPayloadRef`, both `deny_unknown_fields`); the decision refuses audit metadata that is not
  retained, disagrees on epoch/cursor/policy revision, does not name the decision's own source event,
  or when a payload reference reuses an audit event identity.
- `layer_epochs` exposes the data epoch every payload-bearing layer acknowledged; replaying the
  retained decisions (`rebuild_with_retention`) converges to the same per-layer epochs and the same
  snapshot digest.

Deny-first fixtures (`kiana-core/tests/nm12_notification_projector.rs`, 10 named cases):
`fresh_rebuild_reproduces_the_same_inbox_snapshot_as_incremental_apply`,
`withdraw_expire_and_supersede_only_append_facts_and_never_delete_source_history`,
`retention_admission_refuses_unauthorized_hold_and_stale_policy`,
`retention_refuses_incomplete_payload_layers_and_facts_deletion`,
`retention_admission_appends_one_lifecycle_fact_and_propagates_the_data_epoch`,
`read_state_facts_rebuild_the_same_unread_state_from_the_source_cursor`,
`presentation_mutations_become_replayable_read_state_facts`,
`audit_metadata_and_payload_refs_never_cross_the_boundary`, plus the two original
cursor/epoch-bound replay fixtures. `kiana-domain/tests/nm12_notification_lifecycle.rs` adds the
decoding boundary that refuses a lifecycle fact smuggling `payload_refs`/`approved`/`decision`
fields. The source guard (`kiana-core/tests/nm12_notification_projector_guard.rs`) binds the markers,
the forbidden Broker/EventStore/filesystem/process/deletion surface, the fixture names, the shard
registration (`scripts/ci/test-shards.json`) and the manual lane
`.github/workflows/nm12-notification-lifecycle.yml` (`workflow_dispatch`-only, `contents: read`,
`cargo fetch --locked` + fmt gate + three targets + three `--no-run`).

## Known limits

No durable checkpoint store, cross-process retention executor, legal-hold registry, artifact byte
deletion, provider delivery effect or live receipt is claimed. `NotificationReadStateFact` is a
replayable fact shape and fold, not a durable journal: the composition root still has to commit the
fact through the existing EventLog path, and `NotificationStore` remains the in-process presentation
overlay. Retention admission reads adapter-reported layer observations and never erases bytes, mutates
a HumanTask/Approval or calls the Broker; the per-layer epoch map is projection state rebuilt from the
same decisions, not proof that an external layer deleted anything. Behavior is covered only by GitHub
CI fixtures and source guards; the local run is CI-only and the receipt is not awaited.
