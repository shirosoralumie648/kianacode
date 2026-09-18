# AUT-02 Trusted Clock Baseline

## Scope

AUT-02 adds the versioned `ClockObservation` contract and `ClockPort` boundary needed before
automation timers, leases, approvals or trigger expiry can become durable.  Every observation
contains wall time, monotonic time, source, revision, previous sample references, trust state and
a canonical digest.  Zero/overflow samples and revision regressions fail closed; wall or monotonic
rollback is persisted as `rollback`/untrusted rather than silently treated as a future time.

`require_trusted_deadline` is the shared port gate: an untrusted observation cannot authorize or
extend an expiry.  The existing evaluation-only `Clock` trait remains as a compatibility surface;
AUT-09/AUT-10 will inject `ClockPort` into the scheduler and workflow/trigger adapters.  No
scheduler, timer, Broker call or second execution loop is added by this step.

## Evidence and limits

- `kiana-domain/src/clock.rs` is a strict, digest-bound, restart-serializable observation and
  transition contract with `allows_before`/`clamp_deadline` fail-closed helpers.
- `kiana-ports::ClockPort` samples wall/monotonic time and exposes the common trusted-deadline gate;
  fake-clock behavior, rollback and persistence fixtures run in GitHub Actions.
- `kiana-core/tests/aut02_clock_guard.rs` protects the automation boundary and confirms no second
  scheduler loop is introduced.  Local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: existing core automation still
uses its legacy process-local `SystemTime` helper until a later composition change injects a
ClockPort, and no cross-restart durable clock store, timer worker, live scheduler or physical proof
is claimed.  Unknown/untrusted time therefore remains a hard deny only where the new gate is used;
AUT-03+ must wire it into definition, lease and trigger admission.

