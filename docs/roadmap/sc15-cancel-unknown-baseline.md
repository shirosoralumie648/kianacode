# SC-15 Cancellation / Unknown Baseline

## Scope

SC-15 closes the cancellation and uncertain-effect boundary already shared by Runner and
ControlPlane. `RunCancellationFact` is durable intent, not an in-process watch; only a confirmed
stop can become `Cancelled`, while unconfirmed stop or missing/contradictory runner evidence stays
`ResultUnknown`. The Runner state driver exposes explicit cancelling/recovery/unknown transitions,
the Harness interrupts model retry/stream work and emits queued `ToolCancelled` facts, and Core
records cancellation through CAS, waits for capability stop confirmation, fences unknown execution
receipts and keeps unknown resources quarantined.

Human failure incidents and inbox actions are projections over committed facts. Reconciliation
requires independent evidence and explicitly records `automatic_retry_allowed: false`; it never
turns an unknown effect into a successful or silently retried run.

## Evidence and limits

- `kiana-core/tests/sc15_cancel_unknown.rs` covers confirmed-cancel versus result-unknown fact
  invariants and one-way terminal transitions.
- `kiana-core/tests/sc15_cancel_unknown_guard.rs` pins Runner/Core/dispatch/quarantine/reconcile
  markers and rejects automatic retry/false-success bypass strings.
- GitHub Actions runs the fixtures, source guard and workspace compile; local tests are
  intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: external provider reconciliation,
cross-process crash recovery and live/physical stop evidence remain SC-16+ / PD / ER work.
