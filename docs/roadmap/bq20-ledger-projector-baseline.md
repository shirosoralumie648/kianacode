# BQ-20 EventLog billing ledger projector baseline

> Snapshot date: 2026-09-25. This slice adds a rebuildable read projection over committed BQ-13,
> BQ-14 and BQ-19 facts. Local Cargo test/build/check/clippy/smoke commands are intentionally not
> run; GitHub Actions owns the fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-20`](../roadmap.md#step-bq-20) |
| source snapshot | `15533105` plus this BQ-20 source slice |
| feature_status | `implemented` for bounded source-page, projection-fence, quarantine and rollup contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | committed EventLog page → core source epoch/cursor fence → query projector → read-only snapshot |

The EventLog logical cursor is distinct from each RuntimeEvent request sequence. The source-page
adapter reads complete committed pages from the existing `EventStorePort`; it does not append
projection or quarantine facts. The core fence binds source epoch, source cursor, projection
version and replay-fence digest. The query projector folds only `usage.cost_*`,
`cost.ledger_entry`, `cost.corrected` and `cost.allocation` facts.

Estimated, measured, unknown and correction amounts stay in separate totals. A BQ-19 allocation
is retained as an attribution view and is not added to the canonical BQ-14 ledger total. A malformed
recognized billing fact is retained as a bounded, digest-only quarantine record. Reservation and
approval lifecycle events are ignored by design.

Daily buckets use trusted fact timestamps where the source fact carries one; fixed UTC one-hour
windows are also materialized. Facts without a timestamp remain in the source projection and are
not assigned a fabricated epoch bucket. Rebuilding from the same committed EventLog sequence
produces the same snapshot digest.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| source page | page cursor equals `after_cursor + committed event count`; non-contiguous pages fail closed |
| projection fence | candidate snapshot validates before cursor acknowledgement; epoch/version drift and cursor gaps are rejected |
| replay fence | duplicate source event IDs do not advance the projection |
| quarantine | malformed cost facts remain queryable by cursor/event ID/payload digest; no raw payload is copied |
| state split | usage, ledger, allocation and correction totals are separate; unknown is not numeric zero |
| rollup | daily and one-hour windows are deterministic and rebuildable; allocation labels do not multiply ledger totals |
| read-only boundary | projector has no EventStore append, Broker dispatch, reservation release or approval consumption path |

## CI and limitations

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the domain/core/eventlog/query
fixtures and source guards, then `cargo check --workspace --tests --locked`. Local Cargo tests,
builds, checks, clippy and smoke commands are intentionally not run, and CI results are not awaited.

Limitations: the projection snapshot is a rebuildable read model, not a financial ledger or
authorization source. Persistence of the projection/checkpoint and crash-safe write ordering remain
the caller's `ProjectionStorePort` responsibility; this slice does not prove cross-process durable
checkpointing, invoice reconciliation, provider truth, external effects or live billing. The
projector requires a rebuild when restoring a serialized snapshot before applying a tail because
the private source fact index is not persisted as a second ledger. Timestamp-less usage/allocation
facts are intentionally absent from daily/window buckets rather than assigned a synthetic time.
