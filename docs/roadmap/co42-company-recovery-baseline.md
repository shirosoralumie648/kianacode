# CO-42 Company durable-fact recovery and schema baseline

## Scope

CO-42 adds a fail-closed durable-fact hydration contract for Company restart. Facts carry explicit
schema version, project, sequence, source cursor, outcome and payload/evidence digests. Hydration
rebuilds a read-only snapshot, always defaults the project to paused, and marks Unknown/corrupt
evidence for reconciliation. Resume is a separately approved plan bound to the snapshot digest,
data epoch and authority epoch.

## Implemented source slice

- `CompanyRecoverySnapshot::hydrate` rejects sequence gaps, cursor regressions, foreign projects,
  unknown schema versions, corrupt facts and missing evidence; it preserves pending dispatch/
  decision sequences and all fact digests.
- `CompanyRecoveryPlan` supports ReadOnly/Reconcile/Resume, keeps automatic retry disabled and
  requires explicit approval plus a clean current snapshot for Resume.
- `CompanyRecoveryLedger` records snapshots/plans idempotently; it never reexecutes an intent,
  dispatch, provider effect or model call.
- CompanyState/Core expose the typed adapter while existing CompanyReplayReducer, projection
  recovery and EventLog remain the durable authorities.

## CI-only evidence

`.github/workflows/co42-company-recovery.yml` runs formatting, gap/unknown/schema/rebuild/resume
fixtures, the Core no-blind-replay guard and target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- This contract does not perform a real cross-process restart or read a disk EventStore; no durable
  or physical recovery proof is claimed.
- Actual provider/effect reconciliation, HumanTask hydration and schema migration orchestration
  remain later recovery steps.
