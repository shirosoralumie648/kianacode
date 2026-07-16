# Plan 01-06 Production Contract Correction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Correct the production capability-governance validator so the already-approved Plan 01-06 post-implementation corpus can assert every exact ancestry, freshness, drift, security, alias, and summary diagnostic without weakening its oracle.

**Architecture:** Keep `scripts/capability_governance.py` as the only semantic authority and `scripts/validate-capability-governance.py` as a thin renderer/exit adapter. Add standard-library `unittest` contract tests that derive mutations in memory or in temporary directories; these are implementation-correction tests, not Plan 01-06 corpus assets. Preserve the valid fixtures and the `0=valid/current`, `1=invalid/stale`, `2=read-or-usage failure` CLI contract.

**Tech Stack:** Python 3 standard library (`unittest`, `copy`, `tempfile`, `subprocess`), existing JSON Schema helper, Bash smoke scripts, Rust 2021 supervisor tests.

---

## Scope And File Responsibilities

- Create `scripts/tests/test_capability_governance.py`: real production-module/CLI contract tests; temporary inputs only, no committed negative corpus.
- Modify `scripts/capability_governance.py`: exact semantic diagnostics, evidence freshness classification, immutable-history classification, drift classification, alias validation, and symlink containment.
- Modify `scripts/validate-capability-governance.py`: preserve typed usage diagnostics and exact exit status 2.
- Do not modify `.planning/phases/01-baseline-evidence-governance/01-06-PLAN.md`, any protected valid fixture, schema, or `scripts/validate-json-schema.py`.
- Defer `semantic-negative` shell/Rust allowlisting and all `invalid/integrity` or `invalid/drift` assets to the original Plan 01-06 tasks after this correction is green.

## Frozen Diagnostic Contract

| Condition | Exact code | Exit |
|---|---|---:|
| Genesis has no non-empty `genesis_reason` | `genesis_reason_required` | 1 |
| Successor omits any predecessor field or names no predecessor | `previous_revision_required` | 1 |
| Successor predecessor hash differs from canonical predecessor | `previous_revision_hash_mismatch` | 1 |
| Prior evidence IDs disappear | `history_removal` | 1 |
| Prior evidence IDs remain but relative order changes | `history_reorder` | 1 |
| Prior evidence ID order is stable but prior record content changes | `history_rewrite` | 1 |
| Reject decision has no non-empty rationale | `reject_rationale_required` | 1 |
| Verified capability has no evidence IDs | `verified_evidence_required` | 1 |
| Latest matching passing evidence is expired at manifest evaluation time | `evidence_expired` | 1 |
| Latest matching evidence is a failed retest after an older pass | `newer_failed_retest` | 1 |
| Registry `expected_count` differs from actual repository count | `summary_mismatch` | 1 |
| Git HEAD/tree/license, content tree, official source, target, or unavailable source drifts | Plan 01-06 exact drift code | 1 |
| Secret-shaped input or invalid alias contract | `sensitive_value` / `alias_contract_invalid` | 1 |
| File traversal encoded in governed data | `unsafe_path` | 1 |
| A resolved filesystem binding escapes through a symlink | `symlink_escape` | 2 |
| A bounded read exceeds `MAX_JSON_BYTES` | `oversized_input` | 2 |

Plan 01-06 manifests must therefore support optional `expected_exit`; cases without it default to 1. This reconciles the corpus task's broad "exit 1" wording with the earlier, still-authoritative read/usage exit-2 contract.

### Task 1: Add Exact Semantic And Ancestry Contract Tests

**Files:**
- Create: `scripts/tests/test_capability_governance.py`
- Test: `scripts/tests/test_capability_governance.py`

- [ ] **Step 1: Create a production-module test harness**

Load `scripts/capability_governance.py` from its exact path, deep-copy `minimal-graph.json`, and expose helpers that recompute `record_sha256`, successor hashes, and bundle-manifest head hashes after an in-memory mutation. Use real production functions; do not mock validation.

```python
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import capability_governance as governance

def load_minimal_bundle() -> dict[str, object]:
    return json.loads((ROOT / "scripts/fixtures/capability-governance/valid/minimal-graph.json").read_text())

def codes(errors: list[governance.GovernanceError]) -> set[str]:
    return {error.code for error in errors}
```

- [ ] **Step 2: Write ancestry and semantic RED tests**

Add one assertion per exact contract: genesis reason, predecessor presence/hash, removal/reorder/rewrite, reject rationale, verified evidence, alias contract, sensitive value, and summary mismatch. Each test changes one invariant and asserts the Plan 01-06 code is present.

```python
def test_genesis_without_reason_has_exact_code(self) -> None:
    revisions = copy.deepcopy(self.bundle["evidence_revisions"])
    revisions[0].pop("genesis_reason")
    self.assertIn(
        "genesis_reason_required",
        codes(governance.validate_revision_ancestry(revisions, family="evidence_revisions")),
    )
```

- [ ] **Step 3: Run the focused suite and record RED**

Run:

```bash
python3 -m unittest -v scripts.tests.test_capability_governance
```

Expected: failures name the current production codes (`genesis_ancestry`, `successor_ancestry`, `previous_revision_sha256`, `reject_reason_required`, `verified_without_evidence`, `alias_shape`, `secret_value`, or `repository_count`) or show `history_reorder` is absent. No failure may be a test import/setup error.

- [ ] **Step 4: Commit RED tests only**

```bash
git add -- scripts/tests/test_capability_governance.py
git commit -m "test(01-06): lock governance correction contract"
```

### Task 2: Correct Semantic, History, And Freshness Diagnostics

**Files:**
- Modify: `scripts/capability_governance.py`
- Modify: `scripts/tests/test_capability_governance.py`
- Test: `scripts/tests/test_capability_governance.py`

- [ ] **Step 1: Implement exact ancestry/history classification**

Replace aggregate ancestry aliases with exact codes. Classify an evidence successor by stable evidence IDs before comparing record content:

```python
old_ids = [record.get("evidence_id") for record in old_records]
new_prefix = new_records[: len(old_records)]
new_ids = [record.get("evidence_id") for record in new_prefix]
if not set(old_ids).issubset(record.get("evidence_id") for record in new_records):
    _error(errors, "history_removal", revision_id, records_path)
elif new_ids != old_ids:
    _error(errors, "history_reorder", revision_id, records_path)
elif new_prefix != old_records:
    _error(errors, "history_rewrite", revision_id, records_path)
```

Do not special-case fixture paths, case IDs, or filenames.

- [ ] **Step 2: Implement evidence-failure classification**

For a verified capability, select the highest-sequence evidence record and inspect matching older records. Emit the subject capability ID with `freshness="stale"`:

```python
if latest.get("result") != "pass" and any(row.get("result") == "pass" for row in earlier):
    _error(errors, "newer_failed_retest", capability_id, capability_path, freshness="stale")
elif expires_at is not None and expires_at <= evaluation_time:
    _error(errors, "evidence_expired", capability_id, capability_path, freshness="stale")
elif not counts_as_complete(capability, evidence_records, evaluation_time):
    _error(errors, "capability_not_complete", capability_id, capability_path, freshness="stale")
```

The historical success remains in the immutable record list.

- [ ] **Step 3: Correct decision, evidence, alias, secret, and summary codes**

Use Plan 01-06 public names directly. Alias validation must check exact keys plus allowed `kind` and `change_kind` values; extra compatibility/proof keys are invalid. Define `summary_mismatch` only as `expected_count != len(repositories)` so it does not become product-completion authority.

- [ ] **Step 4: Run GREEN and commit the production correction**

Run:

```bash
python3 -m unittest -v scripts.tests.test_capability_governance
```

Expected: semantic/ancestry tests pass; drift and usage tests added in Task 3 may not exist yet.

Commit:

```bash
git add -- scripts/capability_governance.py
git commit -m "fix(01-06): align governance semantic diagnostics"
```

### Task 3: Correct Drift, Containment, And Usage Diagnostics

**Files:**
- Modify: `scripts/capability_governance.py`
- Modify: `scripts/validate-capability-governance.py`
- Modify: `scripts/tests/test_capability_governance.py`
- Test: `scripts/tests/test_capability_governance.py`

- [ ] **Step 1: Add real temporary-repository RED tests**

Use `tempfile.TemporaryDirectory` and real `git init/add/commit` commands. Test each drift code independently and create real symlink-escape and oversized JSON inputs. CLI tests must assert `expected_exit=2` for filesystem usage failures and parse stdout as JSON.

```python
with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    outside = root.parent / f"{root.name}-outside.json"
    outside.write_text("{}")
    (root / "escape.json").symlink_to(outside)
    with self.assertRaisesRegex(governance.GovernanceUsageError, "^symlink_escape:"):
        governance.resolve_repository_path(root, "escape.json")
```

- [ ] **Step 2: Run the focused suite and record RED**

Run:

```bash
python3 -m unittest -v scripts.tests.test_capability_governance
```

Expected: current aliases (`repository_license_drift`, `repository_tree_drift`, `official_source_drift`, `repository_unavailable`) and generic `read_or_usage_failure` cause the new assertions to fail.

- [ ] **Step 3: Implement exact drift and usage mapping**

Emit `license_hash_drift` for license hash changes, `content_tree_drift` for a `content_tree_sha256` repository, `official_source_hash_drift` for controlled artifact identity/hash changes, and `source_unavailable` for an unavailable reference source. Keep Git repository tree changes as `repository_tree_drift` and target changes as `target_revision_drift`.

Preserve typed usage codes in CLI JSON rather than wrapping all failures:

```python
def _usage_report(exc: GovernanceUsageError) -> dict[str, object]:
    raw_code, separator, detail = str(exc).partition(":")
    code = {
        "input_too_large": "oversized_input",
    }.get(raw_code, raw_code if separator else "read_or_usage_failure")
    return validation_report("error", [GovernanceError(code, detail=detail.strip())])
```

`resolve_repository_path` must distinguish lexical traversal (`unsafe_path`) from a valid relative path whose resolved target escapes (`symlink_escape`).

- [ ] **Step 4: Run GREEN and commit**

```bash
python3 -m unittest -v scripts.tests.test_capability_governance
git add -- scripts/capability_governance.py scripts/validate-capability-governance.py scripts/tests/test_capability_governance.py
git commit -m "fix(01-06): align drift and usage diagnostics"
```

### Task 4: Re-Prove Positive And Supervisor Behavior

**Files:**
- Verify only: `scripts/capability_governance.py`
- Verify only: `scripts/validate-capability-governance.py`
- Verify only: `scripts/capability-governance-smoke.sh`
- Verify only: `kiana-capability-governance-supervisor/`

- [ ] **Step 1: Run syntax and focused tests**

```bash
python3 -m py_compile scripts/capability_governance.py scripts/validate-capability-governance.py scripts/tests/test_capability_governance.py
python3 -m unittest -v scripts.tests.test_capability_governance
```

- [ ] **Step 2: Revalidate all four protected valid fixtures**

```bash
for fixture in scripts/fixtures/capability-governance/valid/{minimal-graph,full-38-repositories,offline-source-identity,hostile-rendering}.json; do
  python3 scripts/validate-capability-governance.py validate --fixture-bundle "$fixture" --json
done
```

Expected: each command exits 0 with `status=valid`.

- [ ] **Step 3: Re-run positive supervised slices serially**

```bash
bash scripts/capability-governance-smoke.sh public-baseline
bash scripts/capability-governance-smoke.sh reference-governance
cargo test -p kiana-capability-governance-supervisor --locked --offline --no-fail-fast -- --test-threads=1
```

Expected: both slices and the complete supervisor crate pass without deadline contention.

- [ ] **Step 4: Verify protected bytes and scoped diff hygiene**

Compare the protected hashes recorded by Plan 01-04, then run:

```bash
git diff --check -- scripts/capability_governance.py scripts/validate-capability-governance.py scripts/tests/test_capability_governance.py
```

- [ ] **Step 5: Commit correction verification metadata only if needed**

No code commit is required when verification creates no tracked changes. After this gate passes, resume the original `01-06-PLAN.md` Tasks 1-4 and author the post-implementation corpus for the first time.

## Self-Review

- Spec coverage: every mismatched code found during Plan 01-06 preflight is mapped to a production change or an explicit exit-contract reconciliation.
- Placeholder scan: every code-changing step includes concrete behavior, commands, and expected results.
- Type consistency: all tests consume `list[GovernanceError]`, use existing `validation_report`, and preserve the current CLI JSON schema.
- Scope: no schema, protected valid fixture, dirty Plan 01-06 file, or unrelated product code is modified.
