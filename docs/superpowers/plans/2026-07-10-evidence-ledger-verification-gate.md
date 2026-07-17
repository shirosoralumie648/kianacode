# Evidence Ledger and Verification Gate P0 Implementation Plan

> **For agentic workers:** This project does not use TDD. Implement each contract first, then run the listed focused verification and request code review before accepting the slice.

**Goal:** Implement the complete P0-M3 evidence and verification product loop defined in `docs/superpowers/specs/2026-07-10-evidence-ledger-verification-gate-design.md`.

**Architecture:** Add pure evidence contracts as typed Workflow EventLog projections and immutable workflow-local packets in `kiana-tasks`. Reuse the existing deterministic `kiana checks` runner from thin `kiana-commands` adapters. Keep workers as ResultPacket producers and the coordinator as the only EventLog writer. Project Board consumes resolvable VerificationPacket references rather than trusting arbitrary completion strings.

**Tech Stack:** Rust, serde/serde_json, append-only JSONL, atomic JSON packet writes, existing WorkflowRun and Task storage, Cargo tests.

---

### Task 1: Evidence domain and workflow-local persistence

**Files:**
- Create: `kiana-tasks/src/evidence.rs`
- Modify: `kiana-tasks/src/lib.rs`
- Modify: `kiana-tasks/src/workflow.rs`
- Modify: `kiana-tasks/tests/evidence_ledger.rs`
- Modify: workflow tests under `kiana-tasks/tests/` as needed

- [x] Define `EvidenceKind`, `EvidenceStatus`, `EvidenceSource`, `EvidenceEventDraft`, `EvidenceEvent`, `VerificationCheck`, `VerificationStatus`, `VerificationPacket`, `LedgerSummary`, and structured errors.
- [ ] Implement strict EvidenceEvent validation and bounded/redacted output helpers.
- [x] Extend WorkflowEventKind and use EventLog as the sole sequence/fact source; `evidence/` stores artifacts only.
- [x] Implement a single-writer EventLog lease, EvidenceEvent projection, duplicate detection, schema compatibility checks, and corrupt-tail errors.
- [x] Implement atomic VerificationPacket writes and packet reads with evidence-reference validation.
- [x] Implement Verification final-status precedence exactly as specified.

### Task 2: Domain verification for EvidenceEvent and VerificationPacket

**Files:**
- Create: `kiana-tasks/tests/evidence_ledger.rs`
- Modify: `kiana-tasks/src/lib.rs`

- [x] Add event-validation coverage for empty identity, empty source, invalid confidence, skipped-without-reason, and command pass without exit code `0`.
- [x] Add append/read coverage requiring monotonic sequence, unique event IDs, restart-safe JSONL reads, and malformed-tail detection.
- [x] Add VerificationPacket coverage for pass, fail, blocked, inconclusive, optional failure, and zero-required-check behavior.
- [ ] Extend WorkflowRun initialization tests to require the new artifact shape.
- [x] Run `cargo test -p kiana-tasks --test evidence_ledger --locked` after implementation and record the result.

### Task 3: `kiana validate` implementation

**Files:**
- Create: `kiana-commands/src/validate.rs`
- Modify: `kiana-commands/src/lib.rs`
- Modify: `kiana-commands/src/registry.rs`
- Modify: `kiana-commands/src/checks.rs`
- Modify: `kiana-tasks/src/evidence.rs`

- [ ] Parse `--json`, `--workflow`, `--task`, `--task-list`, and `--profile` without silently accepting unknown/duplicate arguments.
- [ ] Resolve explicit or latest WorkflowRun and run resume consistency before executing checks.
- [ ] Expose only the minimum checks report types/functions needed by the adapter.
- [ ] Convert each check result to a typed EvidenceEvent with bounded/redacted output.
- [ ] Write a VerificationPacket and return `kiana.validation-run.v1` JSON plus concise text output.
- [ ] Reject unsupported profiles instead of weakening them silently.

### Task 4: `kiana validate` verification coverage

**Files:**
- Create: `kiana-commands/src/validate.rs`
- Modify: `kiana-commands/src/lib.rs`
- Modify: `kiana-commands/src/registry.rs`
- Modify: `kiana-commands/src/checks.rs`

- [ ] Add a registry test requiring `validate`.
- [ ] Add no-workflow coverage with actionable `workflow_not_found` output.
- [ ] Add JSON success coverage that creates a WorkflowRun, executes a deterministic temporary project check profile, and asserts event/packet paths and `final_status: pass`.
- [ ] Add command-failure coverage asserting durable fail evidence and `final_status: fail`.
- [ ] Add inconsistent-workflow coverage asserting no optimistic packet is written.
- [ ] Run `cargo test -p kiana-commands validate --locked` after implementation and record the result.

### Task 5: Task evidence linkage and Project Board verification resolution

**Files:**
- Modify: `kiana-commands/src/tasks.rs`
- Modify: `kiana-commands/src/validate.rs`
- Modify: `kiana-tasks/src/project_board.rs`
- Modify: `kiana-tasks/tests/project_board.rs`
- Modify: command tests in `kiana-commands/src/validate.rs` and `kiana-commands/src/project.rs`

- [x] Implement a crate-local Task metadata evidence-reference updater using normalized project-relative paths.
- [x] Append the VerificationPacket reference only after the packet is durably written.
- [x] Resolve `verification:<path>#<id>` in Project Board and require same-task `final_status: pass` for strict evidence.
- [x] Preserve legacy evidence strings but emit a compatibility policy finding.
- [x] Add post-implementation regression coverage for preserving existing metadata, de-duplicating references, missing Task errors, and task/packet identity mismatch.
- [x] Verify completed tasks with missing/failed/unresolved packets remain Blocked.

Implementation evidence (2026-07-10):

- `build_project_board_at_root` resolves only project-relative `verification:<path>#<id>` references and rejects path traversal.
- Completion requires a readable packet with matching verification ID/Task ID, canonical in-project path, internally consistent checks/counts/final status, and evidence backed by the same WorkflowRun EventLog.
- Completed dependencies use the same strict gate; legacy strings no longer unblock downstream work.
- `validate --task` now resolves and validates the Task target before running checks, recording EvidenceEvents, or writing a VerificationPacket.
- Missing tasks, duplicate task identities, non-object metadata, non-array evidence, and non-string evidence entries fail without orphan validation artifacts.
- Exact Task evidence references are de-duplicated while preserving unrelated metadata and legacy evidence.
- Task evidence updates hold a task-list lease across validation, checks, packet creation and Task回写；并使用唯一临时文件与写入前二次校验，避免 Kiana 并发写者覆盖引用。
- Focused verification passed: 11 Project Board tests, 5 Evidence Ledger tests, 8 validate integration tests, 5 project command tests, 13 task command tests, and the registry test.
- `cargo fmt --all --check` and `git diff --check` passed after the slice.

### Task 6: `kiana evidence` inspection and integrity verification

**Files:**
- Create: `kiana-commands/src/evidence.rs`
- Modify: `kiana-commands/src/lib.rs`
- Modify: `kiana-commands/src/registry.rs`
- Modify: `kiana-tasks/src/evidence.rs`

- [x] Implement explicit/latest workflow resolution and optional task filtering.
- [x] Implement integrity verification for sequence, duplicate IDs, schemas, packet references, and task references without rerunning checks.
- [x] Return `kiana.evidence-list.v1`, `kiana.evidence-show.v1`, and `kiana.evidence-integrity.v1`.
- [x] Register and document the command.
- [x] Add post-implementation regression coverage for `list`, `show`, and `verify` JSON/text output.

Implementation evidence (2026-07-10):

- `kiana evidence list/show/verify` is registered and emits the three versioned schemas above.
- Integrity verification checks exact packet path + ID Task links and both packet-level and check-level EvidenceEvent references.
- Seven integration tests cover missing WorkflowRun, latest-run task filtering, exact show, valid integrity, missing Task, missing check evidence, and wrong packet path.

### Task 7: `kiana report progress`

**Files:**
- Create: `kiana-commands/src/report.rs`
- Modify: `kiana-commands/src/lib.rs`
- Modify: `kiana-commands/src/registry.rs`
- Reuse: `kiana-tasks/src/project_board.rs`
- Reuse: `kiana-tasks/src/evidence.rs`

- [x] Implement `report progress [--json] [--workflow <run_id>] [--task-list <id>]`.
- [x] Produce concise Chinese text and `kiana.progress-report.v1` JSON from durable artifacts only.
- [x] Ensure corrupt/missing evidence is reported as Blocked, not omitted.
- [x] Add post-implementation regression coverage joining WorkflowRun, Board, latest VerificationPacket, evidence counts, blockers, and next action.

Implementation evidence (2026-07-10):

- `kiana report progress` is registered and emits concise Chinese text or `kiana.progress-report.v1` JSON.
- The report joins the selected WorkflowRun, root-aware Project Board, ledger summary, latest VerificationPacket, blockers, and next action without rerunning commands.
- Missing verification references remain visible as task blockers; corrupt EventLog data degrades to `evidence.status=blocked`, `evidence_ledger_unreadable`, and `repair:reconcile-eventlog` instead of aborting or omitting the fault.
- Five integration tests cover the durable join, missing WorkflowRun, missing verification blocker, Chinese text output, and corrupt EventLog recovery reporting.

### Task 8: `kiana audit strict` and ReviewPacket

**Files:**
- Create: `kiana-commands/src/audit.rs`
- Extend: `kiana-tasks/src/evidence.rs` or create a bounded review module if separation is clearer
- Modify: `kiana-commands/src/lib.rs`
- Modify: `kiana-commands/src/registry.rs`

- [x] Implement strict evidence/task consistency checks.
- [x] Implement bounded repository marker scanning with explicit exclusions for `.git`, `target`, `reference`, generated/vendor directories, binary files, and the audit specification itself.
- [x] Detect supported test/release/security/policy surfaces without inventing project-specific requirements.
- [x] Consume the commercial blocker report only when the supported script exists; preserve its external/local distinction.
- [x] Write ReviewPacket and matching review/blocker EvidenceEvents.
- [x] Return `kiana.strict-audit.v1` with severity, confidence, decision, evidence, owner, and next action.
- [x] Add post-implementation adversarial coverage for ledger corruption, missing task evidence, unresolved packet references, required skipped checks, and placeholder findings with file/line evidence.

Implementation evidence (2026-07-10):

- `ReviewPacket` now has workflow/run identity, reviewer type, scope, score, blocking count, recommendation, decision, owner-aware findings, creation time, evidence references, and next action.
- Review packets are written atomically under `review/`, can be read/listed, and append `ReviewCompleted` to the Workflow EventLog.
- `kiana audit strict` is registered and joins ledger integrity, VerificationPacket integrity, required-check state, root-aware Project Board findings, bounded marker scanning, and the optional commercial blocker report.
- Marker scanning is bounded to text source/config files, excludes `.git`, `.kiana`, `target`, `reference`, generated/vendor/build output, docs, tests, fixtures, binaries, and the audit implementation itself, and records file/line evidence without treating every marker as an automatic blocker.
- Corrupt EventLog data returns a blocked audit with `persistence_status=blocked` and no false packet path because appending auditable events is unsafe until recovery.
- Nine command integration tests cover missing WorkflowRun, marker evidence, commercial local/external blockers, corrupt ledger degradation, completed task without verification, required skipped checks, applicable projects without a test surface, plugin capability without a policy contract, and Cargo workspace member test discovery. One domain test covers ReviewPacket round-trip and workflow event persistence.
- `surfaces` applies requirements conditionally: language manifests activate test checks; explicit version/release artifacts activate release checks; plugin/MCP/hooks/permission hints activate security and policy checks. Non-applicable surfaces return `not_applicable` rather than creating synthetic blockers.
- Test surface discovery scans bounded workspace members while excluding `.git`, `.kiana`, `target`, `reference`, vendor/build output, docs, testdata, and fixtures, preventing Kiana's multi-crate layout from being misclassified as testless.

### Task 9: Documentation, smoke, review, and release evidence

**Files:**
- Modify: `USAGE.md`
- Modify: `docs/superpowers/plans/2026-07-10-evidence-ledger-verification-gate.md`
- Modify: relevant product volumes only when implementation reveals a contract correction

- [x] Document validate/evidence/report/audit commands with JSON and text examples.
- [x] Run focused tests for evidence, validate, project, report, and audit.
- [x] Run `cargo fmt --all --check` and `git diff --check`.
- [x] Run `cargo test --workspace --locked --offline --no-fail-fast`.
- [x] Build `kiana` and run real CLI smoke in a temporary project with a WorkflowRun.
- [ ] Request independent spec-compliance and code-quality review; close all Critical/Important findings.
- [x] Run `bash scripts/commercial-release-blockers-report.sh --json` and record exact local/external blocker counts.
- [x] Append completion evidence to this plan without claiming external release acceptance.

Verification evidence (2026-07-10):

- `cargo test -p kiana-commands --tests --locked --offline --no-fail-fast`: 252 command unit tests plus all command integration suites passed, including audit 6/6, evidence 7/7, report 5/5, and validate 8/8.
- `cargo test -p kiana-tasks --tests --locked --offline --no-fail-fast`: task unit 6/6, Evidence Ledger 5/5, Project Board 11/11, ReviewPacket 1/1, and Workflow Runtime 6/6 passed.
- `cargo test --workspace --locked --offline --no-fail-fast`: all workspace tests and doc-tests passed. The only warning remains the pre-existing `unused_mut` at `kiana-tools/src/agent.rs:5290`.
- `cargo build -p kiana-entrypoints --bin kiana --locked --offline` passed.
- Real CLI smoke in a temporary project initialized a WorkflowRun, ran `kiana audit strict --json`, persisted one ReviewPacket and one ReviewFinding EvidenceEvent, then read the event through `kiana report progress --json`.
- `cargo fmt --all --check` and `git diff --check` passed.
- `scripts/commercial-release-blockers-report.sh --json` returned `kiana.commercial-release-blockers.v1`, status `blocked`, 16 total checks, 13 blocking, 12 external blocking, 1 local blocking, and 3 satisfied. These are release-readiness facts, not a claim of commercial GA acceptance.

## Execution Note

The active `master` worktree is intentionally dirty and contains the current WorkflowRun, Project Board, documentation, and commercial-readiness changes. Do not reset, clean, or discard unrelated changes. Do not commit or push without a deliberate integration boundary. `reference/**` remains read-only.
