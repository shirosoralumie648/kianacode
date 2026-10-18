# DEP-05 · Operation lifecycle baseline

> Snapshot date: 2026-10-19. This slice defines a bounded deployment operation state machine,
> phase deadlines and replayable operation journal. The 2026-10-19 completion slice enforces
> non-empty evidence, full nested-transition validation, exact persisted-snapshot replay comparison
> and an allocation-free capacity fence. Current local focused verification is recorded below;
> durable deployment integration and remote CI evidence remain open.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-05`](../roadmap.md#step-dep-05) |
| source snapshot | branch `step/dep05-20261018-1` worktree at the 2026-10-19 completion slice (historical anchor `e2b50107`); focused verification on Windows Git Bash |
| feature_status | `implemented` for the pure lifecycle/replay contract (row 610 ✅ at local_behavior); deployment runtime integration remains open |
| proof_level | `local_behavior` for focused lifecycle fixtures; no durable/live/physical promotion |
| canonical path | EventStore facts → operation transition reducer → Core read-only projection → later lease/supervisor/health steps |

`kiana-domain/src/operation_lifecycle.rs` keeps operation state separate from
`DeploymentPhase` and run execution status. Every transition carries the operation and deployment
revision identity, monotonic source cursor, reason, evidence references, observed time and a
stable digest, and `kiana.operation-lifecycle.v1` is registered in the domain schema registry.
Preflight must complete before drain or execution. `Unknown` is a terminal state, and it is
reachable from **two** paths, both of which keep the result uncertain:

- `DrainTimeout` (`Draining → Unknown`): only allowed when the observed time is at or past the same
  active drain deadline; a timeout proves the wait window elapsed, not that the process stopped.
- `ResultUnknown` (`Draining`/`Executing → Unknown`): the upstream fact reports an indeterminate
  result; no deadline is involved and no `Stopped` claim is made.

No transition can produce a `Stopped` state (`OperationState` has no such variant), so neither path
can claim the underlying process was confirmed stopped. Terminal state (including `Unknown`) is
immutable: further transitions fail with `operation_terminal_immutable`.

Every accepted transition must carry a non-empty bounded `evidence_refs` list plus a non-zero
`source_cursor` and a non-empty `reason`; the only exception is the `operation.created` bootstrap
projection written by `OperationJournal::new()`, which never passes through
`OperationTransition::validate()`.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| skip preflight | execute transition from `Created` is rejected (`operation_from_state_mismatch`) |
| late revision | revision id/digest drift is rejected before projection (`operation_revision_late_event`) |
| drain timeout | only a due drain deadline can produce `Unknown`; no `Stopped` state exists |
| duplicate terminal | a second terminal transition is rejected (`operation_terminal_immutable`) |
| cursor regression | source cursor cannot move backwards or repeat (`operation_source_cursor_regressed`) |
| empty evidence (deny-first) | `OperationTransition::new()` and `validate()` reject `evidence_refs=[]` with `operation_evidence_required`; recomputed evidence/transition digests do not bypass it; `append()` propagates the code and leaves the journal unchanged |
| empty projection evidence | a forged journal projection with empty `evidence_refs` fails `operation_evidence_required` |
| forged nested digest (deny-first) | a wire journal whose nested `transition_digest` does not match its fields fails `operation_transition_digest_mismatch` even after `journal_digest` is recomputed |
| forged nested state | a nested `state` that contradicts `kind`/`from_state` fails `operation_transition_state_mismatch` even with matched digests |
| capacity fence | `append()` rejects at `MAX_OPERATION_TRANSITIONS` before any projection mutation; the journal is byte-identical after `Err` |
| append error immutability | preflight/cursor/terminal rejections leave the journal equal to its pre-call snapshot |
| replay | the same ordered transitions rebuild the same terminal journal and digest |
| replay snapshot comparison | Core `replay_operation_journal()` validates the input journal first and requires the rebuilt journal to equal the persisted snapshot; truncated, empty or cursor-drifted sequences fail `operation_journal_replay_mismatch`; no partial journal is returned |
| source boundary | domain/Core contain no filesystem, process, Tokio, Broker, EventStore append or dispatch effect |

## CI and limitations

GitHub Actions is wired through `.github/workflows/dep05-operation-lifecycle.yml` (manual
`workflow_dispatch` lane, `permissions: contents: read`) with domain fixtures, a Core source guard
and affected-target compilation. The local receipt below is current focused proof; remote workflow
results were not awaited.

This slice does not persist the journal, acquire an operation lease, fence a writer, invoke a
supervisor, mutate a deployment root, publish health, consume approval or dispatch a capability.
EventStore/JournalFrame remains the fact source; the reducer is a deterministic projection until
the later DEP-06+ slices provide durable lease, startup, health and shutdown integration.

## Completion-slice focused verification receipt (2026-10-19)

```text
source_snapshot: worktree D:/code/kiana-steps/dep05 on branch step/dep05-20261018-1; kiana-domain/src/operation_lifecycle.rs; kiana-domain/tests/dep05_operation_lifecycle.rs; kiana-core/src/deployment_operation.rs; kiana-core/tests/dep05_operation_lifecycle_guard.rs
worktree_status: DEP-05 completion slice applied in one worktree; no push/merge; D:/code/kianacode untouched
command_argv: `CARGO_TARGET_DIR=D:/code/kiana-steps/target-dep05 CARGO_BUILD_JOBS=4 RUST_MIN_STACK=33554432 cargo test -p kiana-domain --test dep05_operation_lifecycle --offline -- --test-threads=1`; `CARGO_TARGET_DIR=D:/code/kiana-steps/target-dep05 CARGO_BUILD_JOBS=4 RUST_MIN_STACK=33554432 cargo test -p kiana-core --test dep05_operation_lifecycle_guard --offline -- --test-threads=1`; `cargo check -p kiana-domain -p kiana-core --tests --offline`
cwd·environment: D:/code/kiana-steps/dep05; Windows, Git Bash; 4 Cargo build jobs; one test thread; 32 MiB test stack; offline dependency resolution; shared target dir outside the worktree
fixture·cassette: domain lifecycle 10/10; Core guard 4/4; skip preflight, late revision, drain timeout, duplicate terminal, cursor regression, forged state, empty evidence (constructor/validate/append), empty projection evidence, forged nested digest, forged nested state, capacity fence at 256 with no mutation, append-error immutability, deterministic replay and exact persisted-snapshot replay comparison
exit_code: cargo check 0; domain test 0; Core test 0
status_change: DEP-05 row 610 promoted 🔄 → ✅ at local_behavior; the four roadmap gaps (replay snapshot comparison, non-empty evidence, nested validation, capacity fence) are closed in source and fixtures
proof-level_change: focused reducer/facade contract source → local_behavior; no durable/live/physical promotion
limitations: the journal is still an in-memory projection; no EventStore persistence, operation lease/fence, supervisor, startup/health/shutdown admission, cross-process recovery or remote CI run; `journal_digest`/`transition_digest` remain unkeyed content digests, and top-level journal reason/evidence_refs are not yet cross-checked against the final transition
reviewer: integration agent with read-only DEP-05 audit; focused lifecycle/replay reviewer
```

## Historical local focused verification receipt (2026-10-05)

```text
source_snapshot: current dirty worktree after DEP-00 through DEP-04 verification
worktree_status: DEP-05 remains 🔄 because DEP-04 is still open; no automatic commit/push/merge/rebase
command_argv: `CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 RUST_MIN_STACK=33554432 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_STRIP=symbols CARGO_INCREMENTAL=0 cargo test -p kiana-domain --test dep05_operation_lifecycle -p kiana-core --test dep05_operation_lifecycle_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; serial Cargo/test threads; 32 MiB test-thread stack; stripped, non-incremental test profile; combined resource receipt `/tmp/kiana-roadmap-20261005-evidence/dep04-05-client.resources` recorded 2.35s user, 0.41s system, 436268 KiB max RSS, 0 swaps
fixture·cassette: domain lifecycle 4/4; Core purity/replay guard 1/1; skip preflight, late revision, drain timeout, duplicate terminal, cursor regression, forged state and deterministic replay
exit_code: 0
status_change: historical receipt — no roadmap promotion at that time; local lifecycle/replay behavior recorded
proof-level_change: focused reducer contract `source` → `local_behavior`; full card remains partial/open
limitations: journal is not persisted, lease/fence/supervisor/startup/health/shutdown admission are not integrated; DEP-04 protocol admission remains open; no durable/live/physical evidence or successful remote CI run
reviewer: integration agent; focused local lifecycle reviewer
```
