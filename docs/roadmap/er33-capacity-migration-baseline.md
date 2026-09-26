# ER-33 Event/Receipt/Recovery capacity and migration drill baseline

## Scope

ER-33 adds a bounded drill report for event frame, flush, projection rebuild, receipt query,
artifact bytes, queue depth and recovery metrics. It also binds a known schema migration check,
read-only downgrade semantics and fact preservation. The report requires over-quota rejection,
partial-frame non-append, unknown-version rejection and no fact deletion for capacity.

This is a source/CI contract over existing persistence-capacity and migration-preflight boundaries;
it does not benchmark the host, rotate/archive journals, run a migration, delete facts or claim
durable performance.

## CI-only evidence

`.github/workflows/er33-capacity-migration.yml` runs formatting, domain drill fixtures, the Core
storage boundary guard and affected test-target compilation on GitHub Actions. Local Cargo tests,
builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| `bounded_metrics_and_migration_preserve_facts_and_reject_over_quota` | all seven metric classes are bounded; quota rejection and read-only migration preserve facts |
| `partial_frame_unknown_version_and_fact_deletion_cannot_be_success` | partial append, unknown schema and deletion-for-capacity are denied |
| `duplicate_metric_or_unbounded_sample_fails_closed` | duplicate metric kinds and over-limit bytes fail closed |

## Limitations and handoff

- No host benchmark, journal rotation/archive, fsync, migration runner, rollback, disk-full or
  projector rebuild was executed; performance numbers are fixture bounds only.
- Durable/local proof and capacity measurements remain ER-34/PD/DEP work.
- CI results are intentionally not awaited; proof level remains `source`.
