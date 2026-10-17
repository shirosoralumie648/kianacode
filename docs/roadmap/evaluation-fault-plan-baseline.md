# EQ-15 Evaluation Fault Plan Baseline

## Scope

EQ-15 adds a pure `EvalFaultPlan`/`EvalFaultEvidence` reducer for approval denial/expiry,
cancel races, crash-after-effect, restart, stale leases and explicit `result_unknown`. Plans are
strictly tagged, digest-bound and validate unsafe combinations before application. The reducer
never sleeps, starts a process, calls a Broker/EventStore, or treats a dropped/cancelled effect as
success.

Started effects with an unconfirmed stop, crashes, restarts and explicit unknown outcomes produce
`UnknownReconcile` with `effect_known=false` and `requires_reconciliation=true`. A cancel before
an effect with confirmed stop produces `CancelledNotStarted`; approval and stale-lease faults are
known denials. Stop evidence and reconciliation requirements remain visible in the typed result.

## Evidence and limits

- `kiana-daemon/tests/eq15_fault_plan.rs` covers every declared fault class, deterministic
  disposition/terminal codes, stop/Unknown evidence and tamper/unsafe-combination rejection.
- `kiana-core/tests/eq15_fault_plan_guard.rs` protects the pure no-process/no-network/no-second-
  loop boundary. GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not yet drive a real
DaemonHost cancellation/restart or lease worker, persist recovery facts, capture process/file/
network evidence, or prove live/physical effects. Those remain EQ-16+ and ER/PD work.

## Fail-closed verdict bridge

`EvalFaultEvidence::validate` re-checks the schema, plan/evidence digests, terminal code and
disposition/`effect_known`/`requires_reconciliation` consistency, so a rewritten digest or
disposition is rejected before it can be trusted. `blocks_eval_pass` is true for every
reconciliation-required or unconfirmed-effect outcome, and `apply_fault_invariant` downgrades a
candidate `EvalVerdict::Pass` to `Blocked` whenever a fault is not fully known. An unreconciled
fault therefore can never be reported as an evaluation pass; `Fail` is preserved unchanged.
