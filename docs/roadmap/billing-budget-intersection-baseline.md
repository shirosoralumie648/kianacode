# BQ-06 Budget Intersection Baseline

## Scope

BQ-06 adds a pure intersection over the existing `RuntimeBudget`, `ProjectBudget`, `Quota` and
`BudgetLease` plus the new `ProviderBudget`. `EffectiveBudget` takes the conservative minimum for
model calls, tokens, wall time and concurrency, carries project-run and provider token/cost caps,
and remains a digest-bound admission snapshot. `derive_child_lease` rejects any child widening,
usage reset, or reservation reset/widening instead of silently unioning limits.

FinancialBudget is intentionally absent: money/contract capacity cannot grant execution authority.
The calculation does not reserve, dispatch, settle, persist or call a provider; BQ-07 owns quota
windows and BQ-08 owns durable reservations/CAS.

## Evidence and limits

- `kiana-domain/tests/bq06_budget_intersection.rs` covers five-scope minimums, child widening,
  cumulative usage monotonicity/reset, reserved-budget monotonicity, legacy reservation-sentinel
  widening, invalid provider/quota and empty-intersection denial.
- `kiana-core/tests/bq06_budget_intersection_guard.rs` protects the pure/no-financial-authority
  boundary. GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no durable reservation, quota
window, lease fence, queue, provider capacity or financial billing authority is claimed.

## 2026-10-05 focused local receipt

The child-lease fixture now synchronizes `max_model_calls` before asserting the successful narrowed
child; the first widening attempt still proves model-call widening denial. The Core boundary guard
now scans the implementation body rather than the explanatory module comment containing the literal
`FinancialBudget` name.

```text
source_snapshot: current dirty worktree; kiana-domain billing budgets and kiana-core BQ-06 guard
worktree_status: roadmap row 195 remains 🔄; fixture/guard-only correction, no admission or reservation wiring
command_argv: bounded serial cargo test -p kiana-domain --test bq06_budget_intersection --locked --offline -- --test-threads=1; cargo test -p kiana-core --test bq06_budget_intersection_guard --locked --offline -- --test-threads=1
cwd·environment: repository root Linux/bash; one Cargo/test worker, 32 MiB test stack, stripped/non-incremental profile; domain max RSS 227700 KiB, Core 129952 KiB, swaps 0
fixture·cassette: domain 3/3 (five-scope intersection, widening denial/successful narrowed child, invalid/empty denial); Core guard 1/1
exit_code: both commands 0; logs/resources `/tmp/kiana-roadmap-20261005-evidence/bq06-{domain,core}.{log,resources}`
status_change: selected BQ-06 behavior is local_behavior; whole card remains feature_status=implemented, proof_level=source
proof-level_change: selected fixture/guard source → local_behavior; no durable/live/physical promotion
limitations: Continue cumulative usage/reset semantics remain unresolved because derive_child_lease lacks explicit lineage; no ControlPlane admission, quota reservation/CAS, EventLog/provider dispatch, durable or financial authority proof
reviewer: integration agent with BQ-06 read-only audit; no human release reviewer
```

## 2026-10-06 current-head budget monotonicity receipt

```text
source_snapshot: current dirty worktree; kiana-domain/src/billing_budgets.rs; kiana-domain/tests/bq06_budget_intersection.rs; kiana-core/tests/bq06_budget_intersection_guard.rs
worktree_status: roadmap row 195 remains 🔄; production budget derivation now rejects cumulative usage reset and compares normalized reservation limits; no admission/reservation wiring change
command_argv: RED `cargo test -p kiana-domain --test bq06_budget_intersection --locked --offline -- --test-threads=1`; GREEN same target; `cargo test -p kiana-core --test bq06_budget_intersection_guard --locked --offline -- --test-threads=1`; final `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; one Cargo worker and one Rust test thread; stripped/non-incremental test profile; final cached domain 1.46s/max RSS 228280 KiB (~223 MiB), Core guard 1m19.73s/max RSS 4086940 KiB (~3.90 GiB), 0 swaps; `/tmp/kiana-roadmap-20261006-bq06-{final,core}.{log,resources}`
fixture·cassette: domain 6/6 and Core guard 1/1; RED reproduced fresh child usage being accepted after parent consumption; GREEN rejects lower counters as `budget_child_usage_reset_rejected`, higher counters as `budget_child_usage_widening_rejected`, and child zero sentinel widening an explicit parent reservation limit
exit_code: RED 101 expected; final domain/Core/format/diff commands 0
status_change: selected BQ-06 child-lease monotonicity and sentinel boundary recorded as `local_behavior`; whole card remains feature_status=implemented, proof_level=source
proof-level_change: selected budget derivation source → local_behavior; no durable/live/physical promotion
limitations: no explicit lineage identity, ControlPlane admission transaction, quota group/provider binding, durable reservation/CAS, queue/capacity, EventLog/provider dispatch or financial authority proof; BQ-07 scope semantics remain separate
reviewer: root integration agent; BQ-06 read-only billing audit; focused budget reviewer
```

## 2026-10-07 zero-effect read-only budget receipt

`BudgetLease` has always permitted `max_effects=0`, which represents a read-only lease and still
allows zero-effect consumption. `EffectiveBudget::validate` incorrectly rejected that conservative
intersection, so the five-scope calculation returned `effective_budget_invalid`. The validator now
keeps all positive execution dimensions strict while allowing the effect dimension to be zero.

```text
source_snapshot: current dirty worktree; kiana-domain/src/billing_budgets.rs; kiana-domain/tests/bq06_budget_intersection.rs; kiana-core/tests/bq06_budget_intersection_guard.rs
worktree_status: roadmap row 195 remains 🔄 / feature_status=implemented / proof_level=source; zero-effect read-only intersection is locally verified; no admission/reservation or manifest/lockfile change
command_argv: RED `cargo test -p kiana-domain --test bq06_budget_intersection zero_effect_lease_remains_valid_for_read_only_work --locked --offline -- --test-threads=1`; GREEN `cargo test -p kiana-domain --test bq06_budget_intersection --locked --offline -- --test-threads=1`; Core `cargo test -p kiana-core --test bq06_budget_intersection_guard --locked --offline -- --test-threads=1`; final `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; one Cargo worker/test thread; `RUST_MIN_STACK=33554432`; stripped/non-incremental test profile; final domain max RSS 83132 KiB, Core max RSS 85384 KiB in cached reruns; swaps 0; first uncached Core compile reached 4048848 KiB with swaps 0
fixture·cassette: RED returned `effective_budget_invalid`; GREEN domain 7/7 includes zero-effect lease and retains widening/reset/invalid-provider denial; Core no-financial-authority guard 1/1
exit_code: RED 101 expected; GREEN domain/Core/fmt/diff 0
status_change: selected zero-effect intersection behavior moves source → local_behavior; whole BQ-06 remains implemented/source
proof-level_change: read-only zero-effect validation is locally verified; no durable/live/physical promotion
limitations: no lineage persistence, ControlPlane admission transaction, quota/provider scope binding, durable CAS/reservation, queue/capacity, provider dispatch or financial authority; BQ-07 remains separate
reviewer: root integration agent with BQ-06 billing audit; focused budget reviewer
```

## 2026-10-08 ProjectBudget identity denial receipt

`intersect_budgets` now rejects a nil `ProjectBudget.project_id` before calculating any effective
limits. This prevents an unbound project budget from producing a valid-looking digest-bound
intersection while preserving the existing positive-limit and zero-effect semantics.

```text
source_snapshot: current dirty worktree; kiana-domain/src/billing_budgets.rs; kiana-domain/tests/bq06_budget_intersection.rs; kiana-core/tests/bq06_budget_intersection_guard.rs
worktree_status: BQ-06 row 195 remains 🔄 / feature_status=implemented / proof_level=source; selected project identity denial now local_behavior; no reservation/CAS or financial authority wiring
command_argv: RED `cargo test -p kiana-domain --test bq06_budget_intersection project_budget_with_nil_identity_is_rejected --locked --offline -- --exact --test-threads=1`; GREEN domain `cargo test -p kiana-domain --test bq06_budget_intersection --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test bq06_budget_intersection_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo job/test thread, `RUST_MIN_STACK=33554432`, stripped/non-incremental tests; RED max RSS 4083776 KiB / 88% CPU; domain GREEN max RSS 3992416 KiB / 85% CPU; Core guard max RSS 4090588 KiB / 62% CPU; all measured runs reported 0 swaps; logs/resources `/tmp/kiana-roadmap-20261008-bq06-{red,domain,core}.{log,resources}`
fixture·cassette: RED nil `ProjectId` produced an `EffectiveBudget`; GREEN domain 8/8 and Core guard 1/1; stable `project_budget_invalid`, while valid intersection, zero-effect read-only and child monotonicity cases remain green
exit_code: RED 101 expected; GREEN domain/Core 0
status_change: selected BQ-06 project identity denial moves source → local_behavior; whole card remains feature_status=implemented, proof_level=source
proof-level_change: nil project identity guard source → local_behavior; no durable/live/physical promotion
limitations: `EffectiveBudget` still does not carry project identity in its digest; no ControlPlane admission, reservation/CAS, quota/provider binding, durable recovery or financial authority proof
reviewer: root integration agent with BQ-06 read-only billing audit; focused budget reviewer
```

## 2026-10-08 reserved_budget monotonicity denial receipt

`derive_child_lease` now treats the in-flight `reserved_budget` counter as a monotone
lineage value: a child cannot clear a parent reservation or claim a larger reservation,
even when the requested value remains below `max_reserved_budget`. This closes the local
reset/widening gap while leaving the actual reservation authority in BQ-08/ControlPlane.

```text
source_snapshot: current dirty worktree; kiana-domain/src/billing_budgets.rs; kiana-domain/tests/bq06_budget_intersection.rs; kiana-core/tests/bq06_budget_intersection_guard.rs
worktree_status: BQ-06 row 195 remains 🔄 / feature_status=implemented / proof_level=source; selected reserved_budget reset/widening denial is locally verified; no admission/reservation/CAS or financial authority wiring
command_argv: RED `cargo test -p kiana-domain --test bq06_budget_intersection child_reserved_budget_cannot_ --locked --offline -- --test-threads=1`; GREEN domain `cargo test -p kiana-domain --test bq06_budget_intersection --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test bq06_budget_intersection_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo worker/test thread, `RUST_MIN_STACK=33554432`, stripped/non-incremental test profile; RED max RSS 4091068 KiB / 87% CPU; GREEN domain max RSS 4074832 KiB / 87% CPU; Core guard max RSS 4080752 KiB / 86% CPU; all measured runs reported 0 swaps
fixture·cassette: RED old implementation returned `Ok` for both reset and widening fixtures (exit 101); GREEN domain 11/11 includes stable `budget_child_reservation_reset_rejected` and `budget_child_reservation_widening_rejected`; Core guard 1/1 protects the five-scope/no-financial-authority boundary
exit_code: RED 101 expected; GREEN domain/Core 0
status_change: selected BQ-06 reserved_budget monotonicity denial moves source → local_behavior; whole card remains feature_status=implemented, proof_level=source
proof-level_change: local child reservation counter validation source → local_behavior; no durable/live/physical promotion
limitations: no explicit lease lineage identity, ControlPlane admission transaction, durable quota reservation/CAS, cross-process recovery, provider settlement or financial authority proof; in-memory validation does not prove remaining_reserved accounting under concurrent siblings
reviewer: root integration agent with BQ-06 read-only billing audit; focused reservation reviewer
```

## 2026-10-08 BudgetLease identity denial receipt

`BudgetLease::validate` now rejects a nil `BudgetLeaseId` before the five-scope intersection
can produce an apparently valid `EffectiveBudget`. This aligns the lease boundary with the
existing nil-identity checks on budget facts, cell admission resources, recovery snapshots and
project budgets.

```text
source_snapshot: current dirty worktree; kiana-domain/src/work_packets.rs; kiana-domain/tests/bq06_budget_intersection.rs; kiana-core/tests/bq06_budget_intersection_guard.rs
worktree_status: BQ-06 row 195 remains 🔄 / feature_status=implemented / proof_level=source; selected lease identity denial is locally verified; no reservation/CAS or financial authority wiring
command_argv: RED `cargo test -p kiana-domain --test bq06_budget_intersection budget_lease_with_nil_identity_is_rejected --locked --offline -- --test-threads=1`; GREEN domain `cargo test -p kiana-domain --test bq06_budget_intersection --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test bq06_budget_intersection_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo job/test thread, `RUST_MIN_STACK=33554432`, stripped/non-incremental test profile; RED max RSS 227252 KiB / 32% CPU; cached domain GREEN max RSS 83448 KiB / 1% CPU; Core guard max RSS 129524 KiB / 66% CPU; all measured runs reported 0 swaps
fixture·cassette: RED nil `BudgetLeaseId` produced an `EffectiveBudget`; GREEN domain 9/9 and Core guard 1/1; stable `budget_lease_invalid`, while valid intersection, zero-effect read-only, project identity and child monotonicity cases remain green
exit_code: RED 101 expected; GREEN domain/Core 0
status_change: selected BQ-06 nil lease identity denial moves source → local_behavior; whole card remains feature_status=implemented, proof_level=source
proof-level_change: lease identity guard source → local_behavior; no durable/live/physical promotion
limitations: no explicit lease lineage persistence, ControlPlane admission transaction, quota/provider binding, durable reservation/CAS, recovery or financial authority proof; `EffectiveBudget` does not carry lease identity
reviewer: root integration agent with BQ-06 read-only billing audit; focused budget identity reviewer
```

## 2026-10-13 Runtime/Project budget separation receipt

The BQ-06 fixture now directly exercises the roadmap's required separation boundary. A run-scoped
`RuntimeBudget` model-call limit changes `EffectiveBudget.max_model_calls`, while a project-scoped
`ProjectBudget` changes `max_project_runs` and its independent token limit. Neither value is reused
as the other budget's dimension.

```text
source_snapshot: current dirty worktree; kiana-domain/tests/bq06_budget_intersection.rs; existing billing_budgets.rs and Core BQ-06 guard
worktree_status: BQ-06 row 195 remains 🔄 / feature_status=implemented / proof_level=source; selected runtime/project separation fixture is local_behavior; no production authority, manifest/lockfile or second budget loop
command_argv: exact `cargo test -p kiana-domain --test bq06_budget_intersection runtime_and_project_budgets_are_not_interchangeable --locked --offline -- --exact --test-threads=1`; full domain target `cargo test -p kiana-domain --test bq06_budget_intersection --locked --offline -- --test-threads=1`; Core `cargo test -p kiana-core --test bq06_budget_intersection_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode`; single Cargo/compiler job, one Rust test thread, 32 MiB test stack, stripped/non-incremental locked/offline profile; no swap in all receipts
fixture·cassette: exact domain 1/1; full BQ-06 domain 12/12; Core guard 1/1; lowering RuntimeBudget model calls changes only `max_model_calls`, lowering ProjectBudget tokens changes only `max_tokens`, and project run cap remains `max_project_runs`
exit_code: all 0; exact max RSS 232896 KiB / 32% CPU / 1.79s; full domain max RSS 81828 KiB / 64% CPU / 0.22s; Core guard max RSS 132520 KiB / 55% CPU / 0.51s; resource receipts `/tmp/kiana-roadmap-20261010-evidence/bq06-{runtime-project,domain-full,core-guard}.{log,resources}`
status_change: selected runtime/project non-interchangeability boundary moves source → local_behavior; whole BQ-06 and project remain open
proof-level_change: local_behavior only; no durable/live/physical promotion
limitations: remaining/remaining_reserved calculations, explicit lease lineage, sibling CAS, quota windows, ControlPlane admission, durable recovery, provider dispatch and financial authority remain open
reviewer: root integration agent with BQ-06 read-only source audit; focused budget separation reviewer
```
