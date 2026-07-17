# EDA Netlist Structural Review Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

> **Historical completion record:** All checked items below record already completed work and are not a reusable test-first execution order. Kiana's current project rule is production implementation first, followed by focused, adversarial, integration, and release verification.

**Goal:** Extend `/eda review` with bounded KiCad XML netlist parsing, structural power/interface/ERC findings, and release-grade evidence.

**Architecture:** `eda_netlist.rs` owns XML parsing and net-level rules; `eda.rs` owns CLI intake, immutable snapshots, BOM cross-checks, report projection, Evidence and VerificationPacket. The existing EDA schema remains v1 with backward-compatible optional P1 fields and rule version v2.

**Tech Stack:** Rust, `quick-xml 0.41`, serde, existing Workflow/Evidence APIs, JSON Schema smoke, Cargo integration tests.

---

### Task 1: Add isolated netlist parser and rules

**Files:**
- Create: `kiana-commands/src/eda_netlist.rs`
- Modify: `kiana-commands/src/lib.rs`
- Modify: `kiana-commands/Cargo.toml`
- Modify: `Cargo.lock`

- [x] Add `quick-xml = "0.41"`; the default commercial binary must satisfy the resolved dependency advisory gate while optional native-computer-use dependencies remain separately scoped.
- [x] Implement `analyze_kicad_netlist(bytes: &[u8]) -> Result<NetlistAnalysis>` with strict root/section/ref/code/node validation.
- [x] Require unique direct-child `<components>`/`<nets>` sections and direct-child `<comp>`/`<net>`/`<node>` records.
- [x] Reject DOCTYPE, processing instructions, general entity references, and non-whitespace text outside `<export>`.
- [x] Normalize component designators and reject duplicate `ref:pin` nodes before structural rule evaluation.
- [x] Implement power/interface/no-connect classification and deterministic issue ordering.
- [x] Add unit tests for duplicate refs, duplicate net code, unknown node refs, dangling interface and power warnings.
- [x] Run `cargo test -p kiana-commands eda_netlist --locked --offline --no-fail-fast` after implementation and record the result.

### Task 2: Bind netlist analysis into EDA report

**Files:**
- Modify: `kiana-commands/src/eda.rs`
- Test: `kiana-commands/tests/eda_command.rs`

- [x] Add `netlist` to `EdaOptions`, `ResolvedSources::ordered`, `resolve_sources`, usage and review-id material.
- [x] Parse only `ResolvedArtifact.contents`; never reopen the netlist path after hashing.
- [x] Map `NetlistIssue` to `EdaFinding` and cross-check netlist refs against BOM refs.
- [x] Add summary fields and `netlist_structure` check.
- [x] Set `RULE_VERSION` to v2 while preserving report schema v1.

### Task 3: Record post-implementation command and report verification

**Files:**
- Modify: `kiana-commands/tests/eda_command.rs`
- Modify: `kiana-commands/src/eda.rs`

- [x] After Tasks 1 and 2 were implemented, add `--netlist` to fixture arguments and create a valid KiCad XML fixture.
- [x] Assert source kind `netlist`, rule version v2, netlist summary counts and `netlist_structure` check.
- [x] Add a BOM coverage failure fixture and assert `review_required` plus Fail VerificationPacket.
- [x] Add malformed XML and unknown component reference tests that assert no WorkflowRun exists.
- [x] Run `cargo test -p kiana-commands --test eda_command --locked --offline --no-fail-fast` and record the result.
- [x] Run EDA integration tests and confirm all P0 checks pass.

### Task 4: Extend schema, packaged fixtures and docs

**Files:**
- Modify: `docs/schemas/kiana-eda-review.v1.schema.json`
- Modify: `scripts/schema-contract-smoke.sh`
- Modify: `scripts/package-lifecycle-smoke.sh`
- Modify: `scripts/release-smoke.sh`
- Modify: `USAGE.md`
- Modify: `docs/reference-feature-matrix.md`
- Modify: `docs/reference-migration-roadmap.md`
- Modify: `docs/commercial-release-readiness.md`

- [x] Accept `rule_version` v1/v2 and source kind `netlist`.
- [x] Add optional non-negative P1 summary fields and validate a v2 fixture.
- [x] Add netlist files/options to packaged and release binary EDA smokes.
- [x] Assert persisted report summary/check values before schema validation.
- [x] Document the conservative rule set and explicit non-goals.

### Task 5: Verify and refresh release evidence

**Files:**
- Update generated RC evidence only; do not commit or push without explicit authorization.

- [x] Run focused parser and EDA command tests.
- [x] Run `cargo fmt --all --check` and `git diff --check`.
- [x] Run `cargo check --workspace --locked --offline`.
- [x] Run `cargo test --workspace --locked --offline --no-fail-fast`.
- [x] Run `bash scripts/schema-contract-smoke.sh`.
- [x] Run `bash scripts/release-preflight.sh --local-rc`.
- [x] Build a new timestamped RC and run `scripts/package-lifecycle-smoke.sh` against the archive.
- [x] Regenerate recovery-integrity and local RC evidence without fabricating external proofs.
