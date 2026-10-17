# H23 compaction artifact and commit baseline

## Delivered source slice

- `CompactionArtifact` validates summary/artifact digests, source cursor range and content size
  before a view commit can reference it.
- `CompactionCommit` binds the artifact and summary to source event IDs/cursor, old/new context
  revisions, prompt/profile/budget digests, workspace revision and data epoch.
- `validate_before_commit` rejects concurrent steering/source changes, workspace changes and data
  revocation before the compacted view advances; the old context remains the safe fallback.
- `kiana-domain::CompactionCommitJournal` fixes the required order as a deterministic contract:
  `stage` fully saves the summary bytes and recomputes their sha256 before any reference exists,
  `commit` CAS-commits the reference through the existing `ContextCheckpoint` and only then switches
  the effective view and records a `CompactionAppliedFact` (`compaction.applied` equivalent: commit
  reference plus the commit-time view/Inbox binding). A crash before commit leaves the old view
  active, keeps already applied facts queryable and exposes the saved-but-unreferenced artifact to
  normal GC (`gc_candidates`).
- New input that arrives while compaction runs is carried into the fact and the next checkpoint
  instead of being overwritten; a source cursor or context revision that moved rejects the late
  summary with `compaction_source_changed`.
- Source revocation propagates through `ContextInvalidationPlan`: applied summary facts are marked
  `invalidated_by`, the same summary can no longer be staged or committed
  (`compaction_summary_invalidated`), a stale `data_epoch` fails closed
  (`compaction_data_epoch_changed`), and a derived cache binding is invalidated through the existing
  CM-21 contract.
- `NativeCompactionBinding` gates provider-native opaque compaction on an explicit
  `CapabilitySupport::Supported` (`Unknown` fails closed) and refuses to reuse a native handle after
  a route/model switch (`native_compaction_route_changed`) or a changed compiled context
  (`native_compaction_context_recompile_required`).
- `rebuild_view` re-verifies the saved bytes and rebuilds the same `ContextResumeView` from the
  committed checkpoint, so a restart that only has the serialized journal reconstructs the identical
  view digest.
- The CI source guard connects the contract to the existing runner checkpoint/prompt provenance and
  Compacted event path without adding a second event loop.

## Named behaviour fixtures

The deny paths run first; all named fixtures live in
[`kiana-domain/tests/h23_compaction_commit.rs`](../../kiana-domain/tests/h23_compaction_commit.rs) and
stay deterministic (no filesystem, process, model or network):

- `crash_before_compaction_commit_keeps_old_context` (deny): after a committed compaction the journal
  refuses to stage a new artifact until the next compaction starts; an artifact that was fully saved
  and verified but not committed leaves the effective view at the previous revision, lists the
  artifact as a GC candidate, keeps the old fact queryable, and a reopen from the serialized journal
  still rebuilds the old view.
- `stale_summary_cannot_overwrite_new_steering` (deny): a late source event (cursor moved to 13)
  rejects the summary planned for `[10,12]` with `compaction_source_changed`, a stale Inbox snapshot
  is rejected with `context_checkpoint_inbox_changed`, and the accepted commit carries the new
  steering sequence/digest forward instead of overwriting it.
- `revoked_source_invalidates_summary` (deny): a post-revocation `data_epoch` fails closed before the
  view advances and the artifact stays a GC candidate; an applied fact is marked invalidated by the
  plan digest, re-staging the same summary is rejected with `compaction_summary_invalidated`, and the
  derived cache binding is invalidated.
- `compacted_context_rebuilds_to_same_view_after_restart` (success): the applied fact records the
  source event IDs, prompt/profile and budget digests and the old/new context revisions; the
  serialized journal rebuilds the same view digest and checkpoint digest, while tampered saved bytes
  are rejected with `compaction_summary_digest_mismatch`.

`native_opaque_compaction_requires_provider_capability_and_recompiles_on_model_switch` covers the
provider-capability gate and the recompile requirement for a model switch.

## CI wiring

The manual focused workflow [`h23-compaction-commit-diagnostic.yml`](../../.github/workflows/h23-compaction-commit-diagnostic.yml)
runs the fmt gate, the domain commit fixtures, the Core source/wiring guard, per-fixture `--exact`
reruns and `--no-run` compiles for both targets. The Core guard
([`kiana-core/tests/h23_compaction_commit_guard.rs`](../../kiana-core/tests/h23_compaction_commit_guard.rs))
binds the named fixtures to this baseline and the workflow. An unawaited remote run does not raise
the proof level.

## Boundary and proof ceiling

This is a typed artifact-first/CAS contract plus its deterministic order fixtures. It does not claim
a durable artifact adapter, cross-store two-phase commit, model-generated summary retry, physical GC
or power-loss restart recovery; those remain later H24/PD/SC work. The journal never deletes facts
or makes a cache authoritative, and the real byte persistence stays behind the Core
content-addressed artifact store port. No local build or test was run for this slice; compilation
and behaviour depend on GitHub Actions.
