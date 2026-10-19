# AUT-20 compensation baseline

> Snapshot date: 2026-10-19. AUT-20 turns the compensation plan into a bound contract between an
> **original** execution/attempt/action and a **distinct** compensation execution/action under a
> **fresh authorization epoch**, and admits that plan against the real durable workflow state. The
> domain validator refuses a reused permit, a non-advancing epoch and an identity collision; the
> workflow admission planner proves the original instance is a known terminal failure and that the
> compensation runs under its own fresh execution. No compensation workflow is executed: the effect
> is always `None` and the compensation nodes still need a normal `Advance`.

## Scope

| Item | Record |
|---|---|
| roadmap card | [`AUT-20`](../roadmap.md#step-aut-20) (row 562, W7) |
| branch | `step/aut20-20261018-1`, worktree `D:/code/kiana-steps/aut20` |
| feature_status | `partial` — domain contract and workflow admission planner exist; no runtime caller, no dispatched compensation |
| proof_level | `local_behavior` — the three focused Cargo targets below pass locally (`--offline`, single test thread) |
| canonical path (target) | `AutomationCompensationPlan` → `plan_compensation_admission` → durable `plan_command(Compensate)` → `AutomationCompensationAdmission` → later `Advance` through ControlPlane → Broker/Harness |

The v1 slice only had a boolean “reused permit” claim and a nonzero epoch check. It carried no
original instance/execution identity and was never checked against real workflow state, so it could
not express the card's exit condition. v2 (`kiana.automation-compensation.v2`) is a new schema; v1
payloads are refused by schema drift (`automation_compensation_plan_invalid`).

## Typed contracts

| Surface | Contract | Deny-first behavior |
|---|---|---|
| plan DTO | `AutomationCompensationPlan` (`kiana.automation-compensation.v2`, `deny_unknown_fields`, `seal()` recomputes and then validates the digest) | schema/field shape/digest drift or unknown JSON fields → `automation_compensation_plan_invalid` |
| identity | `original_instance_id` / `original_execution_id` vs `compensation_instance_id` / `compensation_execution_id` | equal instance or execution identity → `automation_compensation_identity_collision` |
| permit reuse | `reused_original_permit` flag **or** canonical `fresh_authorization_digest == original_authorization_digest` | both forms → `automation_compensation_original_permit_reused`; digest comparison lowercases the hex so `sha256:AAA…` cannot bypass it |
| action | `compensation_action_digest` vs `original_action_digest` | canonical equality (case variants included) → `automation_compensation_action_unchanged` |
| authorization freshness | `authority_epoch` vs `original_authorization_epoch` | `authority_epoch <= original_authorization_epoch` (and zero) → `automation_compensation_authorization_not_fresh` / `_plan_invalid` |
| admission | `AutomationCompensationAdmission` (`kiana.automation-compensation-admission.v1`), `deny_unknown_fields`, self-digest, strictly ascending `selected_nodes` | internal drift → `automation_compensation_admission_invalid` |
| admission checks | source instance exists; status is `Failed`/`Cancelled`; `compensation_execution_id == authority.execution_id`; original execution exists with the declared attempt; no prior compensation child; at least one succeeded source node declares a compensation node; `plan.authority_epoch == live_authority_epoch` and `> original_authorization_epoch` | `_original_instance_missing`, `_original_not_terminal`, `_execution_identity_mismatch`, `_original_attempt_missing` / `_original_attempt_mismatch`, `_already_requested`, `_not_declared`, `_authorization_stale` |
| original immutability | after planning, the source instance value must be byte-identical to the pre-plan value | `_original_mutated` (defensive assertion) |
| durable planning | `AutomationCommand::Compensate` through the existing `plan_command` | planner codes `workflow_compensation_*` are mapped onto the same AUT-20 code set; no second state machine |

The planner's `Compensate` branch creates the compensation instance with `parent_instance_id =
original instance` and `selected_nodes` = the declared compensation nodes of the succeeded source
nodes, in lexicographic order. It deliberately does **not** charge the source instance's remaining
`max_steps` budget (`create_instance(..., charge_parent_budget = false)`): a compensation is an
independent instance that references the original, not a nested child, so a failed source that
already consumed steps must remain compensatable. The `SubWorkflow` and trigger-child paths keep
`charge_parent_budget = true`.

## Acceptance fixtures

Deny-first order is pinned by the Core source guard (success fixture last).

`kiana-domain/tests/automation_compensation.rs` (5 tests):

- `reused_original_permit_is_refused_by_flag_and_by_identical_digest` — flag reuse, identical
  digest, and a case-only digest variant all return `automation_compensation_original_permit_reused`.
- `authorization_epoch_must_be_strictly_after_the_original` — epochs 4/3 refuse with
  `_authorization_not_fresh`, 0 with `_plan_invalid`, 5 (strictly greater) is accepted.
- `compensation_cannot_reuse_the_original_instance_execution_or_action_identity` — same instance or
  execution → `_identity_collision`; same action (including a case variant) → `_action_unchanged`.
- `structural_drift_and_unknown_fields_fail_closed` — v1 schema, tampered digest, malformed digest,
  zero attempt, blank instance ID, unknown JSON field.
- `a_valid_plan_binds_original_and_compensation_with_a_fresh_epoch` (card success) — valid plan,
  digest binds the changed field, typed round-trip.

`kiana-workflow/tests/aut20_compensation_admission.rs` (7 tests):

- `source_instance_must_exist_and_be_known_failed_or_cancelled` — missing instance, `Ready` and
  `Succeeded` sources are refused before any compensation instance exists.
- `original_execution_and_attempt_must_match_a_real_node_attempt` — an unknown execution ID and a
  mismatched attempt are refused separately.
- `compensation_must_use_fresh_execution_and_live_authorization_epoch` — compensation execution must
  be the authority execution; live epoch 4/6/0 is stale; a plan inheriting the original epoch is
  refused by the domain contract before admission.
- `compensation_action_must_differ_from_the_original_action` — identical action digest is refused.
- `second_compensation_for_one_source_is_refused` — the first plan is admitted, the second is refused
  with `_already_requested`; only one selected-node instance exists.
- `source_without_declared_compensation_nodes_is_refused` — a failed source whose succeeded node
  declares no compensation node is refused.
- `admitted_compensation_references_the_original_and_leaves_it_unchanged` (card success) — effect is
  `None`, the source instance is byte-identical, the compensation instance references the original
  with `selected_nodes = ["undo"]` and status `Compensating`, the admission binds plan digest, original
  execution/attempt and the live epoch, and no node execution is created by planning.

The Core source guard `kiana-core/tests/automation_compensation_guard.rs` (2 tests) pins the schema
and field markers, all 15 refusal codes, lowercase digest comparison, the live-epoch comparison, the
`plan_command`/`Compensate` reuse, the forbidden-symbol list (Broker, model client, process spawn,
Tokio task, EventStore append, `DaemonHost`, `KianaHarness`, protected-event commit), the Core facade
signatures and exports, the fixture names with deny-before-success ordering, no `#[ignore]`, and the
lane/shard wiring.

## Evidence

```text
source_snapshot: branch step/aut20-20261018-1; baseline e3ad600e "fix(windows): portable snapshot
  walk, path probes, CLI stack and author identity"; v1 slice (schema v1, boolean permit claim,
  nonzero-epoch check) rewritten into schema v2 + workflow admission
worktree_status: D:/code/kiana-steps/aut20; modified kiana-domain/src/automation_compensation.rs,
  kiana-domain/tests/automation_compensation.rs, kiana-workflow/src/{durable,lib}.rs,
  kiana-core/src/{automation_compensation,lib}.rs,
  kiana-core/tests/automation_compensation_guard.rs; added kiana-workflow/src/compensation.rs,
  kiana-workflow/tests/aut20_compensation_admission.rs,
  .github/workflows/aut20-compensation.yml; no push, no merge, D:/code/kianacode untouched
command_argv: CARGO_TARGET_DIR=D:/code/kiana-steps/target-aut20 CARGO_BUILD_JOBS=4 \
  RUST_MIN_STACK=33554432 cargo test -p kiana-domain --test automation_compensation \
  --offline -- --test-threads=1            (5 passed, 0 failed, exit 0)
  cargo test -p kiana-workflow --test aut20_compensation_admission --offline -- --test-threads=1
                                            (7 passed, 0 failed, exit 0)
  cargo test -p kiana-core --test automation_compensation_guard --offline -- --test-threads=1
                                            (2 passed, 0 failed, exit 0)
  cargo check -p kiana-domain -p kiana-workflow -p kiana-core --offline (exit 0)
  rustfmt --edition 2021 --check on each touched Rust file (exit 0; cargo fmt --all --check fails on
  this host with os error 206)
cwd·environment: D:/code/kiana-steps/aut20; Windows 11, Git Bash; offline; single test thread
fixture·cassette: in-memory AutomationState built from the existing durable planner (RegisterDefinition
  -> Start -> two Advance calls); no provider, Broker, EventStore, filesystem or network contact
exit_code: 0 for all three focused targets
status_change: roadmap row 562 🔄 -> ✅; compensation now binds original instance/execution/attempt and
  a fresh epoch, reusing the original permit is refused in two independent forms, and the admission
  planner proves the original terminal state and the independent compensation execution
proof-level change: source -> local_behavior for the three focused targets; feature_status stays
  `partial` because nothing dispatches or executes a compensation
limitations: no runtime caller; no ControlPlane wiring (the Core facade is read-only and
  handle_workflow_command does not route through it); live_authority_epoch is caller-supplied and is
  not read from an AuthorityLedger; fresh_authorization_digest is not verified against approval
  events, scope or budget; the original execution is checked only against the in-memory state
  projection, not the EventStore; compensation nodes are selected but never advanced, so there is no
  independent receipt or audit trail beyond the admission fact; state is in-memory, no durable
  store/replay/crash recovery; no provider, live or physical proof; the pre-existing AUT-15
  `automation_dispatch_worker` fixture `dispatch_command_is_refetched_and_replays_without_a_second_effect`
  fails identically with this slice stashed, and `validate-doc-references.sh` reports 7 pre-existing
  NM-15 references; neither is caused by this slice and neither was fixed here
reviewer: local source and focused-test review in this session; refusal ordering, digest
  canonicalization, original-immutability assertion and the effect-free boundary were checked
  manually; no independent runtime reviewer
```

## CI

`.github/workflows/aut20-compensation.yml` is manual-only (`workflow_dispatch`, `permissions:
contents: read`) and runs locked fetch, the rustfmt gate, the three focused targets with
`--test-threads=1` and `--no-run` compilation. Both domain/core targets are already claimed by the
existing chunked shards `kiana-domain-s1/4` (`automation_compensation`) and `kiana-core-s1/6`
(`automation_compensation_guard`); the `kiana-workflow` shard discovers targets automatically, so
`scripts/ci/test-shards.json` needs no new entry. The lane has not been dispatched; no CI receipt
was observed, so the local `local_behavior` proof stands on the commands above only.
