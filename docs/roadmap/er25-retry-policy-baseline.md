# ER-25 retry policy and new attempt baseline

ER-25 treats retry as a new, bounded attempt rather than a free repeat. The model
contract persists attempt identity, retry class, request-sent/effect state,
retry-after backoff and a hard deadline; the Harness reserves and settles each
attempt against the shared budget and records the attempt projection. Only
`BeforeSend`/`Rejected` model failures with no uncertain effect may retry, and every
new attempt rebuilds/prepares the request and re-enters model admission.

Unknown effects, denied capabilities, cancellation and missing idempotency never
auto-retry. Attempt count, task budget or deadline exhaustion returns a terminal
failure/Unknown classification. GitHub Actions runs H05/H07/H08/H11 fixtures, CP
Unknown/effect guards, the ER-25 source guard and workspace target compilation. Local
runtime tests and smoke commands were not part of the original 2026-09-18 receipt. The
2026-10-06 user request authorizes bounded local testing; current focused receipts are in
`CURRENT_STATUS.md`, while current-head remote CI remains unobserved.

This is source/static evidence only; provider-specific live backoff, cross-process
retry durability and external effect reconciliation remain outside this slice.

The ER-25 Core guard now follows the current ownership boundary: the Harness owns the
bounded attempt loop, `kiana-runner/src/retry.rs` delegates classification, and the
domain `RetryPolicy` owns safe pre-send/rejection, Unknown-effect, observed-delta,
idempotency, retry-after and deadline decisions. No retry execution semantics changed.

## Behavioural fixtures (2026-10-17)

The card's deny-first and regression fixtures now live in `kiana-core/tests/er25_retry_policy.rs`
and drive the real `kiana_domain::RetryPolicy::classify` seam and the `RetryAttemptReservation`
value boundary in-process, exactly as the Runner attempt loop and the ControlPlane dispatch
re-preflight consult them:

- `unknown_effect_never_auto_retries` — a post-send `Unknown` side effect, an already-delivered
  delta and a non-idempotent request each refuse a new attempt (`side_effect_unknown` /
  `observed_delta` / `non_idempotent`).
- `policy_denied_never_retries` — a `Never`/repair disposition (`recovery_disposition_not_retryable`),
  an already-sent request (`request_already_sent`) and a rejected-but-never-sent class
  (`retry_class_not_allowed`) are all refused regardless of remaining budget.
- `retry_exhaustion_is_terminal_failure_or_unknown` — attempt budget, request budget and the
  absolute deadline each end the attempt instead of scheduling another (`attempt_budget` /
  `request_budget` / `deadline`).
- `transient_before_dispatch_creates_a_bounded_new_attempt`,
  `known_no_effect_failure_honors_retry_after_backoff`,
  `absolute_deadline_bounds_backoff_before_a_new_attempt` and
  `duplicate_idempotency_key_is_fenced_at_the_attempt_reservation` cover the success/regression
  list: a bounded new attempt, `Retry-After` backoff, the deadline bound and a single terminal
  per sent attempt reservation.

The `er25_retry_policy_guard` target keeps its source guards and adds
`retry_behavioural_fixtures_are_wired` to pin the fixture names and the
`.github/workflows/er25-retry-policy.yml` wiring; Runner transient/Unknown/cancellation
boundaries stay covered by the existing H05 `typed_recovery_disposition_bounds_runner_routing`
and H11 `unknown_result_never_triggers_automatic_retry` fixtures. GitHub Actions runs the new
`er25_retry_policy` target and its `--no-run` compile step; local runtime tests were not run in
this CI-only session.

This is source/static evidence only. It does not promote provider-socket backoff, cross-process
retry durability or external effect reconciliation to durable/live/physical proof.
