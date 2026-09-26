# CO-20 Cell admission and resource lifecycle baseline

## Scope

CO-20 adds a replayable resource-set contract for Cell admission. One `CellAdmission` binds a
SpawnPlan, Cell, BudgetLease, CapabilityGrant, SupervisionLease and canonical owned paths to one
root run and owner cell. `CellAdmissionLedger` rejects identity/path/resource overlap before a
reservation can be recorded.

The lifecycle is explicit: `Reserved` → `Committed` or `RolledBack`, then `Retired`. Rollback and
retire release only the admission's own resource set and are idempotent. Active capabilities fence
retirement, and an uncertain commit becomes `Unknown` with resources retained for reconciliation;
it is never silently treated as released. The existing Core `MemoryCellRegistry` reserve/commit/
abort/retire path remains the concrete local adapter and is source-guarded alongside this contract.

## Implemented source slice

- `CellAdmissionResources` validates typed identities, canonical paths and duplicates.
- `CellAdmission` records status, commit/release/unknown timestamps, active capability count,
  release reason and digest; invalid partial transitions fail closed.
- `CellAdmissionLedger` provides idempotent reserve/commit/rollback/retire/unknown operations and
  rejects resource/path conflicts while preserving each admission's own release boundary.
- GitHub-only fixtures cover partial reserve conflict, rollback then reuse, active-capability
  retirement rejection, own-resource release, unknown reconciliation and serialization.

## CI-only evidence

`.github/workflows/co20-cell-admission.yml` runs formatting, the domain admission fixtures, the
Core source guard and domain/core/daemon test-target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- The ledger is a domain decision contract; durable EventLog/CellRegistry persistence and
  cross-process recovery remain open. Existing process-local MemoryCellRegistry remains the local
  effect adapter.
- Real budget settlement, OS process stop, provider/external effect receipts and live/physical
  outcomes remain unproven. Unknown admissions require explicit reconciliation before release.
