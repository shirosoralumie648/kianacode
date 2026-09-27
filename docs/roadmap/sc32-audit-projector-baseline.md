# SC-32 audit projector with CAS, cursor and rebuild baseline

> Snapshot date: 2026-09-28. This slice owns the *decision* that an audit projection is a cache
> that moved forwards, moved once, or not at all. Local Cargo test/build/check/clippy/smoke
> commands are intentionally not run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`SC-32`](security-compliance.md#step-sc-32) |
| source snapshot | master plus this SC-32 projector/CAS slice |
| feature_status | `partial` for source-level CAS, cursor, freshness and rebuild-equivalence contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | committed events -> fold -> (rebuild proof, freshness, authorized page) |

A projector is a cache of audit facts. A cache that can move backwards is worse than no cache: it
un-answers a question that has already been answered, and a caller re-reading a window sees a row
disappear with no way to tell a deletion from a rollback. This slice makes three properties
decidable in source, and refuses every other shape.

```text
facts -> AuditProjectorFold -> { head digest, source cursor, source identity digest }
                              -> AuditProjectionCommitReport  (CAS + cursor)
                              -> AuditProjectionRebuildReport  (rebuild == replay)
                              -> AuditProjectorFreshness       (is this view current)
                              -> AuditProjectorAuthorization   (may this subject read it)
```

The publish decision admits exactly three outcomes, and the source cursor is strictly increasing in
all but one of them:

| Outcome | When | Cursor |
|---|---|---|
| `Advanced` | `expected_generation` is the published generation, target is generation + 1, the previous state digest is the published one, and the new cursor is greater | strictly greater, and contiguous unless the claim declares a rebuild |
| `ExactReplay` | the claim's target is *already* the published head: same generation, same cursor, same state digest | unchanged, and explicitly a no-op rather than an advance |
| `Rejected` | everything else, each with one stable reason | unchanged |

A gap is only crossable by a claim that declares a rebuild, and the only honest rebuild origins are
`0` (the whole log) and the published cursor (re-fold the published head). Any other origin asserts a
history nobody verified and is refused.

## What the card rejects, and how

| Rejected | How |
|---|---|
| projection covering a fact | `audit_projection_is_append_only` runs *after* the reducer, comparing every previously projected `(audit_id, record_digest)` pair and requiring the source identity list to be a prefix-extension of the previous one; fewer rows, a changed digest, a missing `audit_id` or a rewritten identity list are each refused (`audit_projector_projection_not_append_only`, `..._dropped_record`, `..._rewrote_record`, `..._source_rewritten`) |
| cursor skipped | a tail must start at exactly `source_cursor + 1`; a later start is a gap, an earlier one is a regression, and a re-stated cursor with a different state is `audit_projector_cursor_not_advanced`. At the publish layer the same rule appears as `audit_projection_cursor_gap` and `audit_projection_cursor_regression`, and a non-contiguous advance is admitted only with a declared rebuild origin |
| an old view marked fresh | `AuditProjectorFreshness` binds the view to the published generation, cursor and state digest, in that order, and reports through the existing domain `ProjectionLagView` rather than a second lag vocabulary. Only an exact match is fresh; an edited `fresh` cannot stand on its own because freshness is re-derived in `validate_against` |
| a query outside its scope | `AuditProjectorAuthorization` re-uses the SC-24 `QueryDataBoundary` as the authority. A query must name that boundary's project root, its scope digest, and its *decision digest*; a caller that recomputes a scope for itself cannot produce the last one (`audit_projector_query_authority_digest_mismatch`). A boundary that denies the derived read model denies the whole projection, and `page()` re-derives the decision instead of trusting a deserialized `Allowed` |
| two projectors both advancing | a claim carries the generation it *read*. Only a claim whose `expected_generation` equals the published generation may advance it, and only to generation + 1; the loser is told the generation it lost to (`audit_projection_generation_cas_conflict`, `audit_projection_generation_not_monotonic`) |
| a claim folded from a state the head never published | `previous_state_digest` must equal the published state digest, checked before the cursor rules so "you were folding something else" is never reported as "you moved backwards" (`audit_projection_state_digest_drift`) |
| rebuild and incremental replay disagreeing about history | `AuditProjectionRebuildProof` carries both sides' state digests *and* both sides' source identity digests. `Converged` requires the same facts, the same cursor and the same folded state; anything else is `Divergent` with the cheapest true reason |
| a tampered report standing in for a decision | both reducers run in `evaluate` *and* in `validate_against`, so an edited status, generation, cursor, digest, reason or remediation is rejected against the same head and claim (`*_binding_invalid`, `*_reason_state_mismatch`, `*_digest_mismatch`) |

## Failure-first fixture matrix

### `kiana-core/tests/sc32_audit_projection_commit.rs`

| Fixture | Assertion |
|---|---|
| exact replay | a claim re-stating the published head is `ExactReplay`, is not an advance, and leaves generation, cursor and state where they were |
| rollback in replay clothing | a claim matching on generation but not on cursor is `Rejected` as a cursor regression, never an exact replay |
| CAS race | two claims read generation 1 and both claim generation 2; one advances, and against the advanced head the other is `audit_projection_generation_cas_conflict` and is told generation 2 |
| generation skip | a claim from generation 1 to 3 is `audit_projection_generation_not_monotonic` even though the cursor moved |
| state drift | a claim whose `previous_state_digest` is not the published one is `audit_projection_state_digest_drift` |
| cursor gap | cursor 10 to 12 with no rebuild declared is `audit_projection_cursor_gap`, and the remediation names re-folding the missing range |
| declared rebuild | the same cursor range advances when `rebuild_from_cursor` is the published cursor, and also when it is `0` |
| forged rebuild origin | `rebuild_from_cursor` of `4` is `audit_projection_rebuild_origin_mismatch` |
| projector mismatch | a claim filed against another projector's head is `audit_projection_projector_mismatch` |
| cursor re-stated with a new state | same generation, same cursor, different state is `audit_projection_cursor_not_advanced`, not a replay and not an advance |
| tampered report | an edited status and an edited cursor are both `audit_projection_commit_binding_invalid`; a rejection stripped of its reason is refused |
| tampered head/claim | an edited generation or cursor without re-sealing is a digest mismatch, and the mismatch is raised before any decision |
| decision precedence | a claim violating projector, generation, state, cursor and rebuild-origin rules at once reports the projector; the same claim with the projector corrected reports the CAS conflict |
| rebuild convergence | matching facts, cursor and state digests produce `Converged` with an empty reason |
| rebuild read different facts | a different source identity digest is `audit_projection_rebuild_source_identity_divergent`, reported before the state comparison |
| rebuild state divergence | a different rebuilt state digest at the same cursor is `audit_projection_rebuild_state_divergent` and says the two paths disagree about history |
| rebuild against the wrong head | generation zero is rejected at construction; a proof taken at a different generation, and one that stopped short of the head cursor, are both `Divergent` |
| tampered rebuild report | an edited status and an edited state digest are both refused |
| strict wire shapes | the five schema strings, `SchemaVersion::new(1, 0)`, round-trip, and `deny_unknown_fields` rejection |
| shared identity digest | the identity digest `kiana-query` computes is carried unchanged across the crate boundary and converges; a different identity set never collides |

### `kiana-query/tests/sc32_audit_projector.rs`

| Fixture | Assertion |
|---|---|
| rebuild == incremental replay | a three-event from-scratch fold and a two-event fold plus a one-event tail land on the same state digest, the same source identity digest, the same cursor and the same rows, at different generations; every earlier record digest is retained |
| append-only rule | the reducer's own extension is admitted, and each of the four covering shapes is refused: fewer rows (`ProjectionNotAppendOnly`), a shortened identity list (`ProjectionSourceRewritten`), a row edited under its own identity (`ProjectionRewroteRecord`), and a row removed while its identity stays (`ProjectionDroppedRecord`) |
| replayed source event | a tail re-stating an already projected event is `ProjectionEventReplay` |
| regression | a tail starting at or before the published cursor is `CursorRegression` |
| gap | a tail starting past `source_cursor + 1` is `CursorGap` |
| unfoldable source | an empty source, a zero first cursor, and an untrusted `audit.record` event are each refused, the last through the SC-31 taxonomy (`audit_event_kind_untrusted`) |
| current view | a matching view is fresh, with an empty reason and `ProjectionLagStatus::CaughtUp` |
| older publish | a view answered against a head one generation ahead is `audit_projector_view_generation_stale`, reads as `Pending` in the shared lag view, and carries the published generation |
| state drift under an unchanged generation | a head with a different state digest makes the view `audit_projector_view_state_drift` |
| view ahead of the head | an unpublished projection is refused with `audit_projector_view_cursor_ahead` rather than labelled stale |
| edited freshness | flipping `fresh` and clearing the reason is `audit_projector_view_freshness_mismatch` |
| project crossing | a query naming another project root is `audit_projector_query_project_mismatch` and `page()` returns `AuthorizationDenied` |
| self-asserted scope | a query carrying a caller-computed scope digest is `audit_projector_query_scope_digest_mismatch` |
| caller-asserted authority | right project and scope, wrong decision digest, is `audit_projector_query_authority_digest_mismatch` |
| revoked boundary | a `revoked` `QueryDataBoundary` denies the whole projection: `audit_projector_query_boundary_denied` |
| unbounded page | a limit of zero and one over the bound are both `audit_projector_query_header_invalid` |
| stale view read | an in-scope read of a stale projection is denied with the freshness reason, and `page()` refuses it |
| cursor past the projection | `audit_projector_query_after_cursor_ahead` |
| allowed read | an in-scope query on a fresh view is `Allowed`, returns only rows after the cursor, and pins `fold_state_digest` to the exact read model decided on |
| grant does not follow the object | a granted authorization replayed against a different projection at the same generation and cursor is refused, and still works for the projection it was decided on |
| tampered fold | a snapshot with a rewritten cursor fails snapshot validation; a rewritten source identity digest is `ProjectionSourceRewritten` |
| strict wire shapes | the five schema strings, `SchemaVersion::new(1, 0)`, round-trip, `deny_unknown_fields` rejection, and order/repeat independence of the identity digest |
| deny-code registry | `AUDIT_PROJECTOR_DENY_CODES` has six entries in decision order and its first and last entries are the ones the fixtures produce |

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate. No separate workflow.

This is a source-only decision over supplied digests, counters and already-committed events.
**It does not store, persist, schedule, replay, compact, evict or serve anything.** There is no
`std::fs`, no port call, no adapter call and no event append in either module; the guard asserts that
boundary by text. The fold runs on `RuntimeEvent` values a caller hands it, so nothing here proves
that a real EventLog was read, that a checkpoint was written, that a projector ran on a schedule, or
that two processes racing on one head were serialised — the CAS is a decision over a head and a
claim, not a compare-and-swap performed by a store.

Four things the card asks for are explicitly not established here. First, the CAS is not a lock:
nothing in this slice makes two projectors *unable* to advance, only unable to both produce an
`Advanced` report against the same head, and the atomicity itself still belongs to the EventLog
append path (SC-12). Second, "rebuild from scratch" is a pure fold over events the caller supplies:
no snapshot is restored, no index is rebuilt and no projection is deleted here, so the delete-and-
rebuild rehearsal stays SC-42 work. Third, the freshness decision takes the published position from
the caller; a caller that hands over a stale `AuditProjectionPosition` gets a confidently wrong
`fresh`, and nothing in this slice can tell a truthful projector from a lying one. Fourth, the query
authorization re-uses the SC-24 `QueryDataBoundary` as its authority; that boundary is itself a
source contract, so this slice decides *whether a read is permitted under a stated boundary*, and
does not establish that the boundary was itself correctly derived from a session, a project and a
role. The pre-existing `ControlPlane::query_audit` in `kiana-core/src/audit_projection.rs` derives
its own ownership filter from `run.authorized` facts; the two paths are not unified by this slice
and no test asserts that they agree.

The existing domain vocabularies are reused rather than forked, and the mapping is worth stating so
it is not mistaken for a coincidence: `AuditProjectionSnapshot`/`AuditProjectionCheckpoint` supply
the snapshot and its `projection_version`; `ProjectionLagView` supplies lag and freshness; ER-29
`DataPropagationTarget::Audit` and the PD-26 `RevocationLayer` order both treat a projection as
downstream of the facts, and this slice sits below both of them rather than beside them. SC-32 adds
no `kiana-domain` module, so the shared `kiana-domain/src/lib.rs` and `kiana-domain/src/contracts.rs`
registry are untouched; the six `AUDIT_PROJECTOR_DENY_CODES` and the SC-32 refresh reasons are held
in `kiana-query` because the decision they name is made there.
