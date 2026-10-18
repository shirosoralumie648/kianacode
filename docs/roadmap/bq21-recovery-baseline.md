# BQ-21 billing restart and recovery baseline

> Snapshot date: 2026-09-27. This slice adds a strict source fact for restart, continue,
> replay and recovery around an in-flight billing reservation and an uncertain model attempt.
> Local Cargo test/build/check/clippy/smoke commands are intentionally not run; GitHub Actions
> owns the fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-21`](../roadmap.md#step-bq-21) |
| source snapshot | `a9111a00` plus this BQ-21 source slice |
| feature_status | `partial` for the source recovery and fencing contract |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | committed recovery fact → Core validation facade → caller-owned reconciliation |

`BillingRecoveryFact` keeps the attempt and reservation digest, source cursor, authority epoch and
lease epoch together. Restarted work is paused or remains `NeedsReconciliation` while usage is
unknown. `Recovered` requires an explicit reconciliation reference, known usage and exactly one
settlement; paused/reconciliation states carry zero settlements. `Continue` cannot reset
accumulated usage. The epoch fields make an old lease visible to the caller so a stale worker
cannot be treated as the current authority by this contract.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| default recovery | paused and needs-reconciliation states remain distinct; unknown is visible |
| continue fence | a continue request that resets usage is rejected |
| success fence | unknown usage cannot be represented as recovered/automatic success |
| single-settlement fence | recovered state cannot claim more than one settlement |
| epoch fence | zero lease/authority epochs fail closed |
| digest/shape fence | digest tampering and unknown fields are rejected |
| read-only boundary | Core/domain source contains no provider dispatch, EventLog append or reservation release |

## CI and limitations

GitHub Actions runs `.github/workflows/bq21-recovery.yml` with formatting, the domain fixtures, the
Core source guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and
smoke commands are intentionally not run, and CI results are not awaited.

Limitations: this is a caller-supplied source contract. It does not restart workers, rebuild an
EventLog projection, perform CAS/reconciliation, settle a reservation, or prove that an external
provider effect was known. Durable recovery and live/physical billing remain unproven.

## 2026-10-17 completion slice (CI-only)

The source slice above was kept; this pass closes the card's deny column (a restart that re-sends a
completed attempt, a `Continue` that resets accumulated usage, an old lease/epoch that keeps
settling, unknown usage turned into automatic success) and its success column (recovery defaults to
paused/needs-reconciliation; after an explicit reconcile the attempt settles exactly once) with
deny-first fixtures and one admission boundary.

| Added / hardened | Where | Why |
|---|---|---|
| `BillingAttemptSnapshot` + `BillingBootFence` + `BillingRecoveryRequest` | `kiana-domain/src/billing_recovery.rs` | one server-owned recap (disposition, accumulated units, settlement count, source event/cursor) and one boot fence (boot generation, authority epoch, lease epoch, source cursor) that a caller assertion is compared against; the assertion never becomes authority |
| `plan_billing_recovery` | domain | refuses `billing_recovery_attempt_already_terminal` (completed attempt re-dispatch), `billing_recovery_continue_resets_accumulation`, `billing_recovery_boot_generation_stale` / `_authority_epoch_stale` / `_lease_epoch_stale` / `_epoch_ahead`, `billing_recovery_attempt_binding_mismatch`, `_source_event_mismatch`, `_source_cursor_regression`, `billing_recovery_unknown_requires_reconciliation` and `billing_recovery_reconcile_requires_evidence`; accepted plans are `Paused` (reservation still held) or `NeedsReconciliation` (unknown, still occupied) |
| `BillingRecoveryPlan` + `BillingRecoveryLedger` | domain | a plan can never set `redispatch_permitted`, `Recovered` requires exactly one settlement, `Paused` without a held reservation is only reachable from an evidence-backed release, and the append-only ledger refuses a second settlement (`billing_recovery_second_settlement`), a skipped reconciliation (`_reconciliation_skipped`), a downgrade (`_decision_downgrade`) and any other conflicting decision (`_replay_conflict`) while replaying an identical plan |
| `BillingReconciliation` + `BillingAttemptSnapshot::settled_by` + `reconcile_attempt` | domain | the explicit reconciliation is bound to the snapshot and fence (`billing_recovery_reconcile_binding_mismatch` / `_source_mismatch`), refuses an in-flight (`_reconcile_requires_unknown`) or already settled attempt (`_reconcile_already_settled`), refuses evidence that still reports unknown usage (`billing_recovery_unknown_cannot_be_success`), refuses a release claiming consumption (`billing_recovery_release_claims_consumption`) and advances the committed snapshot to one settlement |
| `BillingRecoveryPlan::matches_fact` | domain | the decision and the committed `BillingRecoveryFact` must agree on attempt, settlement count and unknown usage (`_plan_fact_attempt_mismatch` / `_plan_fact_settlement_mismatch` / `_plan_fact_unknown_mismatch`) |
| `BillingRecoveryAdmission` + `ControlPlane::admit_billing_recovery_restart` / `_reconciliation` / `_plan` | `kiana-core/src/billing_recovery.rs` | Core only classifies the Domain refusal codes; it never re-implements the epoch, cursor, accumulation or settlement rule and never appends a fact, dispatches a provider or releases a reservation |

Fixture matrix after this slice (deny first): domain 3 → 13
(`restart_never_redispatches_a_completed_attempt`,
`continue_never_resets_accumulated_usage`,
`stale_boot_lease_and_authority_epochs_cannot_continue_settlement`,
`unknown_attempts_default_to_needs_reconciliation_and_never_auto_succeed`,
`explicit_reconciliation_settles_exactly_once`,
`in_flight_reservation_stays_held_across_restart_and_replay`,
`recovery_ledger_cannot_skip_reconciliation_or_downgrade_a_decision`,
`recovery_requests_and_snapshots_are_digest_and_binding_fenced`,
`reconciliation_evidence_is_bound_and_cannot_claim_a_release_with_consumption`,
`recovered_facts_and_plans_agree_on_one_settlement` plus the three kept fact fixtures); core 1 → 6
(`recovery_admission_refuses_stale_fences_terminal_attempts_and_second_settlement`,
`control_plane_facade_exposes_only_the_single_recovery_rule`,
`recovery_sources_own_no_dispatch_append_release_or_second_authority`,
`refusal_codes_and_fixture_names_stay_pinned_in_the_fixtures`,
`bq21_fixtures_are_wired_to_the_manual_lane_and_the_auto_shards` plus the kept marker fixture).
Guards pin markers, the refusal-code matrix, fixture names, the absence of update/delete and
second-authority paths, the unchanged five-tool model surface and the
`scripts/ci/test-shards.json` bindings (`kiana-domain-s1/4` lists `billing_recovery`;
`kiana-core-s1/6` lists `billing_recovery_guard`; the registry itself is unchanged).

Manual lane: `.github/workflows/bq21-recovery.yml` (`workflow_dispatch`-only,
`permissions: contents: read`, `cargo fetch --locked`, `cargo fmt --all --check`, the two test
targets with `--test-threads=1` and two `--no-run` compile steps). The automatic lane stays the
unified `ci.yml` shards above.

Known limits after the completion slice: local tests/build/check/clippy/smoke were not run and CI
receipts are not awaited, so the proof ceiling stays `source`. This is still a caller-supplied
value contract: it does not restart workers, rebuild an EventLog projection, perform CAS, release or
settle a reservation itself, and it cannot prove that an external provider effect was known. Durable
recovery, cross-process crash safety, invoice reconciliation, budget allocation and live/physical
billing evidence remain BQ-22/BQ-23/BQ-24/BQ-26 and later.
