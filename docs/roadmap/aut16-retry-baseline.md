# AUT-16 retry attempt baseline

AUT-16 turns "may this failed automation attempt be retried?" into a bounded, auditable decision:
an effect-started-but-unknown outcome becomes reconciliation, and a retry is only planned when the
failure is retryable, the receipt and approval/budget/deadline/lease fences hold, and re-sending the
effect is justified (pure/no-dispatch, or an explicitly idempotent capability with an idempotency
key). The classifier never reserves an execution, re-authorizes, dispatches an effect or writes an
event.

## 2026-09-27 classifier slice

The first slice added a strict retry input/decision to `kiana-domain`
(`AutomationRetryInput`/`classify_automation_retry`) plus a pure Core facade
(`kiana-core::classify_retry`), a domain fixture and a Core marker guard. It stated that
`.github/workflows/aut16-retry.yml` ran those targets; commit `08552ada` then consolidated 704
per-slice workflows and deleted that file, so the claim went stale. The manual lane is rebuilt in
this slice and now runs both the legacy classifier targets and the new ones. That slice stayed
`feature_status=partial`, `proof_level=source`: no attempt, lease, deadline or idempotency
descriptor existed.

## 2026-10-17 completion slice (CI-only)

No local Cargo test/build/check/clippy/smoke was run and no CI receipt is awaited; `proof_level`
stays `source`.

**1. Workflow classifier (`kiana-workflow/src/retry.rs`).**
`plan_automation_retry_attempt(&AutomationRetryRequest)` is a pure, clock-free classifier over a
caller-sealed fact snapshot. It returns exactly one of `NewAttempt(Box<AutomationRetryAttempt>)`,
`ReconcileUnknown` or `Refused(AutomationRetryRefusal)` and evaluates in a fixed deny-first order:

| order | condition | outcome |
|---|---|---|
| 1 | request schema/digest/shape invalid | `automation_retry_request_invalid` (error, no plan) |
| 2 | `effect_started && !effect_known` | `ReconcileUnknown` (never retried) |
| 3 | `effect_started && !effect_receipt_present` | `automation_retry_effect_receipt_missing` |
| 4 | `previous_action_digest != action_digest` | `automation_retry_action_changed` |
| 5 | `!failure_retryable` | `automation_retry_not_retryable_failure` |
| 6 | `attempt >= max_attempts` / `observed_at_ms >= deadline_ms` | `..._attempt_bound_exhausted` / `..._deadline_exceeded` |
| 7 | an unexpired `active_lease_expires_at_ms` | `automation_retry_lease_held` |
| 8 | `!approval_valid` / `approval_expires_at_ms <= observed_at_ms` | `..._approval_invalid` / `..._approval_expired` |
| 9 | `budget_remaining == 0` | `automation_retry_budget_insufficient` |
| 10 | descriptor class/idempotency/key: `PureNoDispatch`, `NonIdempotent`, missing key, or no descriptor while `effect_started` | `..._effect_class_mismatch` / `..._non_idempotent_capability` / `..._idempotency_key_missing` / `..._descriptor_required` |
| 11 | `retry_after_ms >= remaining` / lease end beyond deadline | `..._retry_after_exceeds_deadline` / `..._lease_exceeds_deadline` |
| 12 | everything passes | `NewAttempt` with `attempt = failed + 1` |

Success paths are therefore exactly: a pure/no-dispatch failure that never touched an external
effect, and a known failure of an `ExternalEffect + Idempotent` capability with an idempotency key.
The snapshot binds `execution_id`, `subject_id`, `previous_attempt_id`, both action digests, the
attempt bound, the lease/deadline plan and the new attempt identity; `request_digest` is recomputed
by `AutomationRetryRequest::seal()` so any field change invalidates the snapshot.

**2. Attempt/lease/deadline facts and ledger (`kiana-workflow/src/retry.rs`).**
`AutomationRetryAttempt` records the new attempt number, source and fresh execution IDs, subject,
previous attempt, frozen action digest, capability/descriptor digest, idempotency key, `lease_id`,
`lease_owner`, `lease_expires_at_ms`, `deadline_ms`, `created_at_ms`, planned `retry_after_ms` and
the `decision_digest` of the request that produced it; `validate()` refuses attempt numbers below
2 or above the bound, a lease that does not sit inside `(created_at, deadline]`, a `retry_after`
that already crosses the deadline, capability/descriptor/key inconsistencies and a stale
`attempt_digest`. `AutomationRetryAttemptLedger::record_retry_attempt()` is the auditable chain:

| check | refusal |
|---|---|
| same `(subject, attempt)` with the identical digest | safe replay (`replayed = true`), no second record |
| same `(subject, attempt)` with another digest | `automation_retry_attempt_conflict` |
| one `attempt_id` mapped to another record | `automation_retry_attempt_conflict` |
| one idempotency key bound to another attempt | `automation_retry_idempotency_key_conflict` |
| attempt number that is not `max_recorded + 1` | `automation_retry_attempt_sequence_invalid` |
| a recorded lease still unexpired at `created_at_ms` | `automation_retry_lease_held` |

`retry_attempt_history(subject)` returns the ordered chain, `active_retry_lease(subject, now_ms)`
the latest unexpired lease, and `validate_retry_attempt_start(attempt, worker_id, now_ms)` refuses a
foreign lease owner (`..._lease_owner_mismatch`), a clock rollback (`..._clock_rollback`), an
expired lease (`..._lease_expired`) or a passed deadline (`..._deadline_exceeded`).

**3. Idempotency descriptor (`kiana-workflow/src/retry.rs`).**
`AutomationIdempotencyDescriptor { capability, effect_class, idempotency, idempotency_key_scope }`
is the only declaration that can justify re-sending an effect. `ExternalEffect + Idempotent`
requires an idempotency-key scope; every other combination must leave the scope empty; the digest is
self-checked. `NonIdempotent` capabilities never enter automatic retry.

**4. Core coordinator (`kiana-core/src/automation_retry_attempt.rs`).**
`record_automation_retry_attempt(store, ledger, command_id, request)` re-fetches the authoritative
`AutomationDispatchCommand` (the caller cannot supply its own failure facts) and maps command state
to a decision: `Prepared` → `Refused(automation_retry_command_not_failed)`,
`Observed + Succeeded` → `Refused(automation_retry_command_succeeded)`, `Claimed`/`Unknown` →
`ReconcileUnknown(automation_retry_command_outcome_unknown)`, `Observed + Failed` → the classifier
with `effect_started = effect_known = true` and the structured `retryable` flag. Execution, attempt,
authority epoch and action digest come from the command; lease, approval, budget, deadline,
external receipt and descriptor come from the sealed request. The active lease is derived from the
ledger, not from the caller. Outcomes are `AttemptRecorded` / `AlreadyRecorded` / `ReconcileUnknown`
/ `Refused`; **no** branch claims, observes or commits a command fact, and no branch calls a runner,
Broker or model.

## Fixtures and CI wiring

Deny-first fixtures (local run not performed):

| target | count | focus |
|---|---|---|
| `kiana-workflow/tests/aut16_retry_attempt.rs` (new) | 10 | Unknown-first ordering, missing receipt/descriptor/key/action drift, approval/budget/attempt/deadline/lease fences, pure and idempotent success paths, ledger replay/conflict/sequence/lease/idempotency-key refusals, start gate and tamper closure |
| `kiana-core/tests/aut16_retry_command.rs` (new) | 7 | authoritative command state mapping, 0 store mutations on every path, one auditable attempt plus idempotent replay, receipt/idempotency/approval/budget/bound refusals, ledger lease fence, request validation before any read |
| `kiana-core/tests/aut16_retry_command_guard.rs` (new) | 2 | boundary markers and forbidden effect strings, read-only store negative assertions, refusal-code coverage, fixture-name binding, manual lane and shard binding |

The manual lane `.github/workflows/aut16-retry.yml` (`workflow_dispatch` only,
`permissions: contents: read`) fetches locked dependencies, runs the fmt gate, runs all five targets
serially (`--test-threads=1`) and compiles them with `--no-run`. Automatic coverage: the unified
`ci.yml` shards — `automation_retry` in `kiana-domain-s1/4`, `automation_retry_guard` plus the new
`aut16_retry_command` / `aut16_retry_command_guard` in `kiana-core-s1/6`, and whole-crate discovery
for `kiana-workflow` (`targets: null`).

## Known limits

- The attempt ledger is an in-process `BTreeMap`: no capacity bound, expiry cleanup, persistence or
  cross-process fencing, so a recorded attempt is not a durable fact. AUT-08/AUT-15 stores own the
  durable queue/command side and are not wired to it.
- The coordinator reads the AUT-15 store but never commits a new dispatch command for the next
  attempt: preparing and claiming it still goes through the existing AUT-15 path. Nothing in this
  slice is reachable from a production entrypoint yet.
- Descriptor, approval, budget, external receipt and clock values are caller declarations; the
  classifier does not query a capability registry, approval store, budget ledger or provider. A
  `NewAttempt` is a plan, never authorization.
- `subject_id` stability across retries is a caller obligation and is not verified, and the
  first recorded attempt number of a chain is accepted from the caller.
- No Broker/handler/provider/receipt path is exercised: nothing here is `local_behavior`,
  `durable`, `live` or `physical` evidence, and cancellation/stop-report fences, boot recovery and
  compensation remain AUT-17+/AUT-20+/AUT-21.
