# INT-23 connector cancellation baseline

## Scope

INT-23 makes connector cancellation reconstructible from server-owned stop evidence. A confirmed
stop before dispatch settles as `not_executed` and may release the lease; a started effect keeps its
lease held for reconciliation; an unconfirmed stop is `unknown`. Every terminal settlement fences
late provider results.

## Implemented source slice

- `ConnectorCancellationSettlement::from_stop_report` binds invocation, attempt, command, permit,
  lease and stop report digests. It rejects cancellation after an observation already exists.
- `NotExecuted`, `StopConfirmed` and `Unknown` have explicit lease settlement rules. Started or
  unconfirmed work cannot be marked released, and `late_result_fenced` is required for every
  terminal settlement.
- `ConnectorCancelRequest`/`cancel_checked` and the existing `StopReport` contract remain the
  adapter boundary; the domain settlement is pure and does not send signals, release leases or
  append facts itself.

## CI-only evidence

GitHub Actions runs the cancellation settlement fixtures, source guard, formatting and workspace
test-target compilation. Local Cargo test/build/check/clippy/smoke commands are intentionally not
run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: partial
proof_level: source
```

This slice does not wire a live connector cancel command, process supervisor, durable lease store,
reconciliation inbox delivery or provider outcome. It supplies the typed settlement/fence contract
for the later daemon and recovery integrations.
