# Evidence Ledger and Verification Gate Design

**Status:** Approved by the existing Kiana Project OS product specification and active implementation goal.

**Scope:** P0-M3 Evidence Ledger + Verification Gate, including `kiana validate`, `kiana audit strict`, `kiana report progress`, workflow-local evidence persistence, VerificationPacket generation, Task evidence linkage, recovery checks, and user-facing progress projection.

## 1. Product Goal

Kiana must not infer completion from an agent message, a changed file, or a command that was merely attempted. It may call work Done only when a durable, inspectable chain proves:

1. what was executed;
2. which workflow and task it belonged to;
3. which check was required or optional;
4. what the check returned;
5. how the aggregate Verification Gate decided pass, fail, blocked, or inconclusive;
6. which unresolved risks or review findings remain;
7. where the supporting artifact can be read after a crash or later session.

The Evidence Ledger is the durable fact layer. VerificationPacket is the decision layer. Project Board, strict audit, progress reports, release proof, EDA review, and later dashboard views consume those two layers rather than inventing their own completion logic.

## 2. Non-Goals

P0-M3 does not:

- replace WorkflowRun or its event log;
- make worker processes direct writers to shared project state;
- treat ResultPacket as proof of success;
- upload evidence to a cloud service;
- provide cryptographic signing or third-party notarization;
- claim external acceptance, customer approval, release signatures, or live-service readiness;
- store unlimited raw command output;
- make every TODO or placeholder an automatic blocker without classification;
- implement the complete P1 review marketplace or dashboard.

## 3. Alternatives Considered

### 3.1 Global project ledger

Store every event in `.kiana/evidence/events.jsonl`.

Advantages:

- simple global queries;
- one file to inspect.

Rejected for P0 because workflow recovery, retention, export, fork, and cleanup become ambiguous. A damaged global file would also affect unrelated runs.

### 3.2 Workflow-local ledger with coordinator serialization

Store evidence facts as typed events in the WorkflowRun EventLog and store large evidence artifacts inside the same WorkflowRun directory. Only the coordinator appends events; workers return ResultPacket or command output to the coordinator.

Advantages:

- matches existing `.kiana/workflows/<run_id>/` recovery boundary;
- supports export and retention by workflow;
- prevents parallel workers from competing as state writers;
- reuses current WorkflowRun lookup and artifact ownership;
- limits corruption blast radius.

This is the selected P0 design.

### 3.3 Full event sourcing for all Kiana state

Make Evidence Ledger the only source of truth for workflow, tasks, policy, memory, and review.

Rejected for P0 because it would replace already working WorkflowRun and Task persistence. Evidence remains a bounded contract that can later feed broader projections.

## 4. Core Invariants

1. EvidenceEvent records facts; it does not decide completion.
2. VerificationPacket decides verification status from immutable EvidenceEvent references.
3. ResultPacket is execution output and must pass through Verification before it can support Done.
4. A required check without valid evidence prevents pass.
5. A skipped check must contain a non-empty reason.
6. A pass event for command-like evidence must include a successful exit code or an explicit non-command proof payload.
7. Evidence order is the Workflow EventLog sequence; no second sequence allocator exists.
8. Event IDs and verification IDs are unique within a workflow.
9. VerificationPacket references must resolve to events in the same workflow.
10. A completed Task without a resolvable passing VerificationPacket remains Blocked in Project Board.
11. A worker never writes the ledger directly; the coordinator serializes writes.
12. Corruption, missing references, incompatible schema, or partial packet writes produce Blocked, never optimistic pass.
13. External acceptance is represented only by an explicit approval/acceptance event from a named source; it is never synthesized from local tests.
14. Raw output is bounded and redacted before persistence.

## 5. Artifact Layout

The workflow artifact contract becomes:

```text
.kiana/workflows/<run_id>/
  workflow_dag.json
  state.json
  eventlog.jsonl
  evidence/
    commands/
  resultpackets/
    rp_<id>.json
  verification/
    vp_<id>.json
  reviews/
    review_<id>.json
```

Rules:

- `eventlog.jsonl` is the only append-only fact stream. Evidence Ledger is a typed projection of `evidence_recorded`, `verification_completed`, `review_completed`, approval, blocker, and related Workflow events.
- `evidence/` stores bounded command-output artifacts and referenced files, not a second mutable event stream.
- VerificationPacket and ReviewPacket use write-to-temp plus rename.
- EventLog is created when WorkflowRun is initialized; evidence and verification directories are created with the run.
- Existing WorkflowRun directories are migrated lazily by creating missing directories/files without rewriting existing artifacts.
- No command may truncate an existing ledger.

## 6. EvidenceEvent Contract

Canonical output schema:

```json
{
  "schema": "kiana.evidence-event.v1",
  "sequence": 12,
  "event_id": "evt_1720578000000_0012",
  "workflow_id": "wf_20260710_ab12cd",
  "run_id": "run_20260710_ab12cd",
  "task_id": "task_001",
  "workpacket_id": null,
  "recorded_at_ms": 1720578000000,
  "kind": "test_result",
  "status": "pass",
  "summary": "cargo test -p kiana-tasks --test evidence_ledger passed",
  "source": {
    "type": "local_command",
    "name": "kiana validate",
    "actor": "coordinator"
  },
  "payload": {
    "check_id": "cargo_test",
    "command": "cargo test --workspace --no-fail-fast",
    "exit_code": 0,
    "stdout_tail": "test result: ok",
    "stderr_tail": ""
  },
  "changed_files": [],
  "confidence": 1.0,
  "severity": null,
  "next_action": "review",
  "supersedes_event_id": null
}
```

### 6.1 Kinds

- `command_result`
- `diff_summary`
- `test_result`
- `build_result`
- `lint_result`
- `security_result`
- `review_finding`
- `approval_record`
- `blocker_record`
- `artifact_check`
- `eda_check`
- `memory_decision`

### 6.2 Status

- `pass`
- `fail`
- `blocked`
- `skipped`
- `unknown`

### 6.3 Validation

The writer rejects:

- empty workflow/run identity;
- empty summary;
- empty source type or name;
- non-finite confidence or confidence outside `[0, 1]`;
- `skipped` without a reason in payload;
- command-like `pass` with a non-zero or missing exit code;
- command-like `fail` with exit code `0` unless payload identifies a semantic assertion failure;
- duplicate event IDs;
- sequence gaps;
- unknown schema major version.

## 7. VerificationPacket Contract

```json
{
  "schema": "kiana.verification-packet.v1",
  "verification_id": "vp_1720578001000_ab12cd",
  "workflow_id": "wf_20260710_ab12cd",
  "run_id": "run_20260710_ab12cd",
  "task_id": "task_001",
  "profile": "project_p0",
  "created_at_ms": 1720578001000,
  "checks": [
    {
      "check_id": "cargo_test",
      "description": "Rust workspace tests",
      "required": true,
      "status": "pass",
      "evidence_id": "evt_1720578000000_0012",
      "command": "cargo test --workspace --no-fail-fast",
      "reason": null
    }
  ],
  "pass_count": 1,
  "fail_count": 0,
  "blocked_count": 0,
  "skipped_count": 0,
  "unknown_count": 0,
  "final_status": "pass",
  "evidence_ids": ["evt_1720578000000_0012"],
  "next_action": "review"
}
```

Final status precedence:

1. `fail` when any required check fails.
2. `blocked` when any required check is blocked, missing, has an unresolved reference, or the ledger cannot be read consistently.
3. `inconclusive` when any required check is skipped or unknown.
4. `pass` only when at least one required check exists and every required check passes.

Optional check failure is retained in the packet and progress report but does not by itself change a passing required set to fail. Release and strict-audit profiles may mark all checks required.

## 8. Validation Profiles

P0 defines the schema and routing rules for six profiles:

| Profile | Required surface |
| --- | --- |
| `quick` | formatting/compile sanity |
| `task_default` | formatting, compile, targeted tests |
| `project_p0` | formatting, compile, workspace tests, available smoke gates |
| `release` | project checks plus release/security/package gates |
| `audit_strict` | project checks plus evidence/task/policy/release consistency audit |
| `eda_review` | artifact completeness, rule checks, domain review evidence |

The first executable profile is `project_p0`, backed by the existing deterministic `kiana checks` engine. Unsupported or underconfigured profiles return Blocked with a precise required action; they must not silently run a weaker profile.

## 9. Command Semantics

### 9.1 `kiana validate`

```text
kiana validate [--json] [--workflow <run_id>] [--task <task_id>]
               [--task-list <id>] [--profile project_p0]
```

Behavior:

1. Resolve the explicit WorkflowRun or the latest run.
2. Refuse to continue when no WorkflowRun exists.
3. Verify workflow state/eventlog consistency before writing evidence.
4. Run the selected profile through the existing checks engine.
5. Convert every check result into one EvidenceEvent.
6. Append events in deterministic check order.
7. Build and atomically write VerificationPacket.
8. When `--task` is present, append a stable VerificationPacket reference to Task metadata without deleting existing evidence.
9. Return a validation report containing final status, packet path, evidence IDs, counts, isolation mode, and next action.
10. Return command success only for Verification `pass`; fail/blocked/inconclusive remain machine-visible in JSON and produce an actionable command error in text/CLI status integration.

The command never claims external acceptance.

### 9.2 `kiana evidence`

```text
kiana evidence list [--json] [--workflow <run_id>] [--task <task_id>]
kiana evidence show [--json] --workflow <run_id> <event_id>
kiana evidence verify [--json] --workflow <run_id>
```

`verify` checks sequence continuity, duplicate IDs, schema compatibility, required fields, packet references, and task references. It does not rerun commands.

### 9.3 `kiana report progress`

```text
kiana report progress [--json] [--workflow <run_id>] [--task-list <id>]
```

The report joins:

- WorkflowRun status/current node;
- Project Board counts and blockers;
- latest VerificationPacket;
- evidence counts by kind/status;
- unresolved blocking evidence;
- stale or corrupt artifact findings;
- recommended next action.

Text output is concise Chinese. JSON output uses `kiana.progress-report.v1`.

### 9.4 `kiana audit strict`

```text
kiana audit strict [--json] [--workflow <run_id>] [--task-list <id>]
```

Strict audit checks:

- ledger integrity and schema compatibility;
- completed tasks without passing verification references;
- verification packets with missing evidence;
- required checks skipped/unknown/missing;
- unresolved blocker or blocking review evidence;
- scope/NOT_BUILDING violations when declared;
- suspicious fake/stub/TODO/hardcoded markers with file/line evidence and exclusions;
- test, release, security, and policy surface presence when applicable;
- commercial blocker report when the project exposes the supported report script.

Marker matches are findings, not automatic proof of a product defect. Severity and decision must be explicit. Strict audit writes ReviewPacket plus evidence events and never rewrites source files.

## 10. Task Evidence Linkage

When validation targets a Task, Kiana appends a reference under Task metadata:

```json
{
  "metadata": {
    "evidence": [
      "verification:.kiana/workflows/run_001/verification/vp_001.json#vp_001"
    ]
  }
}
```

Rules:

- preserve existing evidence references;
- de-duplicate exact references;
- use project-relative normalized paths;
- never set Task status to completed automatically;
- Project Board resolves verification references and only treats a Task as evidenced when the packet exists, belongs to the same task, and has `final_status: pass`;
- legacy free-form evidence strings remain readable but produce a compatibility finding until migrated.

## 11. Recovery and Consistency

### 11.1 Crash during event append

- each EvidenceEvent is embedded in one newline-terminated Workflow EventLog record;
- a malformed EventLog line blocks ledger verification;
- recovery reports the byte/line location and required repair action;
- P0 does not silently truncate the tail.

### 11.2 Crash during packet write

- write `<id>.json.tmp`;
- flush and rename to `<id>.json`;
- stale `.tmp` files are reported but never treated as packets.

### 11.3 Workflow state conflict

`kiana validate` first invokes the same state/EventLog reconciliation used by workflow resume. A conflicted workflow cannot receive optimistic verification evidence.

### 11.4 Fork

A fork copies immutable historical evidence and creates new VerificationPackets under the forked run identity. New events may reference historical IDs through provenance, but a packet may aggregate only events available in its run artifact boundary.

### 11.5 Concurrency

Workers can execute checks in parallel but return ResultPackets to one coordinator. The coordinator serializes ledger appends. Direct multi-process append is explicitly unsupported in P0 and must return a busy/ownership error once a writer lease is introduced; it must never be presented as conflict-free.

## 12. Redaction and Retention

- Persist bounded stdout/stderr tails, default 8 KiB each.
- Redact common bearer tokens, API-key assignments, password assignments, and private-key bodies before writing.
- Persist command text only after applying the same redaction.
- Record truncation flags and original byte counts.
- Critical verification/release/approval evidence is retained with the workflow.
- High-volume raw logs stay outside the ledger and are referenced by path plus digest when available.
- Evidence deletion is a later governed retention action and must itself create an audit event.

## 13. Errors

All domain errors expose:

- `code`;
- `message`;
- `evidence` or artifact path;
- `suggested_next`;
- `recoverable`.

Primary error codes:

- `workflow_not_found`
- `workflow_inconsistent`
- `ledger_corrupt`
- `schema_incompatible`
- `duplicate_event_id`
- `sequence_conflict`
- `invalid_evidence_event`
- `missing_evidence_reference`
- `verification_blocked`
- `verification_failed`
- `task_not_found`
- `task_evidence_mismatch`
- `profile_unsupported`

## 14. Acceptance Criteria

P0-M3 is complete when:

1. Workflow initialization creates the EventLog plus `evidence/` and `verification/`; no second evidence event stream exists.
2. EvidenceEvent schema validation rejects invalid identity, source, status/payload combinations, skipped-without-reason, duplicate IDs, and sequence gaps.
3. Ledger append/read survives restart and detects malformed/incompatible records.
4. VerificationPacket status follows the documented precedence and reference validation.
5. `kiana validate --profile project_p0` runs real checks, writes events and a packet, and reports pass/fail without fabricated evidence.
6. `--task` appends a resolvable passing verification reference while preserving existing metadata.
7. Project Board distinguishes resolvable passing verification from legacy/unresolved evidence.
8. `kiana evidence list/show/verify` reads the durable ledger.
9. `kiana report progress` generates text and JSON only from current artifacts.
10. `kiana audit strict` reports evidence/task/release/policy gaps and writes review evidence.
11. Crash/corruption cases return Blocked with actionable repair information.
12. Focused tests, command tests, full offline workspace tests, format checks, diff checks, independent review, CLI smoke, and commercial blocker refresh all complete.

## 15. Delivery Order

1. EvidenceEvent and VerificationPacket domain contracts.
2. Workflow artifact migration and durable ledger persistence.
3. `kiana validate --profile project_p0`.
4. Task evidence reference write and Project Board resolution.
5. `kiana evidence` inspection and integrity verification.
6. `kiana report progress` projection.
7. `kiana audit strict` and ReviewPacket persistence.
8. Full regression, review, documentation, and blocker refresh.
