# SW-08 single-spine child routing baseline

## Scope

SW-08 binds a child run to the single `DaemonHost → ControlPlane → CapabilityBroker → KianaHarness`
route with parent/child IDs, partition, attempt, correlation and causation facts. Direct runner or
provider routes are rejected, and the append-ready receipt explicitly records that no effect was
dispatched by this source contract.

The Core facade validates route evidence only. It does not create a run, call a provider, invoke a
Broker, start a harness or append an EventLog fact.

## Evidence and limits

- `kiana-domain/tests/swarm_routing.rs` covers spine identity, correlation, direct-route denial,
  wrong broker route, digest drift and unknown fields.
- `kiana-core/tests/swarm_routing_guard.rs` protects the no-second-loop/no-effect boundary.
- `.github/workflows/sw08-routing.yml` runs fixtures, source guard, formatting and affected target
  compilation in GitHub Actions; local Cargo tests/build/check/clippy/smoke commands were not run.

This slice is `feature_status=partial`, `proof_level=source`: it does not prove live DaemonHost
routing, child execution, durable correlation/replay, provider behavior or live/physical outcomes.
