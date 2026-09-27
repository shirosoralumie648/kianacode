# SW-10 cancellation, drain and fence baseline

## Scope

SW-10 adds a strict parent/child cancellation fact. It distinguishes Requested, Draining,
Cancelled, ResultUnknown and Fenced states, binds cancel generation and authority epoch, requires
stop confirmation before Cancelled, and only permits a late result after it is explicitly Fenced.

The contract is observation-only. It does not signal a process, drain a runner, retry an effect,
append an Incident/EventLog fact or reconcile an external result.

## Evidence and limits

- `kiana-domain/tests/swarm_cancellation.rs` covers visible Cancelled versus ResultUnknown, late
  result fencing, generation/stop confirmation and strict fields/digests.
- `kiana-core/tests/swarm_cancellation_guard.rs` protects the no-effect fence boundary.
- `.github/workflows/sw10-cancellation.yml` runs fixtures, source guard, formatting and affected
  target compilation in GitHub Actions; local Cargo tests/build/check/clippy/smoke commands were
  not run.

This slice is `feature_status=partial`, `proof_level=source`: real process drain, supervisor
signals, EventLog recovery, effect reconciliation and live/physical outcomes remain unproven.
