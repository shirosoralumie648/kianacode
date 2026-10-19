# CO-15 bounded symposium runtime baseline

## Scope

CO-15 records the finite runtime admission boundary for the existing `ControlPlane::convene_symposium`
path. The chair drives a deterministic schedule over the department's named roles; each member speaks
in its own session through the existing Harness (`start_run` on round 0, `continue_run` afterwards).
A speaker turn is only a structured proposal. `kiana-domain` validates the shape and the governed
board decides whether it may be recorded; Core supplies contribution identity and assignment from the
schedule, never from the payload. Round, message, token, wall-time and stall limits are explicit and
accounted in `SymposiumRunBudget`. Both the convened and anti-meeting paths pass the same
`budget.decision_allowed()` gate immediately before `meeting.close(anti_meeting)`, so budget
exhaustion or cancellation is refused with a stable code and can never become an approved decision.

## Implemented source slice

- `kiana-domain/src/symposium_runtime.rs`: `SymposiumRunBudget` (round/message/token/wall-time/stall
  accounting plus cancellation; `limit_reason()`/`decision_allowed()` fail closed), and
  `SymposiumContributionProposal` (`deny_unknown_fields`) with `parse_symposium_contributions()`.
  Refusal codes: `symposium_round_budget_exhausted`, `symposium_message_budget_exhausted`,
  `symposium_token_budget_exhausted`, `symposium_wall_time_exhausted`,
  `symposium_stall_limit_exhausted`, `symposium_cancelled`, `symposium_run_budget_invalid`,
  `symposium_contribution_invalid`, `symposium_contribution_kind_invalid`,
  `symposium_contribution_visibility_invalid`.
- `kiana-core/src/collaboration.rs::convene_symposium`: deterministic per-role schedule, one private
  session per member, per-turn token/wall accounting, structured-output validation, governed-board
  record before blackboard publication, `private_brief` never broadcast, member failure/cancel
  refused with `symposium_member_failed`/`symposium_member_cancelled` (no silent substitution), one
  shared decision gate for both paths.
- `kiana-domain/src/symposiums.rs`: the speaker prompt asks for the structured contribution with
  evidence. `kiana-domain/src/event_contracts.rs`: `run.completed` admits the `structured` field so
  the parsed proposal can be an audited terminal fact.
- Existing Harness/cassette (`ScriptedModel`) remains the only model path; no second runner, broker
  or execution loop is introduced.

## Fixtures (deny first)

- `kiana-domain/tests/co15_symposium_runtime.rs` (5 fixtures): budget exhaustion/cancellation,
  unshaped/unknown-field proposals, board scope and private-scratch refusals, vote-only decision
  refusal, then the shared decision-gate success fixture last.
- `kiana-core/tests/control_plane.rs` (real Harness): `convene_refuses_unstructured_member_contribution`,
  `convene_refuses_evidence_free_and_unknown_field_contributions`,
  `convene_refuses_private_scratch_broadcast_and_failed_member_substitution`,
  `convene_two_rounds_uses_private_speaker_sessions`, then
  `convened_and_anti_meeting_share_one_decision_gate` last (both `symposium.closed` events validate
  and carry the same `kiana.decision-record.v1` contract).
- `kiana-core/tests/co15_symposium_runtime_guard.rs` (2 fixtures): the typed contract surface, and the
  wiring/deny-first-order/lane/shard guard.

## Local evidence

Run on this Windows host with `--offline` and `CARGO_TARGET_DIR=D:/code/kiana-steps/target-co15`:

```text
cargo test -p kiana-domain --test co15_symposium_runtime --offline -- --test-threads=1
  -> 5 passed; 0 failed

cargo test -p kiana-core --test co15_symposium_runtime_guard --offline -- --test-threads=1
  -> 5 passed; 0 failed (2 CO-15 guards + 3 support guards)

cargo test -p kiana-core --test control_plane --offline convene -- --test-threads=1
  -> 5 passed; 0 failed
```

Formatting was verified per touched file with `rustfmt --edition 2021 --check`; `cargo fmt --all
--check` is unavailable on this Windows host (os error 206, path too long).

`.github/workflows/co15-symposium-runtime.yml` extends the existing manual lane: `workflow_dispatch`
only, `permissions: contents: read`, `cargo fetch --locked`, the format gate, both CO-15 targets
serial, the convene fixtures, and `--no-run` compilation. CI was not awaited.

```text
feature_status: local_behavior
proof_level: local_behavior
```

## Limitations and handoff

- The budget value object is not a live scheduler/clock and does not prove provider usage or
  external effects; fixtures use the cassette `ScriptedModel`, not a live provider.
- Durable restart/replay projection of an interrupted meeting, UI projection, live model calls and
  physical outcomes remain open. Replay here means the recorded `symposium.closed` fact validates and
  reproduces the same `DecisionRecord` shape for both paths; it is not a durable-store restart test.
- No joint symposium is convened: `Symposium::validate` still refuses cross-department attendees
  (`joint_symposium_frozen`), and the governance board remains a contract only.
