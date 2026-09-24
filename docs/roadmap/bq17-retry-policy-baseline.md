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

## CI-only fixtures

GitHub Actions `bq17-retry-policy.yml` runs domain classifier/reservation fixtures, provider
Retry-After/status source fixtures, Runner reservation/cancellation guards and Core lifecycle
guards.  The workflow then compiles workspace test targets.  Local tests, builds, checks, clippy
and smoke commands are intentionally not run.

## Evidence ceiling and limitations

The slice is `feature_status=implemented` with `proof_level=source` and remote-CI wiring.  The
reservation reducer is an in-process value contract; it does not append/flush EventLog facts,
provide cross-process CAS or expose a provider billing receipt.  Capacity lease binding is an
opaque digest supplied by the admission adapter, and live provider behavior, external billing,
invoice reconciliation, crash recovery and physical side-effect proof remain open.  CI results
are intentionally not awaited.
