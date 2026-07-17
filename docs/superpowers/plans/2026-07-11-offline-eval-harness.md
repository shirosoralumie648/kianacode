# Offline Eval Harness Implementation Plan

> **Execution rule:** This project does not use TDD. The checked steps below are a completed evidence inventory, not a required execution order; future maintenance implements the approved contract first, then runs focused, adversarial, integration, and package verification.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a deterministic, read-only `kiana eval run` command that evaluates packaged RuntimeEvent JSONL fixtures and emits a schema-pinned report.

**Architecture:** `kiana-commands::eval` owns suite parsing, path containment, event normalization, metrics, assertions, report rendering, and CLI argument parsing. The command reads only suite-relative local files; schemas and basic fixtures ship in `docs/eval`, and package lifecycle smoke executes the packaged fixture.

**Tech Stack:** Rust, serde/serde_json, sha2, existing command registry, JSON Schema smoke scripts, Cargo integration tests.

---

### Task 1: Freeze command and schema contracts

**Files:**
- Create: `docs/schemas/kiana-eval-suite.v1.schema.json`
- Create: `docs/schemas/kiana-eval-report.v1.schema.json`
- Create: `docs/eval/fixtures/basic-runtime-suite.json`
- Create: `docs/eval/fixtures/basic-tool-success.jsonl`
- Create: `docs/eval/fixtures/basic-tool-failure.jsonl`

- [x] Define suite schema with strict `additionalProperties: false`, unique semantic case IDs, one supported kind, and at least one expectation field.
- [x] Define report schema with stable summary, case metrics, findings, and SHA-256 fields.
- [x] Add one passing fixture and one intentionally failing fixture used by focused tests.
- [x] Run `bash scripts/schema-contract-smoke.sh` after the schema contract is implemented and require schema parsing to pass.

### Task 2: Eval command regression verification evidence

**Files:**
- Create: `kiana-commands/tests/eval_command.rs`
- Modify: `kiana-commands/src/registry.rs`

- [x] Add a registry test requiring `eval` in the default command set.
- [x] Add tests for passing JSON report metrics and deterministic fixture hashes.
- [x] Add tests for assertion failures and `--fail-on-failure` behavior.
- [x] Add validation tests for duplicate IDs, unknown kind, empty expectations, malformed JSONL, absolute paths, `..` escape, directory paths, and symlink escape.
- [x] Run `cargo test -p kiana-commands --test eval_command --locked --offline --no-fail-fast` after implementation and require all focused checks to pass.

### Task 3: Implement suite parsing and path containment

**Files:**
- Create: `kiana-commands/src/eval.rs`
- Modify: `kiana-commands/src/lib.rs`
- Modify: `kiana-commands/src/registry.rs`

- [x] Add serde structs for suite, cases, expectations, report, metrics, and findings.
- [x] Parse `eval run --suite <path> [--json] [--fail-on-failure]` and reject unknown/duplicate options.
- [x] Canonicalize suite directory and fixture paths; require every fixture to remain inside the suite directory and be a regular file.
- [x] Enforce 256-case, 16-MiB, and 100,000-line limits.
- [x] Register `EvalCommand` and export the module.
- [x] Run focused validation tests; expect path and contract tests to pass.

### Task 4: Implement RuntimeEvent replay metrics and assertions

**Files:**
- Modify: `kiana-commands/src/eval.rs`
- Test: `kiana-commands/tests/eval_command.rs`

- [x] Accept direct RuntimeEvent objects and records containing an `event` object.
- [x] Compute event counts, tool lifecycle metrics, token usage, final status/reason/text, tool-name set, and fixture SHA-256.
- [x] Evaluate every supported expectation and emit deterministic finding codes.
- [x] Produce `kiana.eval-report.v1` JSON and concise human output.
- [x] Return command error only for invalid suite/fixture or `--fail-on-failure` with failed cases.
- [x] Run all eval command tests; expect pass.

### Task 5: Wire CLI/help and packaged fixtures

**Files:**
- Modify: `kiana-entrypoints/src/cli.rs` if explicit top-level help/routing requires the command name
- Modify: `USAGE.md`
- Modify: `scripts/package-release.sh`
- Modify: `scripts/package-lifecycle-smoke.sh`
- Modify: `scripts/release-preflight.sh`
- Modify: `scripts/release-smoke.sh`

- [x] Ensure `kiana eval --help` and `kiana eval run ...` route through the shared registry.
- [x] Copy `docs/eval` into release archives.
- [x] Run the packaged basic suite in lifecycle smoke and assert `schema=kiana.eval-report.v1` plus `status=passed`.
- [x] Add static preflight checks for schemas, fixture files, command registration, and package inclusion.

### Task 6: Update roadmap and evidence matrix

**Files:**
- Modify: `docs/reference-migration-roadmap.md`
- Modify: `docs/reference-feature-matrix.md`
- Modify: `docs/commercial-release-readiness.md`

- [x] Record the offline RuntimeEvent replay harness under Phase 8.
- [x] State explicitly that fake-provider scenarios, external benchmarks, model-quality claims, and customer acceptance remain open.
- [x] Record exact focused and package verification commands.

### Task 7: Complete verification

- [x] Run `bash scripts/schema-contract-smoke.sh`.
- [x] Run `cargo test -p kiana-commands --test eval_command --locked --offline --no-fail-fast`.
- [x] Run `cargo test -p kiana-commands --lib --locked --offline --no-fail-fast`.
- [x] Run `cargo test --workspace --locked --offline --no-fail-fast`.
- [x] Run `cargo build --workspace --locked --offline`.
- [x] Run `cargo fmt --all --check`.
- [x] Run `git diff --check`.
- [x] Build into a temporary `DIST_DIR` and run `scripts/package-lifecycle-smoke.sh` against the generated archive.

## Verification Evidence (2026-07-11)

- `bash scripts/schema-contract-smoke.sh`: passed.
- `cargo test -p kiana-commands --lib --locked --offline --no-fail-fast`: 277 passed.
- `cargo test -p kiana-commands --test eval_command --locked --offline --no-fail-fast`: 5 passed.
- `cargo test -p kiana-entrypoints --test cli_eval --locked --offline --no-fail-fast`: 1 passed.
- `cargo test --workspace --locked --offline --no-fail-fast`: passed across the full workspace and doc-tests.
- `cargo build --workspace --locked --offline`: passed.
- `cargo fmt --all --check` and `git diff --check`: passed.
- `bash scripts/release-smoke.sh`: passed after exercising the eval suite with both release and installed binaries.
- Temporary `DIST_DIR` package generation plus `bash scripts/package-lifecycle-smoke.sh`: passed for `kiana-0.1.0-linux-x86_64.tar.gz`.
- `KIANA_PREFLIGHT_SKIP_COMPLIANCE=1 bash scripts/release-preflight.sh --local-rc`: passed; package generation had already executed local-RC compliance audit.
