# CO-29 bounded rework and successor history baseline

## Scope

CO-29 adds explicit `ReworkProvenance` for predecessor/successor packets. It binds the original
rejection reference/digest, effective baseline, successor packet digest, reason, maximum attempts,
remaining budget and predecessor attempt state. Terminal success and Unknown attempts cannot be
revived; only failed/rejected/stopped predecessors are reclaimable.

`ReworkLedger` preserves every predecessor record and rejects duplicate drift and successor-chain
cycles. Existing Company business Rework already enforces 1–3 attempt limit, rejection at the
current baseline, unchanged path/criteria/dependencies, no active runs and superseded packet
references; this contract makes those boundaries explicit for later command/event integration.

## Implemented source slice

- Typed bounded provenance and attempt-state validation.
- Cycle-safe successor ledger with idempotent same-digest record and historical failure retention.
- Core adapter/source guard indexes existing `CompanyBusinessAction::Rework`, baseline and active
  run fences; no old terminal/Unknown attempt is reused.
- GitHub-only fixtures cover terminal/Unknown denial, baseline drift, bounded chain, cycle and
  preserved predecessor history.

## CI-only evidence

`.github/workflows/co29-rework.yml` runs formatting, the domain rework fixtures, the Core source
guard and domain/core test-target compilation on GitHub Actions. Local Cargo tests, builds, checks,
clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- Provenance ledger is source-only and is not yet the sole durable Company successor/attempt
  projector. Existing business rework remains the compatibility command path.
- Dependency closure, semantic re-review and budget settlement across real attempts remain open;
  Unknown still requires explicit reconciliation and no live/physical outcome is claimed.
