# AUT-19 bounded fan-out/fan-in baseline

> Snapshot date: 2026-10-18. AUT-19 adds a domain fan-out **ledger** that validates child scope,
> budget, role, TTL and cycle against real values, records started/observed/failed/skipped facts,
> refuses a start after fail-fast, merges child evidence in a stable order and derives the parent
> state from those facts only. The Core surface is a read-only facade. Nothing here creates or runs
> a child.

## Scope

| Item | Record |
|---|---|
| roadmap card | [`AUT-19`](../roadmap.md#step-aut-19) (row 561, W7) |
| source snapshot | `e4611ee3` ("fix: restore workspace compile on origin/master"), branch `step/aut-19-20261018-2` |
| feature_status | `partial` — domain contract + ledger and Core facade only; no durable planner or runtime caller |
| proof_level | `local_behavior` — the two focused Cargo targets below pass locally |
| canonical path (target) | workflow planner → `AutomationFanoutPlanV2` → `AutomationFanoutLedger` facts → ControlPlane/Broker for any real child execution |

The old v1 `AutomationFanoutPlan` DTO, its `validate()` and the `automation_fanout_plan_invalid`
code stay intact for existing callers. The two caller-declared booleans are no longer trusted:
`validate()` now also rejects a cycle that is visible in the real child ID list, and
`validate_with_scopes()` / `derive_declarations()` check the scope claim against real `ScopeSet`
values and return dedicated mismatch codes.

## Typed contracts

| Surface | Contract | Deny-first behavior |
|---|---|---|
| parent boundary | `AutomationFanoutParent` (`parent_execution_id`, role, allowed child roles, `ScopeSet`, budget, TTL, depth/max_depth, max_concurrency, fail_fast) | invalid identity/role list/budget/TTL/concurrency → `automation_fanout_parent_invalid`; `depth >= max_depth` → `automation_fanout_depth_exceeded`; broken scope → `automation_fanout_scope_invalid` |
| child binding | `AutomationFanoutChildPlan::admit(parent, id, role, scope, budget, ttl)` | child id == parent → `automation_fanout_cycle_refused`; role not in the parent allow-list → `automation_fanout_role_widening`; `child ∩ parent != child` → `automation_fanout_scope_widening`; budget/TTL beyond the parent → `automation_fanout_budget_exceeded` / `automation_fanout_ttl_exceeded` |
| plan | `AutomationFanoutPlanV2` (`kiana.automation-fanout.v2`) | empty/oversized child list, duplicate ids, per-child budget and `checked_add` sum overflow, TTL, role, scope and digest are all revalidated without trusting the constructor |
| ledger | `AutomationFanoutLedger` | `start_child` refuses unknown (`_child_unknown`), already started (`_child_already_started`), skipped (`_child_skipped`) and over-concurrency (`_concurrency_exceeded`); `observe_child` refuses unstarted (`_child_not_started`) and invalid evidence (`_evidence_invalid`) |
| fail-fast | `AutomationFanoutFact::Skipped` with fixed reason `automation_fanout_fail_fast_skip` | not-yet-started children are marked `Skipped` in lexicographic order and a later `start_child` is refused |
| fan-in | `merge(&[AutomationFanoutFanInInput])` → `AutomationFanoutMerge` | duplicate `(child_id, sequence)` → `_fan_in_duplicate`; unknown child → `_fan_in_unknown_child`; admitted child with no input → `_fan_in_missing_child`; non-terminal child → `_fan_in_incomplete`; sequence 0 → `_fan_in_sequence_invalid` |
| parent state | `AutomationFanoutParentState` derived by `parent_state()` | no mutable status field and no setter; `Admitted` / `Running` / `Failed` / `Succeeded` are pure folds of recorded facts |

Scope containment is proven with the shared domain primitives: the child must equal
`child_scope.intersect(parent_scope)`, falling back to `child_scope.is_subset_of(parent_scope)` when
the intersection is empty (so an empty allow-set stays a subset rather than being misread as
authorization). Role containment uses the parent's explicit `allowed_child_roles` allow-list, not a
`RoleSpec` hierarchy; real grants remain ControlPlane work.

## Acceptance fixtures

Deny-first fixtures live in `kiana-domain/tests/automation_fanout.rs` (success fixtures last):

- `child_scope_widening_is_refused_with_a_stable_code`
- `budget_role_ttl_and_child_shape_beyond_parent_are_refused`
- `cycle_parent_and_duplicate_children_are_refused`
- `start_after_fail_fast_is_refused_and_unknown_or_unstarted_children_are_refused`
- `started_children_are_bounded_by_parent_concurrency`
- `fan_in_refuses_duplicate_unknown_missing_and_incomplete_inputs`
- `legacy_plan_declarations_are_checked_against_real_scopes`
- `fail_fast_skip_order_is_lexicographic_and_never_starts`
- `admitted_two_child_plan_fails_one_skips_the_other_and_merges_deterministically` (card success)

The Core source guard `kiana-core/tests/automation_fanout_guard.rs` (two tests) pins the contract
markers, the 24-code refusal matrix, the facts-only parent state, the absence of
Broker/handler/`DaemonHost`/`KianaHarness` symbols in both files, the Core facade signatures and
exports, the fixture names with deny-before-success ordering, the manual lane commands and the shard
bindings.

## Evidence

```text
source_snapshot: e4611ee3 (origin/master compile repair) + this AUT-19 completion slice
worktree_status: D:/code/kiana-steps/aut19 on step/aut-19-20261018-2; modified domain src/tests,
  core src/tests/lib export; added .github/workflows/aut19-fanout.yml and this baseline
command_argv: CARGO_TARGET_DIR=D:/code/kiana-steps/target-aut19 CARGO_BUILD_JOBS=4 \
  cargo test -p kiana-domain --test automation_fanout --locked --offline -- --test-threads=1
  (9 passed, 0 failed, exit 0);
  cargo test -p kiana-core --test automation_fanout_guard --locked --offline -- --test-threads=1
  (2 passed, 0 failed, exit 0);
  rustfmt --edition 2021 on the four touched Rust files (exit 0)
cwd·environment: D:/code/kiana-steps/aut19; Windows 11, Git Bash; rustc/cargo 1.96.1; offline + locked
fixture·cassette: in-memory ScopeSet/budget/role/TTL/cycle fixtures; no provider, filesystem,
  Broker, handler, EventLog or network contact
exit_code: 0 for both focused targets
status_change: AUT-19 🔄 unchanged by this slice except the completion annotation below; ledger,
  fail-fast skip and stable fan-in fixtures added
proof-level change: feature_status=partial (domain + Core facade, no runtime caller);
  proof_level=local_behavior for the two focused targets
limitations: no durable workflow-planner wiring, no ControlPlane/Broker dispatch, no child
  execution or concurrency, no EventLog append/replay, no crash recovery, no real budget lease,
  no RoleSpec hierarchy derivation, no provider/live/physical proof; the ledger is in-memory and
  its parent state is a fold of facts, not persisted truth
reviewer: local source review of the refusal ordering and effect-free boundary; no independent
  runtime reviewer
```

## CI

`.github/workflows/aut19-fanout.yml` is manual-only (`workflow_dispatch`, `permissions: contents:
read`) and runs locked fetch, the rustfmt gate, the domain fixtures, the Core guard and `--no-run`
compilation. Both targets are already claimed by the existing chunked shards
`kiana-domain-s1/4` (`automation_fanout`) and `kiana-core-s1/6` (`automation_fanout_guard`), so
`scripts/ci/test-shards.json` needs no new entry.
