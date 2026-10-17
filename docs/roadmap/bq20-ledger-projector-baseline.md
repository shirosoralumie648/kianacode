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

## 2026-10-17 completion slice (CI-only)

The source slice above was kept; this pass closes the card's deny column (advance cursor before
writing the projection, silently skip bad events, projection writes back facts, stale labelled
fresh) and its success column (a new process rebuilds the same rollup from the EventLog; failed
cursor and quarantine stay queryable) with deny-first fixtures and one shared replay-chain rule.

| Added / hardened | Where | Why |
|---|---|---|
| `billing_replay_fence` + `billing_source_event_id_tail` | `kiana-domain/src/billing_projection.rs` | one deterministic replay chain and one bounded ID-tail rule; the ControlPlane fence and the query projector now call the same functions instead of duplicating the algorithm |
| `BillingProjectionFreshness` + `BillingProjectionSnapshot::require_fresh_at` | domain | a stale snapshot cannot be relabelled fresh (`billing_projection_stale`) and an older live cursor cannot claim the current state (`billing_projection_cursor_ahead`) |
| `BillingProjectionSnapshot::quarantine_for_cursor` / `_for_event` / `_by_payload_digest` | domain | failed cursor and quarantine records are queryable by cursor, event ID and payload digest; a record still carries no raw payload |
| `BillingProjectionFenceError::SourceFenceMismatch` + candidate chain check | `kiana-core/src/billing_projection.rs` | a candidate whose replay fence or bounded ID tail is stale, reordered or forged can never advance the acknowledged cursor, even when its snapshot is internally valid |
| page-level event ID fences | `kiana-eventlog/src/ledger_source.rs` | nil or duplicate event IDs inside one committed page fail closed (`billing_source_event_id_invalid` / `billing_source_event_duplicate`) |
| correction stream sequencing + read-only `quarantine()` | `kiana-query/src/billing_ledger_projector.rs` | corrections on a `cost_ledger` aggregate must strictly increment from the target revision (`correction_stream_regression` / `correction_stream_gap`); callers can read the bounded quarantine without a write path |

Fixture matrix after this slice (deny first): domain 3 → 8, core 3 → 10 (5 fence behaviour + 5
source/wiring guards; the kept `fence_rejects_cursor_gap_and_preserves_previous_cursor` remains its
own test), EventLog 3 → 7, query 3 → 12, query guard 1 → 3. Guards pin markers, refusal-code
matrices, fixture names, the absence of update/delete/second-authority paths and the
`scripts/ci/test-shards.json` bindings (`kiana-domain-s1/4` lists `bq20_ledger_projection`;
`kiana-core-s1/6` lists `bq20_ledger_projector_guard`; `kiana-eventlog` and `kiana-query` are
whole-crate shards).

Manual lane: `.github/workflows/bq20-ledger-projector.yml` (`workflow_dispatch`-only,
`permissions: contents: read`, `cargo fetch --locked`, `cargo fmt --all --check`, five test targets
with `--test-threads=1` and five `--no-run` compile steps).

Known limits after the completion slice: local tests/build/check/clippy/smoke were not run and CI
receipts are not awaited, so the proof ceiling stays `source`. The projection is still an in-memory
rebuildable read model: persistence of the projection/checkpoint, cross-process crash-safe ordering,
invoice reconciliation, budget allocation and live/physical billing evidence remain
BQ-21/BQ-22/BQ-23 and later.
