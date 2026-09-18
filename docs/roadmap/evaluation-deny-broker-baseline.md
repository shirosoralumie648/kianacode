# EQ-11 Evaluation Deny Broker Baseline

## Scope

EQ-11 adds `kiana-daemon::eval_runtime::DenyByDefaultEvalBroker`, an evaluation-only
implementation of the existing `CapabilityBrokerPort`. It accepts an already authorized
request-shaped value solely to return a structured, stable `permission_denied` result; it never
registers, resolves, or invokes an executor. Network, secret, MCP, payment, publish and desktop
operations receive explicit effect-category diagnostics, while unclassified operations remain
denied by default.

The adapter has no network client, secret store, process launcher, desktop API, scheduler, runner,
or background task. Cancellation is also a deny result because this adapter has no in-flight
effect whose outcome could become Unknown. The production Broker/ControlPlane remains the only
place where a real capability could be admitted in later slices.

## Evidence and limits

- `kiana-daemon/tests/eq11_deny_broker.rs` covers all forbidden effect categories, stable error
  strings, unknown-effect denial, cancellation and zero executor dispatch.
- `kiana-core/tests/eq11_deny_broker_guard.rs` protects the no-real-executor/no-network boundary.
  GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it is not yet composed into a
DaemonHost target, fixture store, EventLog/Receipt capture or fault/restart plan, and it does not
prove any real/live/physical external effect. Those remain EQ-12+.
