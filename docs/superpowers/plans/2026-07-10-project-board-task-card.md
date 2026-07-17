# Project Board and Task Card P0 Implementation Plan

> **Execution rule:** This project does not use TDD. The checked steps below are a completed evidence inventory, not a required execution order; future maintenance implements the approved contract first, then runs focused, integration, and workspace verification.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a reusable Project Board projection over existing Kiana Task JSON and expose explainable `/project board` and `/project next` commands.

**Architecture:** Keep TaskCreate/TaskUpdate JSON as the only source of truth. Add a pure `kiana-tasks::project_board` domain module that maps raw task values into validated Task Cards, fixed Kanban columns, policy findings, and deterministic next-task results. Add a thin `ProjectCommand` adapter that reuses the existing task loader from `kiana-commands::tasks`.

**Tech Stack:** Rust, serde/serde_json, existing `kiana-tasks`, `kiana-commands`, async command registry, Cargo tests.

---

## Concrete API Contract

```rust
pub fn build_project_board(
    task_list_id: impl Into<String>,
    tasks: &[serde_json::Value],
) -> ProjectBoardResult<ProjectBoardProjection>;

pub fn select_next_project_task(
    board: &ProjectBoardProjection,
) -> ProjectNextTaskReport;
```

The tests locate Task Cards through `ProjectBoardProjection.columns`, where each column has a `status: ProjectBoardStatus` and `tasks: Vec<ProjectTaskCard>`.

### Task 1: Domain implementation and verification for Board projection

**Files:**
- Create: `kiana-tasks/tests/project_board.rs`
- Modify: `kiana-tasks/src/lib.rs`

- [x] **Step 1: Add verification for Ready, Blocked, Done, and Evidence policy**

Create task fixtures as `serde_json::json!` values and assert:

- pending + verification becomes `ready`;
- pending + unresolved `blockedBy` becomes `blocked`;
- completed without evidence becomes `blocked` with `completed_without_evidence`;
- completed with evidence becomes `done`.

Use this assertion shape:

```rust
let board = build_project_board("default", &tasks).unwrap();
let ready = card(&board, "ready-task");
assert_eq!(ready.board_status, ProjectBoardStatus::Ready);
```

- [x] **Step 2: Run domain verification**

Run: `cargo test -p kiana-tasks --test project_board --locked`

Expected: the domain verification passes after implementation.

- [x] **Step 3: Add deterministic next-task verification**

Use three Ready tasks with different `priority`, `blocks`, and `updated_at`; assert selection order is priority, downstream fanout, age, then id.

```rust
let report = select_next_project_task(&board);
assert_eq!(report.selected_task.unwrap().task_id, "high-fanout");
assert!(report.why.iter().any(|reason| reason == "unblocks=2"));
```

- [x] **Step 4: Add no-ready verification**

Assert `selected_task` is null and blocked/backlog counts are present.

```rust
assert!(report.selected_task.is_none());
assert_eq!(report.blocked_summary["blocked"], 1);
assert_eq!(report.blocked_summary["backlog"], 1);
```

### Task 2: Domain implementation

**Files:**
- Create: `kiana-tasks/src/project_board.rs`
- Modify: `kiana-tasks/src/lib.rs`

- [x] **Step 1: Define public schemas and errors**

Implement:

- `ProjectBoardStatus`.
- `ProjectTaskCard`.
- `ProjectBoardColumn`.
- `ProjectPolicyFinding`.
- `ProjectBoardProjection`.
- `ProjectNextAlternative`.
- `ProjectNextTaskReport`.
- `ProjectBoardError` and `ProjectBoardResult<T>`.

All serialized schema values must match the design document exactly.

- [x] **Step 2: Parse compatible Task JSON**

Implement aliases for task id, title, blockedBy, and metadata fields. Reject missing ids, missing titles, unknown source statuses, duplicate ids, and non-array verification/evidence fields.

- [x] **Step 3: Implement safety-first status projection**

Apply Evidence, dependency, blocker, failure, archive, active, review, ready, and backlog rules in the exact order specified by the design.

- [x] **Step 4: Implement fixed Board columns and summaries**

Return all eight columns even when empty. Populate counts, policy findings, ready ids, blocked ids, and done ids deterministically.

- [x] **Step 5: Implement deterministic next-task query**

Select only Ready tasks using priority, blocks count, updated_at, and task id ordering. Include structured reasons and alternatives.

- [x] **Step 6: Run domain tests after implementation**

Run: `cargo test -p kiana-tasks --test project_board --locked`

Expected: all Project Board tests pass.

### Task 3: CLI verification for `/project`

**Files:**
- Create: `kiana-commands/src/project.rs`
- Modify: `kiana-commands/src/lib.rs`
- Modify: `kiana-commands/src/registry.rs`
- Modify: `kiana-commands/src/tasks.rs`

- [x] **Step 1: Add registry discovery verification**

Extend `default_registry_includes_core_commands` to require `project`.

- [x] **Step 2: Add `/project board --json` verification**

Create on-disk Task JSON fixtures under `.kiana/tasks/default`, execute `ProjectCommand`, and assert schema, fixed columns, Ready status, Blocked status, and Evidence policy.

- [x] **Step 3: Add `/project next --json` verification**

Assert the highest ranked Ready task is selected and the response contains reasons and alternatives.

- [x] **Step 4: Run CLI verification**

Run: `cargo test -p kiana-commands project --locked`

Expected: the CLI verification passes after implementation.

### Task 4: CLI implementation

**Files:**
- Create: `kiana-commands/src/project.rs`
- Modify: `kiana-commands/src/lib.rs`
- Modify: `kiana-commands/src/registry.rs`
- Modify: `kiana-commands/src/tasks.rs`

- [x] **Step 1: Expose the existing task loader inside the crate**

Change only the minimum helpers to `pub(crate)`:

- `load_tasks`.
- `task_list_id`.

Do not duplicate task-root, cwd, disk merge, or sorting logic.

- [x] **Step 2: Implement `ProjectCommand` routing**

Support:

- `board [--json] [task_list_id]`.
- `next [--json] [task_list_id]`.
- `help`.

Reject unknown options and extra positional arguments with the command usage text.

- [x] **Step 3: Implement JSON output**

Serialize domain values without renaming or reshaping:

- `kiana.project-board.v1`.
- `kiana.project-next.v1`.

- [x] **Step 4: Implement concise text output**

Board text must include list id, counts, top Ready/Blocked tasks, and finding count. Next text must include selected task, reasons, and explicit no-ready behavior.

- [x] **Step 5: Register and export the command**

Add `pub mod project` and register `ProjectCommand` in `create_default_command_registry`.

- [x] **Step 6: Run CLI tests after implementation**

Run: `cargo test -p kiana-commands project --locked`

Expected: registry and command tests pass.

### Task 5: Usage and regression verification

**Files:**
- Modify: `USAGE.md`
- Modify: `docs/superpowers/plans/2026-07-10-project-board-task-card.md`

- [x] **Step 1: Document the two commands**

Add JSON and text examples for `kiana project board` and `kiana project next`. State that Board is a projection over TaskCreate files and does not execute tasks.

- [x] **Step 2: Run focused verification**

Run:

- `cargo test -p kiana-tasks --test project_board --locked`
- `cargo test -p kiana-commands project --locked`
- `cargo fmt --all --check`
- `git diff --check`

- [x] **Step 3: Run the full offline workspace regression**

Run: `cargo test --workspace --locked --offline --no-fail-fast`

Expected: all unit, integration, and doc tests pass. Existing non-fatal warnings must be recorded but must not be reported as new failures.

- [x] **Step 4: Refresh the commercial blocker baseline**

Run: `bash scripts/commercial-release-blockers-report.sh --json`

Expected: Project Board introduces no new local release blocker. The dirty tracked tree remains the only local blocker until reviewed changes are committed.

## Execution Note

The current checkout is an intentionally dirty `master` worktree containing the active WorkflowRun and commercial-readiness slices. Do not reset, clean, or discard those changes. This plan omits commit steps for the current session so unrelated user changes are not accidentally bundled; integration can be performed after a deliberate review boundary.

## Completion Evidence

Completed on 2026-07-10 against the current dirty `master` worktree without resetting or discarding unrelated changes.

- Domain verification: `CARGO_INCREMENTAL=0 cargo test -p kiana-tasks --test project_board --locked` passed `8/8` tests.
- Command verification: `CARGO_INCREMENTAL=0 cargo test -p kiana-commands project --locked` passed `24/24` selected tests with `226` unrelated command tests filtered out.
- Formatting and patch hygiene: `cargo fmt --all --check` and `git diff --check` both exited `0`.
- Real CLI smoke: a clean temporary directory returned `kiana.project-board.v1` from `kiana project board --json` with all eight fixed columns, and returned `kiana.project-next.v1` from `kiana project next --json` with `selected_task: null`, `why: ["no_ready_tasks"]`, and an empty `primary_blockers` list.
- Full regression: `CARGO_INCREMENTAL=0 cargo test --workspace --locked --offline --no-fail-fast` exited `0`; the only observed diagnostic was the pre-existing non-fatal `unused_mut` warning in `kiana-tools/src/agent.rs`.
- Independent review: the follow-up review reported no remaining Critical or Important findings. The prior four Important findings were closed by `primary_blockers`, complete tie-break explanations, strict optional-field validation, and stable policy-finding ordering.
- Commercial blocker baseline: `kiana.commercial-release-blockers.v1` remains `blocked` with `13` blocking checks, `12` external blockers, and `1` local blocker. The only local blocker is `source.clean-tracked-tree`, caused by the intentionally dirty tracked worktree.
- Build reliability note: an earlier concurrent incremental build on the FUSE-backed workspace produced a transient `ENOSPC`; rerunning without concurrent reviewer compilation and with `CARGO_INCREMENTAL=0` completed successfully.
