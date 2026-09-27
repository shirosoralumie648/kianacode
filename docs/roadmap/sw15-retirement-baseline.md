# SW-15 swarm retirement baseline

> Snapshot date: 2026-09-27. This slice adds exactly-once release/retire and residual budget/path
> evidence contracts. Local Cargo test/build/check/clippy/smoke commands are intentionally not run;
> GitHub Actions owns fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`SW-15`](../roadmap.md#step-sw-15) |
| source snapshot | `9470bdbb` plus this SW-15 source slice |
| feature_status | `partial` for release/retire evidence contract |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | child terminal facts → residual budget/path release evidence → exactly-once retirement fact with facts retained |

`SwarmRetirementFact` keeps residual budget and path-lock release digests, source cursor, child
terminal/Unknown counts, release receipt and attempt number. Released/Retired require all children
known terminal and exactly one release attempt; Unknown blocks release. Facts are retained and the
contract never deletes history, releases a budget, retires a Cell or appends EventLog.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| residual evidence | budget/path/release digests are mandatory |
| Unknown | blocked Unknown state cannot be released/retired |
| exactly-once | release_attempt > 1 fails closed |
| retention | facts_retained is mandatory |
| shape | digest/unknown-field/child-count drift fails |
| boundary | no Broker/EventLog/delete/release effect in source/Core facade |

## CI and limitations

GitHub Actions runs `.github/workflows/sw15-retirement.yml` with domain/Core fixtures, source guard
and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands are
intentionally not run, and CI results are not awaited.

Limitations: no durable budget/path release, Cell retirement, EventLog append or cross-process
exactly-once proof is added; SW-16+ remains open.
