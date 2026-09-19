# PD-11 Cell/Grant/Budget/Lease/Authority Projection Baseline

## Scope

PD-11 adds a pure authority read model over committed facts. It rebuilds bounded Cell lifecycle,
CapabilityGrant state/expiry, budget reservation/settlement counts, resource lease fencing and a
monotonic authority epoch. Duplicate event IDs are idempotently skipped; epoch rollback, active
expired grants, unknown lease release, settlement without reservation and child cells whose parent
is absent fail closed. Projection returns sorted typed rows and source event evidence.

The model never mutates `MemoryCellRegistry`, grants budget, consumes approval, acquires a lease or
dispatches a Broker effect. Durable checkpoint/lease ownership and restart persistence remain
later data-layer steps.

## Evidence and limits

- `kiana-core/tests/pd11_authority_projection.rs` covers rebuild, fencing, budget settlement,
  stale epoch, unknown lease, child-parent and active-expired grant rejection.
- `kiana-core/tests/pd11_authority_projection_guard.rs` protects fact-only/no-authority-grant and
  no-dispatch boundaries.
- `.github/workflows/pd11-authority-projection.yml` runs the fixtures, source guard and workspace
  compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: the authority projection source
and CI fixtures are present, while durable projection storage, process restart and full registry
re-admission remain open.
