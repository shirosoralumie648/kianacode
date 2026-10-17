# BQ-14 append-only CostLedgerEntry and CostCorrection baseline

> Snapshot date: 2026-09-25. This slice adds a bounded domain/core/query source contract and
> GitHub-only fixtures. Local Cargo tests, builds, checks, clippy and smoke commands are not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-14`](../roadmap.md#step-bq-14) |
| source snapshot | `e9e2d4ad` plus this BQ-14 source slice |
| feature_status | `implemented` for append-only ledger facts, approval-bound correction admission and read-only projection |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | BQ-13 cost fact → immutable CostLedgerEntry → versioned CostCorrection command → ControlPlane approval → EventLog append → query fold |

`CostLedgerEntry` carries the original amount, estimate/measured provenance, source event, source
cursor, revision and digest. `CostCorrectionCommand` has no transcript or model text field and
requires bounded evidence references, an exact target digest/run/attempt/cursor/revision fence and
an approval whose subject is the command digest. `CostCorrection` is a new fact; the reducer exposes
only `append_entry` and `append_correction`, preserving the original entry on replay. Replaying the
same command returns the original correction, while a changed digest or stale target is rejected.

The core helper validates the typed command and builds an immutable `cost.corrected` event without
dispatching a provider or capability. Query rebuilds a ledger from committed `cost.ledger_entry`
and `cost.corrected` events and computes a derived view without writing facts.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| missing approval/evidence | unapproved or evidence-free correction is rejected |
| target fence | run/attempt/source cursor/revision/digest drift is rejected |
| append-only | original entry remains byte-identical; no update/delete API exists |
| approval binding | approval command/target digest and independent approver are required |
| replay | same command digest/idempotency key replays one correction; conflicting digest fails |
| model boundary | typed command has no model text/transcript authority and core guard has no runner/broker/network path |
| projector | source event duplicate/sequence/run/stream mismatch fails closed; corrected totals are deterministic |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the BQ-14 domain/core/query
fixtures and source guards, and `cargo check --workspace --tests --locked`. Local Cargo tests,
builds, checks, clippy and smoke commands are intentionally not run; CI results are not awaited.

Limitations: the reducer and event builders are source contracts and do not append/flush EventLog,
consume approval store state, provide cross-process CAS, import provider invoices, allocate
financial budgets or prove external billing. Approval consumption remains the existing
ControlPlane/ApprovalStore boundary, and daily rollups/projector checkpoints remain BQ-20.

## 2026-10-17 completion slice (CI-only)

The append-only entry/correction shape above is unchanged.  This slice closes the card's refusal
matrix (in-place UPDATE/DELETE, model-text bookkeeping, missing evidence/approval, target fence
drift), rejects foreign-currency correction deltas before the correction fact forms, restores the
manual lane deleted by the workflow consolidation (`08552ada`) and keeps the automatic shards.
Nothing here promotes the proof level beyond `source`; local tests/builds/checks/clippy/smoke are
not run and the CI receipt is not awaited.

Domain fixtures (`kiana-domain/tests/bq14_cost_correction.rs`, 3 → 10, deny first):
`unapproved_evidence_free_or_model_text_commands_are_refused` (missing approval, empty/duplicate
evidence, newline or blank reason, missing requester/idempotency key, absent or zero delta and a
mixed-currency delta pair), `target_fence_drift_is_refused_before_any_fact_forms` (entry id,
digest, run, attempt, source cursor and revision drift, an unreseaed command digest and a foreign
target entry), `approval_binding_requires_an_independent_approver_and_the_exact_subject` (wrong
command id/digest, wrong target digest, self-approval, tampered approval digest, zero approval time
and malformed approver), `ledger_is_append_only_and_refuses_duplicate_or_unknown_sources`,
`replayed_commands_return_the_original_fact_and_conflicting_digests_fail` (command-id replay
conflict and idempotency-key conflict), `ledger_entry_invariants_reject_forged_provenance`,
`ledger_entries_keep_breakdown_provenance_without_transcript_authority` (BQ-13 estimate/measured/
unknown mapping keeps its provenance; unknown stays amount-free with `UNK` currency),
`ledger_facts_are_registered_cost_ledger_events` (both facts pass
`validate_runtime_event`/`validate_event_payload`; a missing `entry_id` or an extra `transcript`
field is refused) and `model_transcript_cannot_be_decoded_into_command_or_ledger_facts`.  Success
column: `original_corrected_values_and_approver_stay_traceable` folds -20/-5 estimated and +7
measured deltas into 75/7, keeps the original 100 untouched, records both approvers and refuses an
approved but foreign-currency delta with `cost_correction_currency_mismatch`.

Core fixtures (`kiana-core/tests/bq14_cost_correction.rs`, 2 → 4): unapproved, stale, foreign and
model-text commands are refused by `CostCorrectionAdmission::authorize`, the projection from the
existing `ApprovalDecisionRecord` must match approval id, command digest, decision command and
approver, the event builders stay append-only on the `cost_ledger` stream and reject
`stream_version == 0`, and the `ControlPlane::admit_cost_correction[_with_approval]` facade admits
exactly the approved, bound command.

Query fixtures (`kiana-query/tests/bq14_cost_ledger_projector.rs`, 2 → 6): correction before its
target and non-increasing sequence fail the whole rebuild, tampered approver/target run/currency/
aggregate/stream-version bindings fail closed, unrelated events are skipped without advancing the
ledger cursor, and the public `validate()` rejects header and cross-run drift.

Protocol wire fixture (`kiana-protocol/tests/bq14_cost_correction_wire.rs`, new target): the
approved command rides the shared `cost.correction` command route, an unapproved or model-text
command is refused before the wire, and the response projection is digest-shape bound with
`deny_unknown_fields`.

Source hardening: `CostCorrection::validate_against` now rejects delta currency drift with
`cost_correction_currency_mismatch` so an approved foreign-currency correction cannot enter the
ledger and fail later inside the read fold.  Guards (`kiana-core/tests/bq14_cost_correction_guard.rs`
expanded, `kiana-query/tests/bq14_cost_ledger_projector_guard.rs` expanded) pin the markers, the
refusal-code matrix, every fixture name, the no-update/no-delete and no-second-authority negative
assertions and the manual-lane/auto-shard wiring.

CI wiring: manual `.github/workflows/bq14-cost-correction.yml` (`workflow_dispatch` only,
`permissions: contents: read`, `cargo fetch --locked`, `cargo fmt --all --check`, the five fixture
targets and six `--no-run` compile steps).  Automatic coverage stays with the unified `ci.yml`
shards `kiana-domain-s1/4` (`bq14_cost_correction`), `kiana-core-s1/6` (`bq14_cost_correction`,
`bq14_cost_correction_guard`) and the whole-crate `kiana-query`/`kiana-protocol` shards;
`scripts/ci/test-shards.json` was already complete for these targets.

Limitations of this slice: local tests/builds/checks/clippy are not run (CI-only session) and the
CI receipt is not awaited.  The ledger and reducer remain source contracts: they do not
append/flush EventLog facts, consume or release approvals, provide cross-process CAS or
checkpoints, import provider invoices, allocate FinancialBudget or prove external billing; the
projector has no production route caller yet (BQ-19/BQ-20/BQ-22 and later steps own those).
