# CO-14 symposium, blackboard and decision governance baseline

## Scope

CO-14 adds a governance-bound domain ledger around the existing `Symposium` and
`DecisionRecord` contracts. It binds a meeting to Project/baseline/agenda, named assignments,
chair, round limits and output schema; contributions are typed Claim/Vote/Draft records with
visibility and evidence; decisions retain alternatives, dissent, unresolved items and explicit
Sponsor approval identity.

## Implemented source slice

- `SymposiumGovernance` binds Project, baseline version, agenda reference, chair assignment,
  attendee assignment map, max rounds, output schema and canonical digest. A department
  composition validates that every attendee role resolves to the same `DepartmentSpec` and that
  the chair is that department's `can_convene` role (`convene_chair_id`); roles spanning
  departments are not a department meeting.
- `SymposiumAssignmentScope` (`kiana.symposium-assignment-scope.v1`) declares, per assignment,
  the visible contribution kinds and `may_decide`. `SymposiumGovernance::new` derives the default
  department scopes (all kinds, chair decides); `SymposiumGovernance::joint` requires the caller
  to supply them explicitly.
- `SymposiumBoard` rejects uninvited, stale, duplicate and out-of-scope contributions
  (`symposium_contribution_scope_denied`), filters `visible_to` by the declared kinds instead of
  the unconditional chair shortcut, and refuses decisions from assignments without decision
  authority (`symposium_decision_authority_denied`, `symposium_decision_chair_required`).
- Vote weight is advisory. A decision whose only cited evidence is board vote records is refused
  with `symposium_vote_does_not_approve`; Sponsor approval stays a separate attachment that
  requires the sponsor role and a declared decision scope (`symposium_sponsor_required`,
  `symposium_sponsor_scope_denied`).
- Joint compositions are a visibility/decision-scope contract only. Every named assignment must
  carry an explicit scope, at least one assignment must be scope-limited, and a deciding
  assignment must be a `can_convene` role (`symposium_joint_scope_undeclared`,
  `symposium_joint_decision_authority_denied`). No joint meeting is convened: the frozen product
  rule in `symposiums.rs` (`joint_symposium_frozen`) is untouched.
- Existing `Symposium`/`DecisionRecord` and the current ControlPlane/Harness meeting path remain
  available; no second meeting runner or effect loop was added.

## Acceptance fixtures

| Fixture | Lan | Assertion |
|---|---|---|
| `symposium_rejects_uninvited_stale_and_duplicate_contributions` | deny | uninvited/stale/duplicate contributions refused; an uninvited assignment sees nothing |
| `symposium_refuses_scope_breach_votes_as_approval_and_foreign_sponsor_approval` | deny | a vote-only decision is refused; a non-sponsor cannot attach approval; duplicate attachment refused |
| `symposium_governance_refuses_undeclared_scope_and_non_convening_department` | deny | missing/mismatched assignment scopes and a non-convening chair are refused; cross-department roles are not a department meeting |
| `symposium_governance_contract_is_usable_by_all_five_departments` | success | one governed board per `DepartmentSpec::catalog()` id, chair from `convene_chair_id()` |
| `joint_symposium_lists_visibility_and_decision_scope_per_assignment` | success | each joint assignment lists visible kinds and decision authority; out-of-scope record/decision refused; frozen joint meeting still refused |
| `symposium_decision_preserves_alternatives_dissent_and_evidence` | success (card, final) | alternatives/dissent/unresolved/evidence preserved; sponsor approval separate |

## Evidence block

```text
source_snapshot: step/co-14-20261018-2 @ e4611ee3 (compile repair) plus this CO-14 slice
worktree_status: D:/code/kiana-steps/co14, branch step/co-14-20261018-2; local focused tests run on Windows Git Bash
command_argv: cargo test -p kiana-domain --test co14_symposium_governance --locked --offline -- --test-threads=1; cargo test -p kiana-core --test co14_symposium_governance_guard --locked --offline -- --test-threads=1
cwd·environment: D:/code/kiana-steps/co14; CARGO_TARGET_DIR=D:/code/kiana-steps/target-co14; Windows 11, stable Rust; --locked --offline; --test-threads=1
fixture·cassette: kiana-domain/tests/co14_symposium_governance.rs (6 fixtures, deny-first, success last); no provider, filesystem or external effect contacted
exit_code: domain 0 (6 passed, 0 failed); core 0 (5 passed, 0 failed, 3 of them shared source_guard self-tests)
status_change: roadmap row 513 🔄 → ✅; CO-14 card 🔄 → ✅; per-assignment scope, vote/approval separation and five-department/joint fixtures added
proof-level change: proof_level=local_behavior for the two focused targets; feature_status=implemented for the domain contract slice
limitations: the board is not yet the persistence/projector for the live symposium path; no durable cross-process replay, UI projection, model session or Broker effect is exercised; CI lane not awaited; joint composition is a contract only, never convened
reviewer: local focused test run above; no independent runtime reviewer
```

## CI-only evidence

`.github/workflows/co14-symposium-governance.yml` is workflow_dispatch-only with
`permissions: contents: read`; it fetches locked dependencies, gates formatting, runs both
targets serially with `--test-threads=1` and compiles each with `--no-run`. Both targets stay
registered in `scripts/ci/test-shards.json` (`kiana-domain-s2/4`, `kiana-core-s2/6`).

## Limitations and handoff

- The governance board is a pure contract and is not yet the sole persistence/projector for the
  existing live symposium path; durable cross-process replay and UI/private-brief projection remain
  later work.
- Assignment authority, ProjectTrust, approval consumption and model session scheduling remain
  ControlPlane/Harness responsibilities. No live model, source-code write, external effect,
  durable, live or physical outcome is claimed.
- Evidence ceiling: `local_behavior` for the two focused targets; the wider workspace and CI are
  not awaited.
