# Workflow Resume and Reconciliation Implementation Plan

> **For agentic workers:** This project does not use TDD. Implement the approved API first, then run the focused tests and workspace verification listed below.

**Goal:** Make persisted WorkflowRun state resumable without silently trusting stale or inconsistent snapshots.

**Architecture:** Add reusable load/list/resume functions to `kiana-tasks`. The domain layer selects a requested or latest run, parses `state.json`, validates append-only `eventlog.jsonl` sequence continuity, and returns a typed ready/blocked report. Expose the same behavior through `kiana tasks workflow list` and `kiana tasks workflow continue` so a future `project continue` entry can delegate to one implementation.

**Scope:** This slice does not execute the next workflow node, mutate Git state, refresh context indexes, or implement the full project registry. It only provides deterministic run discovery and safe resume projection.

### Task 1: Domain implementation

- Add `WorkflowRunSummary`, `WorkflowResumeStatus`, and `WorkflowResumeReport`.
- Add `list_workflow_runs(root)` and `resume_workflow_run(root, run_id)`.
- Validate run identifiers, state run identity, JSONL parsing, contiguous sequence numbers, and state/EventLog sequence agreement.
- Keep malformed or missing state visible as an error; never silently skip it.

### Task 2: Domain post-implementation verification

- Extend `kiana-tasks/tests/workflow_runtime.rs` after the domain APIs exist.
- Verify multiple runs are listed newest-first.
- Verify a matching state/EventLog returns `ready`.
- Verify a mismatched `last_event_seq` returns `blocked` with an explicit reason.
- Run `cargo test -p kiana-tasks --test workflow_runtime --locked` and record the passing result.

### Task 3: CLI implementation

- Add `workflow list` and `workflow continue` routes.
- Support `--json` and an optional explicit run ID for continue.
- Keep text output concise and include blocker/recommended action.
- Update command usage text.

### Task 4: CLI post-implementation verification

- Extend `kiana-commands/src/tasks.rs` tests.
- Verify `workflow list --json` returns initialized runs.
- Verify `workflow continue --json` selects the latest run.
- Verify a state/EventLog conflict is returned as `blocked` rather than success.
- Run `cargo test -p kiana-commands workflow --locked` after implementation and record the result.

### Task 5: Verification

- `cargo test -p kiana-tasks --test workflow_runtime --locked`
- `cargo test -p kiana-commands workflow --locked`
- `cargo test --workspace --locked --offline --no-fail-fast`
- `cargo fmt --all --check`
- `git diff --check`

## Completion Evidence

- Completed on 2026-07-10.
- Domain verification: 6 workflow runtime integration tests passed.
- CLI verification: 5 workflow-filtered command tests passed, including ready and blocked resume projections.
- Full regression: `cargo test --workspace --locked --offline --no-fail-fast` passed, including doctests.
- Quality checks: `cargo fmt --all --check` and `git diff --check` passed.
- Commercial blocker report: 13 blocking checks remain, of which 12 require external release evidence and 1 is the intentionally dirty tracked worktree.
