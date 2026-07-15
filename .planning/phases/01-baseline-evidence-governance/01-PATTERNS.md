# Phase 1: 现状基线与证据治理 - Pattern Map

**Mapped:** 2026-07-15
**Dispatch:** `gsd-pattern-mapper` via **generic-agent workaround** (typed `agent_type` was unavailable)
**Scope:** Phase 1 governance compiler only: versioned schemas/canonical JSON, semantic validation, deterministic generated views, focused fixtures/smoke, and at most one thin existing audit/report adapter.
**Files classified:** 18 logical targets (16 required, 2 conditional)
**Unique analog assignments:** 5
**Exact or role-match coverage:** 17 / 18; the append-only cross-family evidence index has no exact live analog.

> Line references below describe the live dirty working tree on 2026-07-15. Several analog files are modified or untracked; planners and implementers must re-anchor excerpts if those user-owned changes move. Do not reset or overwrite them.

## File Classification

The exact canonical directory split is still planner discretion. The paths below use the concrete layout proposed by `01-RESEARCH.md`; renaming is acceptable only if the four object-family boundaries remain separate.

| New/Modified File | Role | Data Flow | Closest Live Analog | Match Quality |
|---|---|---|---|---|
| `docs/schemas/kiana-public-parity-baseline.v1.schema.json` | config / contract | transform + validation | `docs/schemas/kiana-commercial-release-blockers.v1.schema.json` | exact role |
| `docs/schemas/kiana-reference-repository-registry.v1.schema.json` | config / contract | transform + validation | `docs/schemas/kiana-commercial-release-blockers.v1.schema.json` | exact role |
| `docs/schemas/kiana-capability-decisions.v1.schema.json` | config / contract | transform + validation | `docs/schemas/kiana-commercial-release-blockers.v1.schema.json` | exact role |
| `docs/schemas/kiana-capability-evidence-index.v1.schema.json` | config / contract | transform + validation | `docs/schemas/kiana-commercial-release-blockers.v1.schema.json` | exact role |
| `docs/agent-program/kiana-completion/governance/public-baselines/<snapshot-id>.json` | model / immutable store | file-I/O + snapshot | commercial blocker report contract | role match |
| `docs/agent-program/kiana-completion/governance/repository-registry/<snapshot-id>.json` | model / immutable store | file-I/O + snapshot | `docs/agent-program/kiana-completion/references.json` | migration seed only |
| `docs/agent-program/kiana-completion/governance/capability-decisions/<review-revision>.json` | model / revisioned store | file-I/O + append/review | commercial blocker `checks[]` + action contract | role match |
| `docs/agent-program/kiana-completion/governance/evidence/index.json` | model / append-only index | event-driven + file-I/O | no exact analog; borrow closed schema and audit evidence binding only | partial |
| `docs/agent-program/kiana-completion/governance/generated/public-parity.md` | generated view | batch transform | `render_handoff_markdown` in `scripts/commercial-release-blockers-report.sh` | role match; must add determinism |
| `docs/agent-program/kiana-completion/governance/generated/reference-governance.md` | generated view | batch transform | `render_handoff_markdown` in `scripts/commercial-release-blockers-report.sh` | role match; must add determinism |
| `scripts/validate-capability-governance.py` | service / validator CLI | file-I/O + graph transform | `scripts/validate-json-schema.py` | exact role, broader semantics |
| `scripts/generate-capability-governance.py` | service / renderer CLI | file-I/O + deterministic batch | `scripts/commercial-release-blockers-report.sh` embedded Python | role match; `--check` is new |
| `scripts/capability-governance-smoke.sh` | test runner | batch + request-response | `scripts/schema-contract-smoke.sh` | exact role |
| `scripts/fixtures/capability-governance/valid/**` | test fixtures | file-I/O + graph input | positive fixtures in `scripts/schema-contract-smoke.sh` | exact role |
| `scripts/fixtures/capability-governance/invalid/**` | test fixtures | file-I/O + negative graph input | invalid fixture loop in `scripts/schema-contract-smoke.sh` | exact role |
| `scripts/schema-contract-smoke.sh` | integration gate | batch | its existing schema/report slices | exact modification point |
| `kiana-commands/src/audit.rs` **or** `kiana-commands/src/report.rs` (conditional) | controller / thin adapter | request-response | current `AuditCommand` in `kiana-commands/src/audit.rs` | exact role |
| matching `kiana-commands/tests/{audit,report}_command.rs` (conditional) | integration test | request-response | `kiana-commands/tests/audit_command.rs` | exact role |

## Pattern Assignments

### 1. Four schemas and canonical JSON families

**Apply to:** all four new schema files and all four canonical JSON families.

**Primary analog:** `docs/schemas/kiana-commercial-release-blockers.v1.schema.json`

**Supporting migration input:** `docs/agent-program/kiana-completion/references.json` is a seed, not a shape to preserve as a second authority.

**Imports/auth:** JSON contracts have no imports or auth layer. Their guard is a stable schema identity plus closed validation at every object boundary.

**Versioned root contract** (`docs/schemas/kiana-commercial-release-blockers.v1.schema.json:1-18`):

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://kiana.local/schemas/kiana-commercial-release-blockers.v1.schema.json",
  "title": "Kiana Commercial Release Blockers v1",
  "type": "object",
  "required": [
    "schema",
    "version",
    "generated_at",
    "status",
    "release_tag",
    "summary",
    "checks",
    "action_plan"
  ],
  "properties": {
    "schema": {
      "const": "kiana.commercial-release-blockers.v1"
    }
```

Copy the `$schema` / `$id` / stable instance `schema` pattern. Each new family needs its own ID and lifecycle; do not create a universal union record.

**Closed enum-bearing records** (`docs/schemas/kiana-commercial-release-blockers.v1.schema.json:137-160`, `:235-236`):

```json
"properties": {
  "id": {
    "type": "string",
    "pattern": "^[a-z0-9][a-z0-9.-]*$"
  },
  "category": {
    "type": "string",
    "enum": [
      "source-control",
      "build-test",
      "signing",
      "distribution",
      "live-service",
      "acceptance"
    ]
  },
  "severity": {
    "type": "string",
    "enum": ["blocker", "warning", "info"]
  },
  "status": {
    "type": "string",
    "enum": ["satisfied", "blocking"]
  }
},
"additionalProperties": false
```

Copy strict IDs, enums, required fields, and `additionalProperties: false` at the root and nested object levels. Put reusable record shapes under `$defs`, following `docs/schemas/kiana-commercial-release-blockers.v1.schema.json:325-399`.

**Seed shape to migrate, not retain unchanged** (`docs/agent-program/kiana-completion/references.json:2-5`):

```json
"schema": "kiana.agent-reference-catalog.v1",
"expected_count": 38,
"references": [
  {"id": "12-factor-agents", "path": "reference/12-factor-agents", "domains": ["D10", "D17", "D18"]}
]
```

The new repository registry may import these `id` / `path` / `domains` values, but must add frozen revision kind/value, scan time, source availability, license state/evidence, freshness, aliases, and snapshot identity. The old file cannot remain an editable second completion source.

**Error handling/validation:** schema handles local shape only. Duplicate IDs, dangling references, 38/38 reconciliation, proof ordering, evidence binding, freshness, and summary equality belong in the semantic validator. Never claim that structural schema success proves a valid ledger.

**Testing pattern** (`scripts/schema-contract-smoke.sh:163-179`): generate/load a real positive instance, run the shared structural validator, then assert semantic summary relationships separately.

```bash
tmp_commercial_blockers="$(mktemp)"
bash scripts/commercial-release-blockers-report.sh --json > "$tmp_commercial_blockers"
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_commercial_blockers" >/dev/null
"$python" - "$tmp_commercial_blockers" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
action_plan = report.get("action_plan", {})
if action_plan.get("schema") != "kiana.commercial-release-action-plan.v1":
    raise SystemExit("commercial blocker action_plan schema mismatch")
if action_plan.get("total_actions") != report.get("summary", {}).get("blocking"):
    raise SystemExit("commercial blocker action_plan total does not match blocking summary")
PY
```

### 2. Semantic validator CLI

**Apply to:** `scripts/validate-capability-governance.py`.

**Analog:** `scripts/validate-json-schema.py`

**Imports pattern** (`scripts/validate-json-schema.py:4-11`):

```python
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any
```

Use Python standard library only. Add `hashlib` and, if needed, `urllib.parse`; do not add PyPI dependencies or ad hoc shell JSON parsing.

**Read and typed error boundary** (`scripts/validate-json-schema.py:14-20`):

```python
class ValidationError(Exception):
    pass


def load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)
```

The new validator should use a governance-specific exception carrying a stable code such as `duplicate_capability_id`, `unknown_reference`, `unsafe_path`, `proof_regression`, or `generated_view_drift`. Include the affected IDs in detail without changing the prefix.

**Recursive validation pattern** (`scripts/validate-json-schema.py:55-76`):

```python
def validate(schema: dict[str, Any], instance: Any, root: dict[str, Any], path: str) -> list[str]:
    errors: list[str] = []

    if "$ref" in schema:
        try:
            referenced = resolve_ref(root, schema["$ref"])
        except ValidationError as exc:
            return [f"{path}: {exc}"]
        return validate(referenced, instance, root, path)

    if "allOf" in schema:
        for index, subschema in enumerate(schema["allOf"]):
            errors.extend(validate(subschema, instance, root, f"{path}.allOf[{index}]"))

    if "not" in schema:
        forbidden = schema["not"]
        if isinstance(forbidden, dict) and not validate(forbidden, instance, root, path):
            errors.append(f"{path}: matched schema forbidden by not")

    if "if" in schema and "then" in schema:
        if not validate(schema["if"], instance, root, path):
            errors.extend(validate(schema["then"], instance, root, f"{path}.then"))
```

For Phase 1, preserve the accumulate-all-errors style, but build explicit indexes first and validate in deterministic order: sources/snapshots, repositories, capabilities/decisions, evidence, then recomputed summaries.

**Closed-object ingress validation** (`scripts/validate-json-schema.py:95-118`):

```python
if isinstance(instance, dict):
    required = schema.get("required", [])
    for key in required:
        if key not in instance:
            errors.append(f"{path}: missing required property {key!r}")

    properties = schema.get("properties", {})
    if isinstance(properties, dict):
        for key, value in instance.items():
            if key in properties:
                prop_schema = properties[key]
                if isinstance(prop_schema, dict):
                    errors.extend(validate(prop_schema, value, root, f"{path}.{key}"))

    additional = schema.get("additionalProperties")
    if additional is False and isinstance(properties, dict):
        allowed = set(properties)
        for key in instance:
            if key not in allowed:
                errors.append(f"{path}: unexpected property {key!r}")
```

**CLI/error/exit contract** (`scripts/validate-json-schema.py:164-189`):

```python
def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("schema", type=Path)
    parser.add_argument("instance", type=Path)
    args = parser.parse_args()

    try:
        schema = load_json(args.schema)
        instance = load_json(args.instance)
    except Exception as exc:  # noqa: BLE001
        print(f"failed to read JSON: {exc}", file=sys.stderr)
        return 2

    errors = validate(schema, instance, schema, "$")
    if errors:
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1

    return 0
```

Keep usage/read failures distinct from semantic invalidity. Validation must be fail closed and offline, normalize paths before opening them, reject absolute/`..`/symlink escapes, bound file sizes, and never emit secrets or local home paths.

**Testing:** every D-21 invariant and threats T-01 through T-05 need one named negative fixture with the exact stable error prefix asserted by `scripts/capability-governance-smoke.sh semantic-negative`.

### 3. Deterministic generator and generated views

**Apply to:** `scripts/generate-capability-governance.py` and both generated Markdown files.

**Analog:** embedded Python in `scripts/commercial-release-blockers-report.sh`

**Imports and CLI boundary** (`scripts/commercial-release-blockers-report.sh:1-71`):

```bash
#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
```

```python
import json
import os
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
```

The new generator should be a direct Python CLI rather than another large embedded-Python shell script. Copy the repo-root-relative behavior and standard-library boundary; do not copy network/subprocess discovery into generation.

**Derived summary and machine contract** (`scripts/commercial-release-blockers-report.sh:1773-1811`):

```python
blocking_checks = [check for check in checks if check["status"] == "blocking"]
external_blocking = [check for check in blocking_checks if check["external"]]
local_blocking = [check for check in blocking_checks if not check["external"]]
blocking_by_resolution_scope = {
    scope: sum(1 for check in blocking_checks if check["resolution_scope"] == scope)
    for scope in RESOLUTION_SCOPES
}

report = {
    "schema": "kiana.commercial-release-blockers.v1",
    "version": VERSION,
    "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "status": "blocked" if blocking_checks else "ready",
    "release_tag": EXPECTED_TAG,
    "summary": {
        "total_checks": len(checks),
        "satisfied": len(checks) - len(blocking_checks),
        "blocking": len(blocking_checks),
        "external_blocking": len(external_blocking),
        "local_blocking": len(local_blocking),
        "blocking_by_resolution_scope": blocking_by_resolution_scope,
    },
    "checks": checks,
    "action_plan": action_plan,
}
```

Copy the recompute-from-detail rule, not stored completion booleans. For Phase 1, the single completion predicate is `verified && current && proof_rank >= required_rank && current matching evidence`. Governance completion and product completion remain separate projections.

**Important divergence:** do **not** copy the wall-clock `generated_at` at line 1798. Generated governance views must use canonical snapshot/review time, sort all input collections explicitly, and render identical bytes for identical input.

**Escaping and stable writes** (`scripts/commercial-release-blockers-report.sh:1814-1855`, `:1906-1921`):

```python
def md_escape(value):
    return str(value).replace("|", "\\|").replace("\n", " ")


def render_handoff_markdown(report):
    summary = report["summary"]
    lines = [
        f"# Kiana Commercial Release Handoff {report['version']} ({report['release_tag']})",
        "",
        f"Generated: {report['generated_at']}",
        f"Status: {report['status']}",
    ]
```

```python
out = os.environ.get("KIANA_COMMERCIAL_BLOCKERS_OUT", "")
if out:
    path = Path(out)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

if os.environ.get("KIANA_BLOCKERS_FORMAT") == "json":
    print(json.dumps(report, indent=2, sort_keys=True))
```

Extend `md_escape`: reject raw unsafe HTML and escape table delimiters, control characters, and link targets. Add `<!-- generated; do not edit -->`, canonical snapshot IDs, explicit blocker reasons, UTF-8/LF output, and exactly one trailing newline.

**`--check` core:** render in memory, compare exact bytes to both checked-in views, and return non-zero `generated_view_drift:<repo-relative-path>` without rewriting. Normal generation may write only after all four canonical families pass structural and semantic validation.

**Error handling:** unknown/missing CLI arguments return 2 (matching `scripts/commercial-release-blockers-report.sh:18-24`, `:42-45`); invalid canonical data returns 1 with stable semantic codes; `--check` drift returns 1. Never silently choose a newest-looking snapshot.

### 4. Focused fixtures and smoke integration

**Apply to:** `scripts/capability-governance-smoke.sh`, both fixture trees, and the narrow invocation added to `scripts/schema-contract-smoke.sh`.

**Analog:** `scripts/schema-contract-smoke.sh`

**Harness/import pattern** (`scripts/schema-contract-smoke.sh:1-17`):

```bash
#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

python_bin() {
  command -v python3 2>/dev/null || command -v python 2>/dev/null || {
    echo "schema contract smoke requires python3 or python" >&2
    exit 1
  }
}

python="$(python_bin)"

for schema in docs/schemas/*.json; do
  "$python" -m json.tool "$schema" >/dev/null
done
```

Use the same fail-fast, repo-root, Python fallback pattern. The new focused runner should dispatch exactly these stable slices: `schemas`, `public-baseline`, `reference-governance`, `semantic-negative`, and `generated-views`; no argument runs all slices.

**Positive generated-output contract** (`scripts/schema-contract-smoke.sh:163-179`): generate the actual report, structurally validate it, then verify cross-field semantics. Do the same for both canonical generated views and use only fixture-local data.

**Negative fixture pattern** (`scripts/schema-contract-smoke.sh:628-650`):

```python
for path in sorted(invalid_dir.glob("*.json")):
    document = json.loads(path.read_text(encoding="utf-8"))
    if not semantic_errors(document):
        raise SystemExit(f"EDA negative fixture is not invalid: {path.name}")
```

```bash
for invalid_eda_review in \
  "$tmp_eda_invalid_dir/pass-missing-gerber.json" \
  "$tmp_eda_invalid_dir/pass-with-blocked.json" \
  "$tmp_eda_invalid_dir/pass-with-errors.json"
do
  if "$python" scripts/validate-json-schema.py \
    docs/schemas/kiana-eda-review.v1.schema.json \
    "$invalid_eda_review" >/dev/null 2>&1; then
    echo "EDA schema unexpectedly accepted invalid fixture: $(basename "$invalid_eda_review")" >&2
    exit 1
  fi
done
```

Copy the rule that each negative fixture must demonstrably fail. Improve it by asserting the expected stable prefix per fixture, rather than accepting any failure.

**End-to-end report assertion pattern** (`scripts/schema-contract-smoke.sh:2173-2207`, `:2224`):

```bash
tmp_report="$(mktemp)"
tmp_handoff="$(mktemp)"
bash scripts/commercial-release-blockers-report.sh --json --handoff-md "$tmp_handoff" > "$tmp_report"
"$python" scripts/validate-json-schema.py \
  docs/schemas/kiana-commercial-release-blockers.v1.schema.json \
  "$tmp_report" >/dev/null
"$python" - "$tmp_report" "$tmp_handoff" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
handoff = Path(sys.argv[2]).read_text(encoding="utf-8")
checks = report.get("checks", [])
if not checks:
    raise SystemExit("commercial blockers report has no checks")
if "## Blocking Assignments" not in handoff:
    raise SystemExit("commercial blockers handoff is missing assignment section")
PY

echo "schema contract smoke passed"
```

The Phase 1 focused gate must remain offline, avoid Cargo/full reference rescans, include one full 38-repository fixture, measure elapsed time, and target `<30s`. `scripts/schema-contract-smoke.sh` should call the focused runner once rather than duplicate its semantic implementation.

### 5. Conditional thin audit/report adapter

**Apply only if:** checked-in generated views plus scripts cannot demonstrate an observable Phase 1 success criterion. Choose one existing command surface, not both, and do not add entrypoint routing or duplicate validation.

**Analog:** `kiana-commands/src/audit.rs`

**Test companion:** `kiana-commands/tests/audit_command.rs`

**Imports and command registration pattern** (`kiana-commands/src/audit.rs:1-21`, `:23-50`):

```rust
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

pub struct AuditCommand;

#[async_trait]
impl Command for AuditCommand {
    fn name(&self) -> &str {
        "audit"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> Result<CommandResult> {
        let (subcommand, rest) = split_word(context.args.trim());
        match subcommand.unwrap_or("strict") {
            "strict" => audit_strict(&context, rest),
            "help" | "--help" | "-h" => Ok(CommandResult::text(usage())),
            other => Err(anyhow!("unknown audit command '{other}'\n\n{}", usage())),
        }
    }
}
```

Do not create a new `GovernanceCommand` unless the existing surface cannot express the report. The adapter should invoke/consume a serialized validated summary, return `CommandResult`, and remain non-interactive.

**Closed consumer contract** (`kiana-commands/src/audit.rs:71-91`):

```rust
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommercialReportContract {
    schema: String,
    version: String,
    generated_at: String,
    status: String,
    release_tag: String,
    summary: CommercialReportSummaryContract,
    checks: Vec<CommercialCheckContract>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommercialReportSummaryContract {
    total_checks: usize,
    satisfied: usize,
    blocking: usize,
    external_blocking: usize,
    local_blocking: usize,
    blocking_by_resolution_scope: CommercialScopeCountsContract,
}
```

**Fail-closed parsing and semantic recheck** (`kiana-commands/src/audit.rs:1101-1141`, `:1243-1271`):

```rust
let report: CommercialReportContract = match serde_json::from_slice(&output.stdout) {
    Ok(report) => report,
    Err(error) => {
        return (
            None,
            vec![block_finding(
                "commercial_report_invalid",
                "商业 blocker 报告不是有效 JSON",
                Some("scripts/commercial-release-blockers-report.sh".to_string()),
                None,
                error.to_string(),
                "修复脚本输出契约后重新审计",
                "release-owner",
            )],
        );
    }
};
let summary = match validate_commercial_report(&report) {
    Ok(summary) => summary,
    Err(error) => {
        return (
            None,
            vec![block_finding(
                "commercial_report_invalid",
                "商业 blocker 报告违反数据契约",
                Some("scripts/commercial-release-blockers-report.sh".to_string()),
                None,
                error,
                "修复 schema、枚举值和 summary/checks 一致性后重新审计",
                "release-owner",
            )],
        );
    }
};
```

```rust
let total_checks = report.checks.len();
let satisfied = total_checks.saturating_sub(blocking);
let local_blocking = blocking.saturating_sub(external_blocking);
let expected_status = if blocking == 0 { "ready" } else { "blocked" };
let summary = &report.summary;
if summary.total_checks != total_checks
    || summary.satisfied != satisfied
    || summary.blocking != blocking
    || summary.external_blocking != external_blocking
    || summary.local_blocking != local_blocking
    || report.status != expected_status
{
    return Err("summary counts or report status do not match checks".to_string());
}
```

The command must not trust stored summaries or promote governance completion into product completion. Invalid compiler output becomes a blocking finding, never a partial success.

**Machine/human dual output** (`kiana-commands/src/audit.rs:280-283`):

```rust
if args.json_output {
    return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
}
Ok(CommandResult::text(format_audit_report(&report)))
```

**Stable command errors** (`kiana-commands/src/audit.rs:1513-1523`): use machine-readable prefixes such as `workflow_not_found:`; the governance adapter should preserve semantic validator prefixes in JSON and human output.

**Integration test pattern** (`kiana-commands/tests/audit_command.rs:244-255`):

```rust
let result = AuditCommand
    .execute(context("strict --json", &root))
    .await
    .unwrap();
let report: Value = serde_json::from_str(&result.value).unwrap();
assert_eq!(report["status"], "blocked");
assert_eq!(report["blocking_count"], 2);
assert_eq!(report["commercial"]["local_blocking"], 1);
assert_eq!(report["commercial"]["external_blocking"], 1);
assert_eq!(
    report["commercial"]["schema"],
    "kiana.commercial-release-blockers.v1"
);
```

The test's fixture construction is at `kiana-commands/tests/audit_command.rs:168-242`. The adjacent negative test at `:260-294` loops over empty and count-mismatched payloads and asserts `commercial_report_invalid`; mirror that for invalid governance summaries if an adapter is added.

## Shared Patterns

### Validation Pipeline

All consumers use one ordered path:

```text
UTF-8 JSON parse
  -> existing structural schema validator
  -> Phase 1 semantic graph validator
  -> deterministic in-memory render
  -> exact-byte write/check
  -> optional command adapter
```

No generator, Markdown view, or command adapter may bypass the semantic validator.

### Central Completion Predicate

Compute, do not store as a second truth:

```text
counts_as_complete(subject) =
  coverage_state == verified
  AND freshness == current
  AND proof_rank(proof_level) >= proof_rank(required_proof_level)
  AND current evidence matches subject + source/target revision
```

Required journey children aggregate with `all`, not averages. Adopt/Adapt/Reject completeness remains independent from Kiana implementation/proof completeness.

### Error Handling

- CLI usage/read failures: exit 2.
- Structural or semantic invalidity: exit 1 with stable code prefix and affected IDs.
- Generated view drift: exit 1 with `generated_view_drift:<repo-relative-path>`.
- Invalid optional adapter input: blocking finding; no optimistic fallback.
- Collect deterministic errors where useful, sort by `(code, subject_id, path)`, and never leak credentials, absolute home paths, or raw unbounded evidence.

### File and Path Guards

- Accept repo-relative paths or explicitly allowlisted external URIs only.
- Reject absolute paths, `..`, unsupported URI schemes, symlink escape on opened files, malformed UTF-8/JSON, and files above a documented size bound.
- Hash canonical bytes with SHA-256; record revision kind separately from stable identity.
- Snapshot refresh creates a new snapshot/diff. It never overwrites history.

### Deterministic Rendering

- Validate first.
- Sort every list explicitly by stable IDs and documented secondary keys; do not depend on filesystem or dict iteration.
- Use snapshot/review timestamps, never current wall-clock time.
- Emit UTF-8, LF, one trailing newline, generated/do-not-edit marker, canonical IDs, snapshot ID, and visible blocker reasons.
- Escape Markdown/HTML/link inputs and omit temporary or absolute paths.
- `--check` renders in memory and performs exact-byte comparison without writes.

### Testing

- Quick gate: `bash scripts/capability-governance-smoke.sh` with named slices.
- Broad contract gate: `bash scripts/schema-contract-smoke.sh`, which invokes the quick gate once.
- One small valid cross-file graph, one full 38-repository reconciliation fixture, and one focused invalid fixture per invariant/threat.
- Test both repeat rendering and hand-edited view drift.
- Record the first green quick-gate duration and keep it below 30 seconds without network or full Cargo execution.
- Add focused Rust command tests only if the optional adapter is implemented.

## No Exact Analog

| Target | Missing Live Pattern | Planner Direction |
|---|---|---|
| `governance/evidence/index.json` | No existing documentation-domain index combines immutable typed evidence, subject/revision binding, proof rank, freshness invalidation, and append-only history across all four families. | Use the closed schema pattern plus locked D-16/D-17/D-21 semantics; do not copy mutable summaries or workflow artifact storage directly. |
| `generate-capability-governance.py --check` behavior | The commercial report has sorted JSON and Markdown rendering but uses wall-clock time and has no exact-byte check mode. | Copy its dual-output/escaping boundary, then implement the stricter research contract described above. |

## Migration Inputs, Not Copy Targets

- `docs/agent-program/kiana-completion/references.json`: import 38 IDs/paths/domains, then generate, retire, or explicitly constrain it to non-authoritative inventory input.
- `docs/reference-feature-matrix.md`, `docs/reference-migration-roadmap.md`, and `docs/reference_audit/*`: preserve provenance while migrating status-bearing rows; do not keep editable duplicate completion fields.
- `docs/commercial-release-readiness.md`: retain narrative proof-boundary guidance, but canonical completion counts must come from the governance compiler.
- Capability-gap implementation, continuous monitoring, target-environment proof, cloud/enterprise work, and user acceptance remain out of Phase 1.

## Metadata

**Analog search scope:** `docs/schemas/`, `docs/agent-program/kiana-completion/`, `scripts/`, `kiana-commands/src/`, `kiana-commands/tests/`

**Five strong analog assignments:**

1. `docs/schemas/kiana-commercial-release-blockers.v1.schema.json`
2. `scripts/validate-json-schema.py`
3. `scripts/commercial-release-blockers-report.sh`
4. `scripts/schema-contract-smoke.sh`
5. `kiana-commands/src/audit.rs` with `kiana-commands/tests/audit_command.rs` as its test companion

**Pattern extraction date:** 2026-07-15
