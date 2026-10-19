# EQ-43 quality promote/rollback admission baseline

## Scope

`quality.promote` and `quality.rollback` are authority-sensitive admissions, not effects. They
route only through `ControlPlane::handle_command` -> `kiana-core/src/quality_gate.rs`; no
entrypoint, evaluator or Broker owns a second admission path, and `kiana-quality` stays a pure
contract crate with no ControlPlane logic.

The entry point now enforces four independent fences, in this order, before anything is written:

1. **Identity** — trusted project, named actor, reviewer/QA role.
2. **Epoch** — the project's current `authority` ledger epoch must exist and equal the caller's
   `expected_authority_epoch`.
3. **Scope** — the server derives `scope_digest` from canonical project root + session/actor/role/
   department + the explicit `target_path`; the caller's digest must equal it, and the actor's
   `path_allow` must actually cover that target (`allow_list_covers`). A self-consistent digest for
   a target outside the actor scope is still refused.
4. **Approval** — an `approval:` decision must exist, be bound to the exact operation/candidate/
   gate/scope/epoch request hash, be `Approved` + `Approve`, be decided by a different principal,
   and be unexpired.

Only then does the gate call `ApprovalStorePort::prepare_consumption` and commit the
`approval.consumed` transition together with the admission fact in one `TransitionBatch` through
`commit_confirmed`. Consumption and admission land atomically or not at all. The command identity
is derived deterministically from the request hash, so re-submitting the same mutation reads the
committed receipt and returns the same admission with `replayed=true` instead of spending a second
approval.

This slice records an authorized quality mutation admission. It does not change a live provider
route or grant, and it does not add a durable candidate/gate store; that remains a later,
separately authorized transition.

## Refusal codes

| Condition | Stable code |
|---|---|
| Unparseable arguments, bad digest shape, absolute/traversing `target_path`, bad approval ref | `quality_mutation_arguments_invalid` |
| Project not trusted | `project_untrusted` |
| No named actor | `quality_actor_required` |
| Role is not reviewer/QA | `quality_reviewer_role_required` |
| No authority stream / epoch 0 | `quality_authority_epoch_missing` |
| Caller epoch differs from the ledger | `quality_authority_epoch_stale` |
| Scope digest does not match the derived scope (incl. target) | `quality_scope_digest_mismatch` |
| `path_allow` does not cover the target | `quality_target_outside_scope` |
| Recomputable request hash does not match the argument | `quality_approval_request_hash_mismatch` |
| Approval missing/unreadable/expired record, wrong binding, not Approved/Approve, self-decided, already Consumed | `quality_approval_unavailable` / `quality_secondary_approval_invalid` |
| Adapter cannot prepare atomic consumption | `quality_approval_consumption_unavailable` |
| Same command identity with a different digest | `quality_mutation_replay_mismatch` |
| CAS conflict on approval/authority/admission version | `quality_mutation_conflict` |

## Fixtures

`kiana-core/tests/quality_promote_fixtures.rs` — 21 deny-first fixtures over a real
`ControlPlane::handle_command`, `MemoryEventLog` and a scripted approval store that implements
`prepare_consumption` and records consumption counts:

- `passing_eval_cannot_promote_without_authority` (card acceptance name, first)
- `stale_authority_epoch_is_rejected_before_the_approval_is_read`
- `scope_digest_from_another_session_cannot_promote`
- `target_outside_the_actor_scope_cannot_be_promoted`
- `target_path_traversal_is_rejected_before_any_authority_check`
- `tampered_candidate_cannot_reuse_the_original_approval_binding`
- `tampered_report_cannot_self_authorize_with_a_matching_request_hash`
- `missing_approval_record_cannot_promote`
- `approval_store_without_atomic_consumption_cannot_promote`
- `consumed_approval_cannot_promote_a_second_candidate`
- `denied_approval_cannot_promote`
- `expired_approval_challenge_cannot_promote`
- `self_approved_promotion_is_refused_as_separation_of_duties`
- `untrusted_project_cannot_promote`
- `non_reviewer_role_cannot_promote`
- `anonymous_request_cannot_promote`
- `malformed_report_digest_is_rejected_before_any_authority_check`
- `rollback_cannot_bypass_the_same_secondary_authority`
- `rejected_promotion_cannot_be_replayed_into_a_later_authority_epoch`
- `approved_promotion_with_live_authority_is_admitted_and_replays_idempotently` (success)
- `admitted_mutation_cannot_be_reused_for_a_different_candidate`

The success fixture asserts the approval is consumed exactly once, `approval.consumed` and the
admission fact each appear exactly once, and replaying the identical mutation returns the same
admission with `replayed=true`, no second consumption and no new events.

`kiana-core/tests/eq43_quality_mutation_guard.rs` — two source guards: the route/role/epoch/scope/
approval markers and the event field whitelist, plus atomic consumption/receipt replay/target
coverage markers and a negative assertion that the admission fact is not written through the
single-event append path.

## Evidence and limits

- `.github/workflows/eq43-quality-mutation.yml` is `workflow_dispatch` only with
  `permissions: contents: read`; it runs locked fetch, `cargo fmt --all --check`, both core targets
  serially and a `--no-run` test compile.
- Local run: `cargo test -p kiana-core --test quality_promote_fixtures --offline --
  --test-threads=1` (21 passed), `cargo test -p kiana-core --test eq43_quality_mutation_guard
  --offline -- --test-threads=1` (2 passed), `cargo check -p kiana-core --tests --offline`
  (0 errors). Whole-workspace `cargo fmt --all --check` fails on this host with os error 206 (path
  too long); each touched file was checked with `rustfmt --edition 2021 --check` instead.

`feature_status=implemented`, `proof_level=local_behavior` for the two focused core targets only.
Still open: an actual provider route/grant mutation, a durable candidate/gate/approval store, a
second effect receipt, entrypoint wiring and live/physical evidence. The scripted approval store is
a test double; it proves the gate's consumption contract, not a production journal round trip.
