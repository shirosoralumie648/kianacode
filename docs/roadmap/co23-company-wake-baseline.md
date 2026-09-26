# CO-23 durable Company wake and intent-consumption baseline

## Scope

CO-23 adds a durable wake projection for committed Company process intents. `CompanyWakeLedger`
keys wakes by intent ID, retains the source cursor, coalesces duplicate notifications, and
rebuilds from committed intent facts after restart. A wake claim is a coordination record only;
authority is rechecked by the existing ControlPlane before dispatch.

Consumption requires a typed `CompanyDispatchReceipt`. A consumed wake is idempotent for the same
receipt. A crash or uncertain dispatch becomes `Unknown` and cannot be retried by relabeling it
successful. Pending wakes are ordered by due time and intent ID, and cursor advancement remains
separate from best-effort delivery hints.

## Implemented source slice

- `CompanyWake` binds intent/process/kind/trigger digest/source cursor and claim/dispatch state.
- `CompanyWakeLedger` supports enqueue/coalesce, committed-intent scan, claim, consume,
  Unknown/reconciliation and deterministic pending projection.
- `CompanyDispatchReceipt` rejects consumed-with-unknown-effect and binds receipt to the exact
  intent/dispatch/source cursor.
- Daemon `CompanyDispatchAdapter` only rebuilds/consumes typed wake facts; it never invokes a
  model, capability or provider. Existing core automation/workflow planner remains the execution
  authority.

## CI-only evidence

`.github/workflows/co23-company-wake.yml` runs formatting, the domain wake fixtures, the Core
source guard and domain/core/daemon/workflow test-target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- The new ledger is a typed source contract; full durable EventLog cursor/checkpoint storage and
  cross-stream Company/workflow atomicity remain open. A wake never grants execution authority.
- Daemon restart scanning, external dispatch receipts, budget/model accounting and live/physical
  effects remain unproven; Unknown requires explicit reconciliation.
