# BQ-17 · Retry classifier, attempt reservation and cancellation baseline

This source slice makes provider retry a typed, bounded decision over a new attempt.  It keeps
the existing `DaemonHost → ControlPlane → ModelBudgetPort/Broker → ProviderGateway` execution
spine: the retry policy never dispatches a request or grants authority.

## Source contract

- `RetryPolicy` accepts only typed pre-send failures or an explicit 429/408 response whose
  side-effect state is `None`.  Post-send unknown, observed deltas, TLS/connect ambiguity,
  authentication errors, 503 and non-idempotent observations are terminal.
- A retry decision checks attempt count, request count and the same absolute deadline used by
  admission and transport.  `Retry-After` seconds/date values remain visible as bounded delay
  evidence; values above `MAX_RETRY_AFTER_MS` or beyond the deadline do not schedule an early
  request.
- `RetryAttemptReservation` binds one run/attempt/reservation identity, retry ordinal, token
  upper bound, deadline and optional admission lease digest.  It records at most one request and
  one terminal state (`settled`, `unknown` or `cancelled`), so cancellation cannot race a second
  terminal outcome.
- Runner allocates a fresh reservation and attempt identity for every retry, settles usage before
  deciding the next attempt, and records the reservation in the model-turn evidence.  Existing
  BQ-15/BQ-16 budget and capacity paths remain the accounting/dispatch boundaries.
- Provider SDK implicit retries remain disabled.  The provider parser keeps Retry-After parsing
  injectable for fixtures and does not copy response bodies or credentials into errors.

## CI-only completion slice (2026-10-17)

The original slice landed with bounded source contracts but only thin fixtures and a manual lane
that was later removed by the workflow fan-in.  This completion keeps the source contracts
unchanged and closes the card's denial surface and acceptance column with executable, deny-first
fixtures plus a rebuilt dispatch-only lane.

- `kiana-domain/tests/bq17_retry_policy.rs` (3 → 15): deny-first fixtures for TLS/auth/503/
  post-send-unknown/permanent failures, `observed_delta`/non-idempotent/phase drift/repair
  dispositions, stable refusal-code strings, attempt/request/deadline budgets, oversized
  Retry-After and deadline crossing, bounded exponential backoff, policy/observation/decision
  validation (including forged digests and injected phase/code), contract round-trips with
  `deny_unknown_fields`, a bounded retry ledger loop proving one dispatched request per attempt
  with the request count matching the attempt ledger, and the reservation fences: one request and
  one terminal state, cancellation cannot race a later settle/unknown, observed usage cannot
  exceed the reserved bound, and nil identities/forged request counts/states/digests are refused.
- `kiana-provider/tests/bq17_retry_policy.rs` (2 → 3): pins the injectable Retry-After function,
  the 429/408-only typed rejection mapping, `ModelRetryClass::Never` elsewhere, disabled reqwest
  implicit retries, the four transport unit fixture names and the absence of a second retry loop
  or error-body copying.
- `kiana-runner/tests/bq17_retry_policy_guard.rs` (2 → 3): one classification site per attempt,
  reservation/accounting markers, settlement before the next attempt, the retry reservation in
  the model-turn evidence, the cancellable backoff wait, and negative markers for a second retry
  driver.
- `kiana-core/tests/bq17_retry_policy_guard.rs` (2 → 4): projection marker/refusal-code/fixture
  matrix, single `parse_retry_class` helper and call site, read-only projection negatives
  (`reqwest::`, `retry_delay(`, sleep, `RetryPolicy::new(`), and lane/shard wiring.
- `kiana-daemon/tests/bq17_retry_policy_guard.rs` (new): the daemon composes `DaemonHost` and must
  not own retry classification, reservations or a retry loop.

## CI-only fixtures

GitHub Actions `bq17-retry-policy.yml` runs domain classifier/reservation fixtures, provider
Retry-After/status source fixtures plus the in-crate transport unit filters, Runner
reservation/cancellation guards and Core/daemon lifecycle guards, then compiles the same targets
with `--no-run`.  The lane is `workflow_dispatch`-only with `permissions: contents: read`; the
automatic lane stays the unified `ci.yml` shards (`kiana-domain-s1/4` → `bq17_retry_policy`,
`kiana-core-s1/6` → `bq17_retry_policy_guard`, whole-crate `kiana-provider`/`kiana-runner`/
`kiana-daemon`).  Local tests, builds, checks, clippy and smoke commands are intentionally not
run; CI receipts are not awaited.

## Evidence ceiling and limitations

The slice is `feature_status=implemented` with `proof_level=source` and remote-CI wiring.  The
reservation reducer is an in-process value contract; it does not append/flush EventLog facts,
provide cross-process CAS or expose a provider billing receipt.  Capacity lease binding is an
opaque digest supplied by the admission adapter, and the runner retry driver is only exercised by
source guards, not by a runtime fixture that drives `KianaHarness` end to end.  Live provider
behavior, external billing, invoice reconciliation, crash recovery and physical side-effect proof
remain open.
