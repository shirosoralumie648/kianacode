# CO-44 Company Integrator and MergeReceipt baseline

## Scope

CO-44 adds a fixed-base integration contract over the existing Swarm merge path. An integration
plan binds the project/base revision, every child output digest, output contract, Integrator and
approval. Conflicts are named and require reviewer/evidence decisions. A MergeReceipt proves only
revalidated combination; it never proves Project acceptance, push or publication.

## Implemented source slice

- `CompanyIntegrationPlan` rejects missing/changed child outputs, duplicate conflicts and missing
  Integrator/approval bindings.
- `CompanyConflictDecision` and `CompanyMergeReceipt` require complete conflict coverage,
  revalidation, result revision advancement and preserved child/evidence digests.
- `pushed_or_published` is explicitly false in a valid receipt; existing Swarm MergeReceipt,
  review and ControlPlane paths remain execution/authority boundaries.

## CI-only evidence

`.github/workflows/co44-company-integration.yml` runs formatting, stale-base/hidden-conflict/
revalidation fixtures, the Core Swarm/receipt boundary guard and target compilation on GitHub
Actions. Local Cargo tests, builds, checks, clippy and smoke scripts were not run and CI is not
awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- No filesystem/Git merge, push, publication or real child worker is executed by this contract.
- Project/Milestone/Packet acceptance remains separate; durable Integrator conflict workflow and
  live/physical output remain later steps.
