# CO-43 bounded Company parallel baseline

## Scope

CO-43 adds a Company-facing wrapper around the existing SwarmWorkGraph/ControlPlane contracts.
The plan binds one project/parent packet, merge owner, disjoint partition identities, input version,
authority epoch, isolation digest, output contract, concurrency and expiry. Settlement covers every
partition and makes ResultUnknown/partial children non-mergeable.

## Implemented source slice

- `CompanyParallelPlan` rejects duplicate partitions, missing parent/owner, invalid isolation,
  stale epoch and unbounded identity; it does not create a message bus or executor.
- `CompanyParallelSettlement` requires complete partition coverage and evidence; only all-success
  children are mergeable, while Pending/Failed/Cancelled/ResultUnknown remain non-mergeable.
- Existing `SwarmPlan`, `SwarmWorkGraph`, `WorkFingerprint`, CellRegistry and ControlPlane remain
  the admission/dispatch/retirement authorities.

## CI-only evidence

`.github/workflows/co43-company-parallel.yml` runs formatting, bounded parallel settlement
fixtures, the Core Swarm/Cell boundary guard and target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- No real concurrent worker, isolated workspace, power-loss or cross-process Cell proof is claimed;
  this is a typed source boundary over the existing process-local Swarm path.
- Integrator conflict/MergeReceipt, durable fan-out and live/physical effects remain CO-44+ work.
