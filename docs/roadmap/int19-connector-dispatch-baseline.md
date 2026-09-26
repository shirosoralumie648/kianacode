# INT-19 Broker dispatch / adapter observation baseline

## Scope

INT-19 separates connector dispatch from effect observation. A connector attempt now carries a
strict, digest-bound lifecycle: `prepared → dispatching → observed → result_committed`, with
`unknown` as a terminal quarantine state. The domain transition reducer rejects skipped phases,
incomplete evidence, terminal resurrection and forged lifecycle digests.

## Implemented source slice

- `ConnectorDispatchLifecycle` is a pure domain contract with immutable invocation/attempt,
  command, binding, payload and idempotency digests. `Prepared` and `Dispatching` cannot carry
  receipt/result evidence; `Observed` requires receipt and observation digests; terminal states
  cannot transition again.
- `ConnectorEventJournal` is the only connector-local object holding `EventStorePort`. The
  capability handler reads and appends through that boundary instead of owning the EventStore
  dependency directly.
- The local fixture path uses distinct `LocalFixtureAdapter` and
  `LocalFixtureEffectObserver` objects. The journal commits `prepared` and `dispatching` before
  adapter entry, then commits `observed` and the terminal lifecycle before `connector.invoked`.
  A replay that finds an incomplete or terminal lifecycle returns `result_unknown` and does not
  invoke the adapter again.
- `connector_bindings` validates lifecycle facts and binds the final receipt/observation digests;
  no second execution loop or authorization source is introduced.

## CI-only evidence

GitHub Actions runs the domain lifecycle fixtures, daemon source guard, formatting and workspace
test-target compilation. Local Cargo test/build/check/clippy/smoke commands are intentionally not
run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: partial
proof_level: source
```

The current adapter is still a deterministic project-local fixture and does not prove an external
provider effect, durable cross-process EventLog recovery, provider receipt truth, or physical/live
outcome. The journal boundary is local to the daemon composition; a future live connector must
provide the same dispatch/observation ports and durable recovery evidence. Reconciliation and
cancel settlement remain INT-22/INT-23 work.
