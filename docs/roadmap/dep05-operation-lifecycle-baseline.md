# DEP-05 · Operation lifecycle baseline

> Snapshot date: 2026-09-27. This slice defines a bounded deployment operation state machine,
> phase deadlines and replayable operation journal. Current local focused verification is recorded
> below; DEP-04 admission integration and remote CI evidence remain open.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-05`](../roadmap.md#step-dep-05) |
| source snapshot | historical implementation anchor `e2b50107`; focused verification on current dirty worktree (2026-10-05) |
| feature_status | `partial` for the pure lifecycle/replay contract; deployment integration remains open |
| proof_level | `local_behavior` for focused lifecycle fixtures; no durable/live/physical promotion |
| canonical path | EventStore facts → operation transition reducer → Core read-only projection → later lease/supervisor/health steps |

`kiana-domain/src/operation_lifecycle.rs` keeps operation state separate from
`DeploymentPhase` and run execution status. Every transition carries the operation and deployment
revision identity, monotonic source cursor, reason, evidence references, observed time and a
stable digest, and `kiana.operation-lifecycle.v1` is registered in the domain schema registry.
Preflight must complete before drain or execution; a drain deadline timeout folds
to `Unknown` and never to `Stopped`; terminal state is immutable.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| skip preflight | execute transition from `Created` is rejected |
| late revision | revision id/digest drift is rejected before projection |
| drain timeout | only a due drain deadline can produce `Unknown`; no `Stopped` state exists |
| duplicate terminal | a second terminal transition is rejected |
| cursor regression | source cursor cannot move backwards or repeat |
| replay | the same ordered transitions rebuild the same terminal journal and digest |
| source boundary | domain/Core contain no filesystem, process, Tokio, Broker, EventStore append or dispatch effect |

## CI and limitations

GitHub Actions is wired through `.github/workflows/dep05-operation-lifecycle.yml` with domain
fixtures, a Core source guard and affected-target compilation. The local receipt below is current
focused proof; remote workflow results were not awaited.

This slice does not persist the journal, acquire an operation lease, fence a writer, invoke a
supervisor, mutate a deployment root, publish health, consume approval or dispatch a capability.
EventStore/JournalFrame remains the fact source; the reducer is a deterministic projection until
the later DEP-06+ slices provide durable lease, startup, health and shutdown integration.

## Local focused verification receipt (2026-10-05)

```text
source_snapshot: current dirty worktree after DEP-00 through DEP-04 verification
worktree_status: DEP-05 remains 🔄 because DEP-04 is still open; no automatic commit/push/merge/rebase
command_argv: `CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 RUST_MIN_STACK=33554432 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_STRIP=symbols CARGO_INCREMENTAL=0 cargo test -p kiana-domain --test dep05_operation_lifecycle -p kiana-core --test dep05_operation_lifecycle_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; serial Cargo/test threads; 32 MiB test-thread stack; stripped, non-incremental test profile; combined resource receipt `/tmp/kiana-roadmap-20261005-evidence/dep04-05-client.resources` recorded 2.35s user, 0.41s system, 436268 KiB max RSS, 0 swaps
fixture·cassette: domain lifecycle 4/4; Core purity/replay guard 1/1; skip preflight, late revision, drain timeout, duplicate terminal, cursor regression, forged state and deterministic replay
exit_code: 0
status_change: no roadmap promotion; local lifecycle/replay behavior recorded
proof-level_change: focused reducer contract `source` → `local_behavior`; full card remains partial/open
limitations: journal is not persisted, lease/fence/supervisor/startup/health/shutdown admission are not integrated; DEP-04 protocol admission remains open; no durable/live/physical evidence or successful remote CI run
reviewer: integration agent; focused local lifecycle reviewer
```
