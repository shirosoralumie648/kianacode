# BQ-28 retention and archive bounds baseline

> Snapshot date: 2026-09-28. Local Cargo **tests were not executed**; GitHub Actions owns fixtures
> and the workspace gate.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-28`](#step-bq-28) |
| code landing | `kiana-core/src/retention_archive_bounds.rs`, registered by `kiana-core/src/lib.rs` |
| fixtures | `kiana-core/tests/bq28_retention_archive_bounds.rs`, `kiana-core/tests/bq28_retention_archive_bounds_guard.rs` |
| feature_status | `partial` — the bounds are decidable in source; nothing is pruned or archived here |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |

## The four failures, and the invariant that outranks them

| Card failure | Refusal |
|---|---|
| rollup 无界内存 | `rollup_unbounded_memory` — both the bucket count and the byte count have ceilings, and blowing either refuses |
| 高基数标签 | `rollup_high_cardinality_label` — compared against **BQ-25's** `metric_label_budget()` |
| 删除 retained evidence | `retention_retained_evidence_deleted` |
| archive 后仍可结算旧 lease | `archive_lease_still_settleable` — and only for leases this pass actually archived, so the rule is about archives rather than about ids |
| **保留/归档不改账本事实** | `retention_ledger_fact_changed` |

The last one is the invariant the card hangs on. A pass that prunes storage and, in the same
breath, moves a number has stopped being a retention pass. The caller states the ledger fact digest
before and after, and this module refuses the two disagreeing rather than trusting that nothing
moved.

## The cardinality ceiling is not a new one

A rollup key is a metric label wearing a different hat, so it lives inside the rule BQ-25 already
established. The module imports `metric_label_budget()` rather than declaring a constant, and the
guard asserts both that the import is there and that no `MAX_ROLLUP_LABEL_VALUES` or
`BQ28_CARDINALITY` was invented — two ceilings for one question is how they drift.

## Percentiles come from a fixed fixture

`percentile_ms` sorts what it is given and takes the nearest rank: `ceil(p/100 * n)`, clamped. A p95
of a ten-element fixture is therefore a real observation rather than a number interpolated between
two of them, and the same fixture yields the same number on every run and in every CI job. A p95
derived from whatever the process happened to be doing is not a bound, it is a mood — which is why
the card asks for a fixed fixture.

## The check order

Bounds first, in the order an operator would hit them: rollup memory, then cardinality, then disk,
then queue, then p95. The evidence and lease rules follow. The ledger invariant is checked **last**,
because a pass that moved a ledger fact *and* blew a bucket bound has two problems, and the one an
operator can act on immediately is the bound. The ledger check still runs, and it is the one that
cannot be waived.

## Honest limitations

This module computes. It prunes nothing, archives nothing, reads no clock and opens no file — the
guard asserts the absence of each of those tokens, including `remove_file` and `remove_dir`. Every
count, byte total and latency observation is supplied by the caller, and so is the pair of ledger
digests: the module compares two strings it was handed and does not take a diff itself, which the
report's own always-non-empty `limitations` says out loud. No retention window was actually swept,
no evidence was deleted, no lease was archived, no percentile was measured against real traffic, and
nothing is wired into `ControlPlane::handle_command` or into the `RetentionStorePort` adapter, which
remains the default unsupported port.
