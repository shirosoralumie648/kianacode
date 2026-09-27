# SW-14 independent swarm merge review baseline

> Snapshot date: 2026-09-27. This slice adds independent partition review, immutable merge
> receipt and explicit Company acceptance boundary. Local Cargo test/build/check/clippy/smoke
> commands are intentionally not run; GitHub Actions owns fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`SW-14`](../roadmap.md#step-sw-14) |
| source snapshot | `d01268e7` plus this SW-14 source slice |
| feature_status | `partial` for independent review/receipt contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | SW-13 merge digest → one reviewer decision per partition → MergeReceipt → separate Company acceptance |

`SwarmPartitionReview` requires reviewer/author separation, output/evidence digests and a stable
decision. `SwarmMergeReview` enforces canonical one-per-partition coverage and reviewer epoch;
`SwarmMergeReceipt` binds merge/review/policy/acceptor/conflict refs and permanently marks
`company_acceptance_required=true`. The reducer/review facts do not mutate Company acceptance,
release budget or EventLog.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| independence | reviewer=author self-review is rejected |
| coverage | duplicate/missing/out-of-order partition decisions fail |
| evidence | accepted output requires output/evidence digests and merge/review digest binding |
| receipt boundary | receipt requires policy/acceptor/conflict refs and Company acceptance remains separate |
| shape | unknown fields/digest drift fail closed |
| effect boundary | no Broker/EventLog/company acceptance mutation in review facade |

## CI and limitations

GitHub Actions runs `.github/workflows/sw14-merge-review.yml` with domain/Core fixtures, source
guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands
are intentionally not run, and CI results are not awaited.

Limitations: no actual reviewer identity provider, durable MergeDecision/MergeReceipt store,
artifact review UI, budget release or Company project acceptance wiring is added; SW-15+ remains
open.
