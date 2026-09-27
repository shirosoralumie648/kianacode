# SW-17 swarm projection baseline

> Snapshot date: 2026-09-27. This slice adds a parent/child swarm display projection reducer with
> epoch/sequence/source cursor and terminal replay fences. Local Cargo test/build/check/clippy/
> smoke commands are intentionally not run; GitHub Actions owns fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`SW-17`](../roadmap.md#step-sw-17) |
| source snapshot | `48506456` plus this SW-17 source slice |
| feature_status | `partial` for source projection/replay/hydration fences |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | committed child/progress/terminal event → epoch/sequence reducer → read-only snapshot state |

`SwarmProjectionState` accepts ordered events for one epoch, records source cursor/event digests,
and treats exact source replay as idempotent. Gaps, epoch drift, conflicting replays and terminal
resurrection fail closed. Hydration/display state is separate from dispatch and cannot trigger a
new child or effect.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| ordered events | progress then terminal advances source/sequence deterministically |
| replay | exact event replay is a no-op and cannot duplicate effect |
| gaps/drift | sequence gap and epoch mismatch require snapshot hydration |
| terminal fence | late nonterminal event after terminal is rejected |
| shape | event/state digest and unknown fields fail closed |
| effect boundary | projection has no dispatch/Broker/EventLog path |

## CI and limitations

GitHub Actions runs `.github/workflows/sw17-projection.yml` with domain/Core fixtures, source guard
and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands are
intentionally not run, and CI results are not awaited.

Limitations: no UI/CLI/Web/Workbench/Desktop adapter, durable snapshot store or terminal must-deliver
transport is wired here; SW-18 remains open.
