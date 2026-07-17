# EDA Review Workbench Implementation Plan

> **For agentic workers:** This project does not use TDD. Implement each approved EDA contract first, then use verification-before-completion before reporting completion.

**Goal:** Add a bounded `/eda review` workflow that validates local hardware project artifacts, emits immutable review deliverables, records Evidence Ledger events, and refuses unsafe or unsupported actions.

**Architecture:** `kiana-commands::eda` owns CLI parsing, project-relative path containment, lightweight BOM/CPL/Gerber analysis, deterministic report construction, WorkflowRun creation or resume, immutable artifact commit, and Evidence Ledger recording. The first release is a review workbench, not a schematic editor, router, CAM engine, procurement client, or electrical sign-off authority.

**Tech Stack:** Rust, serde/serde_json, sha2, existing `kiana-tasks` WorkflowRun and Evidence Ledger APIs, JSON Schema smoke checks, Cargo integration tests.

---

### Task 1: Freeze command and product contracts

**Files:**
- Create: `kiana-commands/tests/eda_command.rs`
- Create: `docs/schemas/kiana-eda-review.v1.schema.json`

- [ ] Require `/eda` in the default command registry.
- [ ] Define `eda review [--workflow <run_id>] [--requirements <path>] [--schematic <path>] [--bom <path>] [--gerber <path>] [--cpl <path>] [--constraints <path>] [--json]`.
- [ ] Define `pass`, `review_required`, and `blocked` review outcomes.
- [ ] Define immutable outputs: `eda_review.json`, `bom_risk.md`, and `bringup-plan.md`.
- [ ] Declare limitations and `hardware_order` approval requirements without executing external actions.

### Task 2: Implement bounded artifact intake and analyzers

**Files:**
- Create: `kiana-commands/src/eda.rs`
- Modify: `kiana-commands/src/lib.rs`
- Modify: `kiana-commands/src/registry.rs`
- Modify: `kiana-tasks/src/workflow.rs`
- Modify: `kiana-commands/src/tasks.rs`

- [ ] Add `WorkflowInputKind::Eda` and CLI parsing support.
- [ ] Resolve every supplied path against canonical project root and reject escapes before state mutation.
- [ ] Enforce regular-file inputs for requirements, schematic, BOM, CPL, and constraints; accept only a Gerber directory in P0.
- [ ] Parse bounded UTF-8 CSV for BOM/CPL without adding an unavailable dependency.
- [ ] Cross-check designators and packages and emit deterministic finding codes.
- [ ] Inventory Gerber fabrication layers and drill files using bounded directory traversal.
- [ ] Generate a conservative bring-up sequence from available evidence and explicit limitations.

### Task 3: Bind WorkflowRun, immutable artifacts, Evidence, and approvals

**Files:**
- Modify: `kiana-commands/src/eda.rs`
- Test: `kiana-commands/tests/eda_command.rs`

- [ ] Create a gated EDA WorkflowRun when `--workflow` is absent.
- [ ] Strictly resume an existing run and require `WorkflowInputKind::Eda` when `--workflow` is supplied.
- [ ] Derive `review_id` from rule version plus source paths and SHA-256 values.
- [ ] Commit all outputs through `commit_immutable_artifacts_with_unique_event`.
- [ ] Append exactly one `EvidenceKind::EdaCheck` event for each deterministic review identity.
- [ ] Record approval requirements for ordering, cost, production-file mutation, and automatic component replacement; execute none of them.

### Task 4: Add EDA command verification coverage

**Files:**
- Create: `kiana-commands/tests/eda_command.rs`

- [ ] Test registry exposure and non-interactive support.
- [ ] Test a complete local artifact set produces all three outputs, one workflow artifact event, and one EDA evidence event.
- [ ] Test missing critical artifacts produce a blocked report rather than a success claim.
- [ ] Test BOM/CPL designator and package mismatches produce actionable findings.
- [ ] Test absolute paths, `..`, and symlink escapes are rejected before WorkflowRun creation.
- [ ] Test retrying identical inputs in one WorkflowRun reuses the same review identity and does not duplicate evidence.
- [ ] Run `cargo test -p kiana-commands --test eda_command --locked --offline --no-fail-fast` after implementation and record the result.

### Task 5: Ship schemas, docs, and package surfaces

**Files:**
- Create: `docs/schemas/kiana-eda-review.v1.schema.json`
- Modify: `USAGE.md`
- Modify: `docs/workflow-runtime-design.md`
- Modify: `docs/reference-feature-matrix.md`
- Modify: `docs/reference-migration-roadmap.md`
- Modify: `docs/commercial-release-readiness.md`
- Modify: `scripts/package-release.sh`
- Modify: `scripts/package-lifecycle-smoke.sh`
- Modify: `scripts/release-preflight.sh`

- [ ] Add strict report schema and schema smoke coverage.
- [ ] Document supported inputs, deterministic outputs, approval boundaries, and explicit non-goals.
- [ ] Include the EDA schema in release archives and static preflight checks.
- [ ] Add a packaged local fixture smoke if the release layout already supports fixture directories.

### Task 6: Complete verification and rebuild local RC evidence

- [ ] Run focused EDA tests.
- [ ] Run `cargo fmt --all --check`.
- [ ] Run `git diff --check`.
- [ ] Run `cargo check --workspace --locked --offline`.
- [ ] Run `cargo test --workspace --locked --offline --no-fail-fast`.
- [ ] Run `bash scripts/schema-contract-smoke.sh`.
- [ ] Run `bash scripts/release-preflight.sh --local-rc`.
- [ ] Record the new RC directory, hashes, workflow proof, and remaining external blockers without claiming external release completion.
