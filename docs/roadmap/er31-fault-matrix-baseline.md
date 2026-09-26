# ER-31 Event/Receipt/Recovery crash-point matrix baseline

## Scope

ER-31 adds a replay-only twelve-point Event/Receipt/Recovery matrix covering prepare before/after,
permit consume, spawn, partial write, patch rename, stop/reap, result commit, result delivery,
projection checkpoint, artifact publish and cleanup. Each case binds a seed, source cursor/event
refs, safety disposition, effect confirmation, Unknown queryability, resource fencing and Receipt
limitations. The matrix rejects duplicate source refs, duplicate effects, false success, unfenced
Unknown and missing limitation evidence.

It extends the existing H25 eight-point `FaultMatrix` without changing that v1 contract. The new
domain/core slice remains a pure validator and never kills a process, invokes a handler/provider or
writes EventLog/Receipt facts.

## CI-only evidence

`.github/workflows/er31-fault-matrix.yml` runs formatting, the domain matrix fixtures, the Core
Event/Receipt boundary guard and affected test-target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| `every_event_receipt_recovery_boundary_is_unknown_or_rejected_and_replayable` | all twelve points are deterministic, source-bound and never false success/duplicate effect |
| `forged_success_duplicate_effect_and_missing_unknown_limitation_are_rejected` | safety flags, missing fence and missing Receipt limitation fail closed |
| `duplicate_source_refs_and_matrix_drift_fail_closed` | duplicate source refs and incomplete matrix cannot validate |

## Limitations and handoff

- The matrix classifies supplied evidence and does not execute physical crash/timeout/disk-full,
  process reaping, patch rename/fsync, provider calls or durable restart.
- New-process recovery, JSONL durability, adapter conformance, performance/capacity and physical
  evidence remain ER-32+ / PD / DEP work.
- CI results are intentionally not awaited; proof level remains `source`.
