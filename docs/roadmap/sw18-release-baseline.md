# SW-18 swarm release gate baseline

> Snapshot date: 2026-09-27. This slice adds deny-first swarm release evidence covering fake
> golden, Unknown, replay, crash and race scenarios. Local Cargo test/build/check/clippy/smoke
> commands are intentionally not run; GitHub Actions owns fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`SW-18`](../roadmap.md#step-sw-18) |
| source snapshot | `49594d1a` plus this SW-18 source slice |
| feature_status | `partial` for deny/replay/Unknown/release evidence gate |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | swarm source facts/projection → deny-first case matrix → release evidence gate |

`SwarmReleaseGate` requires six unique scenarios: Deny, Unknown, Replay, Crash, Race and
FakeGolden. Deny/Race verified cases must have zero handler/effect calls; Unknown preserves
uncertainty; Replay requires replay fencing; every case records same-spine/secret-free/evidence and
limitations. The gate is source evidence and never publishes a release or runs a swarm.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| deny/race | effect_count and handler_calls are zero |
| Unknown | unknown outcome remains visible and bounded |
| replay | replay fence and effect bound are mandatory |
| fake golden | fake evidence stays separate from live proof |
| matrix | all six scenarios, unique and digest-bound, are required |
| boundary | no Broker/EventLog/launch/live publication path |

## CI and limitations

GitHub Actions runs `.github/workflows/sw18-release-gate.yml` with domain/Core fixtures, source
guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands
are intentionally not run, and CI results are not awaited.

Limitations: this gate cannot run crash/race/property workloads, publish a release, prove durable
replay or live/physical swarm behavior; SW phase handoff remains source-only.
