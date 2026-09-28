# DEP-24 crash / restore / backup rehearsal and RPO/RTO measurement baseline

> Snapshot date: 2026-09-28. This slice owns the *decision and the measurement contract* for a
> recovery rehearsal: whether a run that somebody else performed may be published as evidence that
> the declared recovery objectives were met. It does not perform the run. Local Cargo
> test/build/check/clippy/smoke commands are intentionally not run by the integration owner; GitHub
> Actions owns fixtures and the workspace gate.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-24`](../roadmap.md#step-dep-24) |
| code landing | `kiana-domain/src/recovery_objective.rs` and `kiana-core/src/recovery_rehearsal.rs`, registered by each crate's `lib.rs` |
| fixtures | `kiana-domain/tests/dep24_recovery_objective.rs` (deny-first, the success path last) and `kiana-core/tests/dep24_recovery_rehearsal_guard.rs` (source guard) |
| feature_status | `partial` for the source-level objective, measurement, evidence and lease-binding contracts |
| proof_level | `source`; no `local_behavior`, durable, live or physical promotion |
| canonical path | declared objective + supplied measurement -> fixed-order `derive` -> digest-sealed report -> re-derivation -> core facade bound to a real `OperationLease` |

**There was no process kill, no restore, no backup and therefore no measured RPO or RTO number in this slice.**

Every number in the fixtures is a literal written into a test file to make a rule observable. Nothing here has ever been recovered from anything.

## RPO and RTO are declarations, not observations

The whole slice turns on one distinction. A **recovery point objective** says how much history a
restore is allowed to lose; a **recovery time objective** says how long service is allowed to be
unavailable. Both are numbers somebody wrote down in advance, and both are allowed to be
optimistic -- that is what a target is. What is not allowed is for the optimism to quietly become a
fact, which is why `RecoveryObjective` carries budgets and `RecoveryMeasurement` carries
observations and the two are only ever compared, never merged.

The comparison is deliberately asymmetric. Losing more history than the RPO allows is a miss
(`recovery_rpo_objective_exceeded`); taking longer than the RTO allows is a miss
(`recovery_rto_objective_exceeded`). A run that sits exactly on the budget is inside it, because
the objective is a ceiling and not a target to beat.

## What the card rejects, and how

| Rejected | How |
|---|---|
| 声明的 RPO 优于实际恢复点 | `evaluate_rpo` refuses `observed_data_loss_ms > rpo_max_data_loss_ms` as `recovery_rpo_objective_exceeded` |
| 声明的 RTO 短于实际恢复耗时 | `evaluate_rto` refuses `observed_recovery_ms > rto_max_recovery_ms` as `recovery_rto_objective_exceeded` |
| 恢复后 cursor 越过 source cursor | `evaluate_cursor_monotonicity` refuses `recovered_cursor > source_cursor` as `recovery_cursor_ahead_of_source`; losing history is what the RPO budgets, inventing it is not a restore |
| 恢复后 cursor 回退 | the same function refuses `recovered_cursor < restored_root_cursor` as `recovery_cursor_regressed`. A resume behind a checkpoint the quarantined root had already verified is a *second* loss, and no RPO budget covers it |
| 旧 lease 继续 effect | `evaluate_fence_state` refuses `old_lease_still_effective` as `recovery_stale_lease_still_effective`, and the core facade independently refuses a post-recovery lease that is not `Active` (`recovery_rehearsal_lease_not_active`) |
| 旧 fence 继续生效 | `evaluate_fence_state` refuses `old_fence_token_still_effective` as `recovery_stale_fence_token_still_effective`; the core facade refuses a reissued token as `recovery_rehearsal_fence_token_reused` by comparing the real `OperationLease`'s token against the measurement's `superseded_fence_token` |
| 恢复后误 dispatch | `evaluate_fence_state` refuses `stale_dispatch_observed` first, as `recovery_stale_dispatch_after_restore` -- a command that already ran is the harm, and the two flags above it are only descriptions of how it got there |
| 进程重启当成 durable 恢复 | `evaluate_durability` refuses a `Succeeded` run with `durable_recovery: false` as `recovery_restart_not_durable`. `ResultUnknown` is deliberately *not* promoted by this rule: it falls through to `recovery_rehearsal_outcome_not_succeeded`, because deciding what actually happened is DEP-25's reconciliation and not this slice's guess |
| rehearsal 自报、无绑定 | `evaluate_evidence_completeness` refuses `recovery_evidence_exit_code_missing` and `recovery_evidence_limitations_missing`; a measurement whose `objective_digest` is not this objective's is refused with `recovery_measurement_objective_binding_invalid` |

### The decision order is fixed

`derive` checks in one order so the reported reason is the first violated rule and therefore the
same for the same facts. The first group is **positive observations** about the restored root: a
dispatch that already happened, a writer that still holds its old lease, a fence token that is
still accepted, a cursor in a position no source ever had, a resume behind a verified checkpoint.
Each is a live writer or an invented fact rather than an absence of proof, so each outranks what
follows. Then come the two honesty invariants -- a restart is not a restore, and a run that did
not succeed measures nothing -- which no operator intent can waive. Finally the absences of proof,
ordered by how cheap the caller's fix is: capture the exit code, state the limitations, or
re-measure against a target the run actually met.

`RecoveryRehearsalReport::validate_against` re-derives the whole decision from the same two inputs
and refuses any report whose status, numbers, cursors, reason or remediation no longer follows
(`recovery_rehearsal_report_binding_invalid`). Re-sealing the digest around an edit does not help.
Independently, a within-objective decision must carry no reason and a missed one must always name
the rule it hit (`recovery_rehearsal_report_reason_incoherent`), so a report cannot present itself
as a plain success while hiding which rule refused it.

## A rehearsal is not evidence until it is bound to something

`RecoveryRehearsalEvidence` is the evidence block one run must be bound to: the worktree state it
started from, the argv that produced it, its exit code, a digest over the environment, the
fixture or cassette it was driven from, and **what it did not prove**. The last field is required
and a placeholder is refused: writing `none`, `n/a`, `TBD`, `-` or `unknown` in the one field
reserved for limitations is how an unproven run gets published as a clean one, so those strings are
rejected by `unstated`.

The exit code and the limitations are checked by `evaluate_evidence_completeness` rather than by
`validate` on purpose. An absent exit code and an unstated limitation are structurally serializable,
so "the run happened" and "the run is evidence" stay two different questions with two different
answers instead of collapsing into one shape check.

## The core facade adds the one thing the domain leaves out

`kiana-core/src/recovery_rehearsal.rs` binds a rehearsal decision to the single-writer state the
restore actually left behind, taken as a real `kiana_domain::OperationLease` rather than as a
boolean an adapter set. `evaluate_post_recovery_lease` refuses, in order: a restored root that never
reached readiness (`recovery_rehearsal_restored_root_not_ready`), a lease that is not the active
one (`recovery_rehearsal_lease_not_active`), a lease minted under the pre-fault fence token
(`recovery_rehearsal_fence_token_reused`), and authority or data epochs that did not strictly
advance past the measurement's own (`recovery_rehearsal_authority_epoch_not_advanced`,
`recovery_rehearsal_data_epoch_not_advanced`).

`RecoveryRehearsalOutcome::seal` runs those checks *before* the report is derived, so a restore that
left no serving root is refused before its RPO and RTO are even computed. The `publishable` flag is
derived from `report.within_objective()` and re-checked on every validation
(`recovery_rehearsal_outcome_publish_flag_invalid`), so an outcome that missed its objective is not
publishable however small the miss was, and one that met it cannot be quietly withheld.

## What was deliberately *not* built here

The card's reject list also names retention destroying evidence. That is **SC-42's** work, and this
slice does not restate it: there is no retention, legal-hold or prune decision in either module,
and `RecoveryRehearsalEvidence` has no field that could be read as one. The one thing this slice
does about it is refuse to publish a rehearsal whose evidence block is missing or whose limitations
were never stated, which is the narrowest possible version of the same concern.

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| zero RPO / zero RTO / zero declaration time | `recovery_objective_rpo_invalid`, `recovery_objective_rto_invalid`, `recovery_objective_header_invalid` |
| measurement bound to a foreign objective | `recovery_measurement_objective_binding_invalid` |
| lost one millisecond past the RPO | `recovery_rpo_objective_exceeded`; exactly on the budget is `Ok` |
| took one millisecond past the RTO | `recovery_rto_objective_exceeded`; exactly on the budget is `Ok` |
| recovered cursor past the source | `recovery_cursor_ahead_of_source` |
| resume behind the restored root's own checkpoint | `recovery_cursor_regressed` |
| stale dispatch plus a live lease, a live fence, an RPO miss, an RTO miss and no exit code | the reported reason is still `recovery_stale_dispatch_after_restore` |
| live lease and live fence together | `recovery_stale_lease_still_effective`; fence alone is `..._fence_token_still_effective` |
| success without a durable re-read | `recovery_restart_not_durable` |
| `ResultUnknown` and `Denied` | `recovery_rehearsal_outcome_not_succeeded`, never a success and never the more flattering restart code |
| unknown that also claims durability | still `recovery_rehearsal_outcome_not_succeeded` |
| no exit code | `recovery_evidence_exit_code_missing` |
| `none` / `None` / ` n/a ` / `TBD` / `-` / `unknown` / empty limitations | `recovery_evidence_limitations_missing`; a stated limitation is `Ok` |
| source cursor zero / epoch zero | `recovery_measurement_cursor_invalid`, `recovery_measurement_epoch_invalid` |
| empty argv / credential in argv / `file://` cassette | `recovery_evidence_command_argv_invalid`, `..._not_redacted`, `recovery_evidence_fixture_cassette_invalid` |
| report edited and re-sealed | `recovery_rehearsal_report_binding_invalid`; edited without re-sealing, `..._digest_mismatch` |
| within-objective report relabelled as missed, and the reverse | `recovery_rehearsal_report_reason_incoherent` |
| periodic ledger: replay, contradiction, duplicate | idempotent replay, `recovery_rehearsal_identity_digest_conflict`, `recovery_rehearsal_ledger_duplicate_rehearsal` |
| all seven named faults | the same rules apply to each, and a stale dispatch is refused identically under each |
| success (last) | within objective, empty reason, `"none"` remediation, every standalone judgment `Ok`, and the report re-derives |

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate, which auto-discovers
`kiana-core/tests/*.rs` and `kiana-domain/tests/*.rs`. No separate workflow was added, per the CI
consolidation rule that a new roadmap step must never multiply automatic CI fan-out.

This is a source contract over values somebody else produced. It **does not** kill a process, take
or restore a backup, copy or scan a root, create or quarantine a directory, start or stop a timer,
read the wall clock, append an event, dispatch a capability or persist a ledger. Every observation in
`RecoveryMeasurement` is an adapter-reported claim; the domain can require that the claims are
present, well formed, mutually consistent and not flattering, but it cannot require that they are
true, so an adapter that reports a fictional clean run defeats this check exactly the way a lying
`present_artifacts` inventory defeats DEP-22's scan. Minting or carrying an `OperationLease` value
here does not persist it: a later step must do that, exactly as `OperationLeaseCas` documents for
itself, and must revalidate at the effect boundary. The `Command`-shaped facts in
`RecoveryRehearsalEvidence` are supplied by whoever ran the rehearsal, and a command that genuinely
crashed a process is a harness this repository does not contain.

Two coverage gaps are named rather than papered over. First, the core facade's lease rules have a
source guard but no behavioural fixture in this slice, because building a valid `OperationLease`
chain in a fixture would be the first place this slice started constructing effects; the guard
asserts the decision codes and the absence of an effect boundary, and the behavioural coverage
belongs with the slice that persists the lease. Second, nothing is wired into
`ControlPlane::handle_command` -- `ControlPlane` remains the only place a command becomes an effect,
and routing a rehearsal through it is open work rather than something this source contract pretends
to have done. `result_unknown` reconciliation stays with DEP-25, retention and legal hold stay with
SC-42, and the fault fixtures themselves stay open: this slice names the seven faults and refuses a
bad rehearsal, it does not inject one. Nothing here is `local_behavior`, durable, live or physical
evidence.
