# Workflow Transition And Completion Implementation Plan

> **For agentic workers:** This project does not use TDD. Implement the approved transition contract first, then run focused verification and the execution-plan checkpoints. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add evidence-bound DAG advancement and VerificationPacket-gated WorkflowRun completion without manual state or EventLog edits.

**Architecture:** `kiana-tasks::workflow` owns atomic transition event batches and state projection. `kiana-commands::tasks` owns CLI parsing, evidence path validation, DAG decision resolution, VerificationPacket revalidation, and JSON/human reports. Existing Evidence Ledger, writer lease, integrity key, event signing, and package schema paths remain authoritative.

**Tech Stack:** Rust, serde/serde_json, sha2, existing WorkflowRun and Evidence Ledger APIs, Bash release gates.

---

### Task 1: Freeze schemas and command contracts

**Files:**
- Create: `docs/schemas/kiana-workflow-transition.v1.schema.json`
- Create: `docs/schemas/kiana-workflow-completion.v1.schema.json`
- Create: `kiana-commands/tests/workflow_transition_command.rs`

- [ ] Add strict JSON schemas for transition and completion reports.
- [ ] Define the legal edge, evidence containment, packet integrity, idempotency, and stable report contracts consumed by Tasks 2-4.

### Task 2: Implement atomic Workflow transition batching

**Files:**
- Modify: `kiana-tasks/src/workflow.rs`
- Modify: `kiana-tasks/src/lib.rs`
- Test: `kiana-tasks/tests/workflow_runtime.rs`

- [ ] Add a transition draft/result type carrying source, destination, decision, transition ID, evidence descriptors, and note.
- [ ] Append `gate_evaluated`, `node_exited`, and `node_entered` under one writer lease and integrity chain.
- [ ] Sync the EventLog before projecting the final entered node into `state.json`.
- [ ] Add unit tests proving ordered events, one transition ID, state projection, signed verification, and failure-before-state behavior.

### Task 3: Implement `workflow advance`

**Files:**
- Modify: `kiana-commands/src/tasks.rs`
- Test: `kiana-commands/tests/workflow_transition_command.rs`

- [ ] Parse `advance [--json] --decision <decision> [--evidence <path>]... [--note <text>] <run_id>` without silently accepting duplicate scalar options or unknown flags.
- [ ] Reconcile state, resolve exactly one persisted DAG edge, and report allowed decisions on failure.
- [ ] Validate Workflow-relative evidence containment, regular-file status, non-placeholder content, byte count, and SHA-256.
- [ ] Call the atomic transition API and emit `kiana.workflow-transition.v1`.

### Task 4: Implement `workflow complete`

**Files:**
- Modify: `kiana-commands/src/tasks.rs`
- Test: `kiana-commands/tests/workflow_transition_command.rs`

- [ ] Parse `complete [--json] --verification <verification_id> <run_id>`.
- [ ] Require reconciled running state at the `completed` terminal node.
- [ ] Read the exact same-run VerificationPacket and Evidence Ledger.
- [ ] Re-run packet integrity and completion validation and require `final_status=pass`.
- [ ] Bind packet path/hash/counts into `workflow_completed` and return `kiana.workflow-completion.v1`.
- [ ] Make same-packet retries idempotent and reject a different completion packet.

### Task 5: Add command verification coverage

**Files:**
- Modify: `kiana-commands/tests/workflow_transition_command.rs`

- [ ] Add command coverage for legal edge advancement and event sequence metadata.
- [ ] Add coverage for unknown decisions, terminal advancement, missing/placeholder evidence, absolute paths, `..`, directory paths, and symlink escape.
- [ ] Add coverage for missing, foreign, tampered, and non-pass VerificationPackets.
- [ ] Add coverage for successful and idempotent completion.
- [ ] Run `cargo test -p kiana-commands --test workflow_transition_command --locked --offline --no-fail-fast` and record the result.

### Task 6: Wire docs and release gates

**Files:**
- Modify: `USAGE.md`
- Modify: `docs/workflow-runtime-design.md`
- Modify: `docs/reference-migration-roadmap.md`
- Modify: `docs/reference-feature-matrix.md`
- Modify: `docs/commercial-release-readiness.md`
- Modify: `scripts/release-preflight.sh`
- Modify: `scripts/package-lifecycle-smoke.sh`

- [ ] Document advance/complete commands, evidence boundaries, and validation prerequisite in Chinese.
- [ ] Require new schemas, source, tests, design, and plan in preflight.
- [ ] Exercise a fixture WorkflowRun transition/completion in package lifecycle smoke without fabricating external proof.

### Task 7: Produce a real local release Workflow proof

**Files:**
- Create runtime artifacts only under `.kiana/workflows/` and `dist/proofs/workflow/`.

- [ ] Initialize the Workflow integrity key without exposing it.
- [ ] Create a new `ship` WorkflowRun for the verified offline-eval release slice.
- [ ] Populate each referenced Workflow artifact with actual decisions/evidence, not header-only placeholders.
- [ ] Advance through the legal DAG path using recorded decisions.
- [ ] Run `kiana validate --workflow <run_id> --json` and require a passing VerificationPacket.
- [ ] Enter the `completed` terminal node and run `workflow complete` with that packet.
- [ ] Generate `dist/proofs/workflow/recovery-integrity.json` from the same completed run and current Git state.
- [ ] Re-run the blocker report with `KIANA_RELEASE_WORKFLOW_RUN_ID`; expect `workflow.recovery-integrity` satisfied while `source.clean-tracked-tree` remains local blocking.

### Task 8: Complete verification

- [ ] Run focused command and workflow runtime tests.
- [ ] Run `bash scripts/schema-contract-smoke.sh`.
- [ ] Run `cargo test --workspace --locked --offline --no-fail-fast`.
- [ ] Run `cargo build --workspace --locked --offline`.
- [ ] Run `cargo fmt --all --check` and `git diff --check`.
- [ ] Run `bash scripts/release-smoke.sh`.
- [ ] Build a fresh temporary release package and run `scripts/package-lifecycle-smoke.sh`.
- [ ] Run `KIANA_PREFLIGHT_SKIP_COMPLIANCE=1 bash scripts/release-preflight.sh --local-rc`.
