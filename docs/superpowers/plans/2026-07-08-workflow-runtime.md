# Workflow Runtime Implementation Plan

> **Execution rule:** This project does not use TDD. Implement the core and CLI contracts first, then add and run focused, integration, and workspace verification.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a durable workflow runtime protocol that initializes complete Kiana workflow runs with DAG, state, event log, and artifacts.

**Architecture:** Implement the core runtime in `kiana-tasks` as a pure Rust library, then expose a small CLI surface from `kiana-commands/src/tasks.rs`. The runtime creates `.kiana/workflows/<run_id>/`, writes a complete default DAG, initializes append-only events, and persists state snapshots.

**Tech Stack:** Rust workspace, serde/serde_json, std filesystem APIs, existing command trait system.

---

### Task 1: Core Runtime Library

**Files:**
- Create: `kiana-tasks/src/workflow.rs`
- Modify: `kiana-tasks/src/lib.rs`
- Modify: `kiana-tasks/Cargo.toml`

- [ ] **Step 1: Implement workflow data types**

Define `WorkflowInit`, `WorkflowRun`, `WorkflowState`, `WorkflowDagTemplate`, `WorkflowNodeSpec`, `WorkflowEdge`, `WorkflowEvent`, `GateResult`, `WorkPacket`, `EvidencePacket`, and `ReviewPacket`.

- [ ] **Step 2: Implement default DAG**

Define `default_workflow_template()` with the full capture -> context -> research -> design -> plan -> execute -> quality -> verify -> review -> ship -> learn path plus branch nodes.

- [ ] **Step 3: Implement initialization**

Implement `initialize_workflow_run(root, init)` to create `.kiana/workflows/<run_id>/`, write `workflow_dag.json`, `state.json`, `eventlog.jsonl`, and initial markdown/json artifact directories.

### Task 2: Core Runtime Verification

**Files:**
- Create: `kiana-tasks/tests/workflow_runtime.rs`

- [ ] **Step 1: Add workflow initialization coverage**

Create tests asserting that `initialize_workflow_run` creates the run directory, `workflow_dag.json`, `eventlog.jsonl`, `state.json`, and the default artifact files.

- [ ] **Step 2: Run focused verification**

Run: `cargo test -p kiana-tasks workflow_runtime -- --nocapture`

Expected: tests pass.

### Task 3: CLI Surface Implementation

**Files:**
- Modify: `kiana-commands/src/tasks.rs`
- Modify: `kiana-commands/Cargo.toml`

- [ ] **Step 1: Add `kiana-tasks` dependency**

Add `kiana-tasks = { path = "../kiana-tasks", version = "0.1.0" }`.

- [ ] **Step 2: Add `tasks workflow` routes**

Support:

```text
kiana tasks workflow template --json
kiana tasks workflow init [--json] [--type <kind>] [--profile <quick|standard|gated>] <request>
kiana tasks workflow show <run_id>
```

- [ ] **Step 3: Keep existing public output compatibility**

Do not change unrelated CLI output while adding the workflow routes.

### Task 4: CLI Surface Verification

**Files:**
- Modify: `kiana-commands/src/tasks.rs`
- Modify: `kiana-commands/Cargo.toml`

- [ ] **Step 1: Add command verification coverage**

Add tests for `tasks workflow template --json` and `tasks workflow init --json <request>`.

- [ ] **Step 2: Run command verification**

Run: `cargo test -p kiana-commands workflow -- --nocapture`

Expected: tests pass.

### Task 5: Verification

**Files:**
- Entire workspace affected by new crates only.

- [ ] **Step 1: Run targeted crate tests**

Run:

```bash
cargo test -p kiana-tasks
cargo test -p kiana-commands tasks
```

- [ ] **Step 2: Run formatting check**

Run:

```bash
cargo fmt --check
```

- [ ] **Step 3: Review git status**

Run:

```bash
git status --short
```

Expected: only workflow runtime docs and implementation files are new/modified in addition to pre-existing unrelated dirty files.
