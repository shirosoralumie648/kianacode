# INT-21 connector retry baseline

## Scope

INT-21 defines a connector-specific retry classifier and bounded attempt policy. A retry is a
decision for a new attempt, never a permission to reuse a consumed permit or replay an unknown
effect. The classifier admits only known-no-effect or explicitly declared-idempotent observations
with an idempotency key, within absolute deadline, attempt, backoff and per-attempt timeout limits.

## Implemented source slice

- `ConnectorRetryObservation` accepts only bounded, digest-bound, redacted classification evidence.
  Unknown, approval-denied, epoch-stale, scope-denied, validation-failed and cancelled classes are
  explicit deny outcomes.
- `ConnectorRetryPolicy` enforces attempt/deadline/backoff bounds. `KnownNoEffect` requires no
  request sent and `NoEffect`; `DeclaredIdempotent` additionally requires a provider-declared
  idempotency key and still rejects an effect state that is not `NoEffect`.
- `ConnectorRetryDecision` binds the policy digest and next attempt/delay. It does not call a
  Broker, EventStore, adapter or scheduler; the caller must perform fresh INT-16/INT-18 admission.

## 2026-10-17 completion slice (CI-only)

The completion slice makes every decision server-bound and adds the missing deny-first fixtures,
the daemon boundary fixture, the wiring guard and the manual lane.

### Server-owned retry binding

- `ConnectorRetryBinding` (`kiana.connector-retry-binding.v1`) is derived by
  `ConnectorRetryBinding::from_command(command, retry_mode, idempotency)` from the admitted
  `ConnectorInvocationCommand` plus the admitted operation contract's retry/idempotency modes.
  It carries invocation id, attempt, command digest, idempotency key digest, retry mode,
  idempotency mode and a self-certifying `binding_digest`; a nil invocation, attempt 0, a
  tampered digest or a contradictory `DeclaredIdempotent` + `Required` combination fails closed
  (`connector_retry_binding_invalid`, `connector_retry_binding_idempotency_conflict`).
- `ConnectorRetryPolicy::classify(binding, now_unix_ms, observation)` is the only classification
  entry point. It requires the binding, so a caller can no longer pass its own retry/idempotency
  mode, and evidence that references another invocation's key is refused
  (`connector_retry_idempotency_key_mismatch`) before any decision exists.
- `ConnectorRetryDecision` now carries `binding_digest`; `validate_for_binding` refuses a decision
  applied to another invocation/binding (`connector_retry_decision_binding_mismatch`) or replayed
  as a different next attempt (`connector_retry_decision_attempt_mismatch`). The decision still
  carries no reservation, permit or authorization material.

### Timeout and backoff bounds

- `ConnectorRetryPolicy` gained a bounded `attempt_timeout_ms` (≤ 1 h,
  `MAX_CONNECTOR_RETRY_ATTEMPT_TIMEOUT_MS`). A retry is admitted only when the backoff delay plus
  the new attempt's timeout window still fits before the absolute deadline; otherwise the decision
  denies with `deadline`. Backoff starts at `CONNECTOR_RETRY_BASE_BACKOFF_MS` (100 ms), grows as
  `base * 2^(attempt - 1)` and is clamped by `max_backoff_ms`; a provider `retry_after_ms` wins
  over the smaller step but may not exceed `max_backoff_ms` (`retry_after_too_large`).
- Evidence invariants: a request that was never sent cannot claim an effect
  (`connector_retry_observation_effect_state_mismatch`), and `retry_after_ms` is only valid on a
  throttled observation (`connector_retry_observation_retry_after_invalid`).

### Deny-first fixtures

| Fixture | Denies / proves |
|---|---|
| `unknown_non_idempotent_and_authority_classes_never_retry` | Unknown/approval/epoch/scope/validation/cancel classes, unknown effect after a timed-out send, `Never` mode, forbidden idempotency, request-already-sent evidence and a class outside `KnownNoEffect` mode all deny |
| `attempt_budget_deadline_retry_after_and_the_timeout_window_fail_closed` | exhausted attempt budget, absolute deadline, oversized `retry_after`, and the attempt-timeout window versus the deadline |
| `foreign_or_forged_retry_material_is_refused_before_a_decision` | foreign idempotency key digest, forged observation shapes, tampered observation/policy/binding, out-of-bounds policies, attempt 0/nil invocation, mode conflict and unknown fields in every strict decoder |
| `decisions_cannot_be_replayed_across_bindings_attempts_or_policies` | cross-binding replay, forged next-attempt replay, malformed denial/zero-delay decisions, policy digest mismatch and tampered decision digest |
| `known_no_effect_and_declared_idempotent_retry_as_a_new_bound_attempt` | known-no-effect and declared-idempotent evidence retry deterministically as attempt `n + 1` with bounded delay, and the decision carries no dispatch material |

Daemon boundary fixture `kiana-daemon/tests/int21_connector_retry.rs` (real
`DaemonHost → ControlPlane → Broker → EventStore` path, hash-pinned `local_fixture` binding):

| Fixture | Denies / proves |
|---|---|
| `wire_supplied_retry_material_is_refused_before_any_dispatch` | wire-supplied `retry_decision`/`retry_class`/`retry_policy`/`next_attempt`/`retry_after_ms`/`attempt`/`idempotency_policy`/`connector_retry_binding` → `connector_arguments_invalid`, zero `connector.invoked` facts and an unchanged connector stream; the compatibility invoke still reaches the adapter exactly once with no retry material in the output |
| `a_completed_attempt_is_never_auto_retried_and_facts_keep_one_attempt` | one completed attempt stays attempt 1 in every lifecycle fact, no retry fact or retry-material key is persisted, and replaying the same command returns the original receipt/lifecycle with the stream unchanged |

### Wiring

- `kiana-core/tests/int21_connector_retry_guard.rs` holds two guards: the purity/boundary guard
  (single binding-bound `classify` entry point, deny codes, no broker/event-store/scheduler/
  filesystem/async surface, ControlPlane/daemon never schedule retries) and the wiring guard
  (fixture names, no `#[ignore]`, manual lane steps and `--no-run` count, no automatic triggers,
  `int21_connector_retry` ∈ `kiana-domain-s3/4`, `int21_connector_retry_guard` ∈
  `kiana-core-s4/6`, daemon whole-crate shard).
- Manual lane `.github/workflows/int21-connector-retry.yml` (`workflow_dispatch`-only,
  `permissions: contents: read`): `cargo fetch --locked` + `cargo fmt --all --check` + the three
  targets with `--test-threads=1` + three `--no-run` compilations. Automatic coverage remains the
  existing `ci.yml` shards; `scripts/ci/test-shards.json` is unchanged.
- No `Cargo.toml`/`Cargo.lock`/`package.json` change, no new dependency, no new test target
  outside the existing crate test auto-discovery, no second execution loop; the five model-visible
  tools (`shell`, `apply_patch`, `mcp`, `memory.search`, `memory.write`) are unchanged.

## CI-only evidence

GitHub Actions runs connector retry fixtures, the pure source guard, formatting and workspace
test-target compilation. Local Cargo test/build/check/clippy/smoke commands are intentionally not
run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: implemented
proof_level: source
```

This slice classifies supplied evidence only. It does not schedule retries, create new durable
reservations, prove provider no-effect truth, implement timeout cancellation, reconcile Unknown,
or demonstrate live/physical connector behavior. The daemon still never retries automatically: a
`Retry` decision is an instruction for the caller to build a fresh attempt, and no ControlPlane
path consumes the classifier yet. Those remain INT-22/INT-23 and later integration work.
