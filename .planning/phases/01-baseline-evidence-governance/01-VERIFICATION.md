---
phase: 01-baseline-evidence-governance
verified: 2026-07-22T17:39:39Z
status: gaps_found
score: 3/4 must-haves verified
behavior_unverified: 0
overrides_applied: 0
gaps:
  - truth: "The offline production governance gate completes reliably within its fixed 30-second contract."
    status: failed
    reason: "Identical committed inputs produced deadline_exceeded at 30.29s and a 9/10 Rust integration result, then passed warm at 24.09s; the proof is timing-sensitive."
    artifacts:
      - path: scripts/capability-governance-smoke.sh
        issue: "run_production_slice performs more cold-path work than the fixed 30-second budget reliably permits."
      - path: kiana-capability-governance-supervisor/src/lib.rs
        issue: "The fail-closed 30-second supervisor correctly exposes the production slice overrun."
      - path: kiana-capability-governance-supervisor/tests/supervisor_linux.rs
        issue: "linux_public_slices_succeed_after_prebuilt_binary failed once with deadline_exceeded and passed on exact rerun."
    missing:
      - "Reduce and verify cold-path production work so repeated runs have deterministic margin below 30 seconds without weakening the fail-closed deadline."
      - "Add regression evidence that distinguishes a real performance fix from a warm-cache-only pass."
---

# Phase 01: Baseline Evidence Governance Verification Report

**Phase Goal:** 用户和维护者可以用冻结日期、来源和证据判断公开能力与 reference 覆盖，而不是依赖功能数量或乐观描述。
**Verified:** 2026-07-22T17:39:39Z
**Status:** gaps_found
**Re-verification:** No - initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | A frozen-date Claude Code public-parity ledger gives every public journey a governed result or explicit difference. | VERIFIED | The selected official-source and public-baseline ancestry validates; generated public parity is wired from `current.json`; focused public-baseline checks pass. |
| 2 | All 38 references carry live source, license, decision, owner, test, risk, evidence, and reject rationale. | VERIFIED | Registry and decision chains contain 38 unique identities and validate through the reference-governance slice and current broad gate. |
| 3 | Governance output distinguishes source, local, target, and user-value proof instead of treating modules, stubs, mocks, or test counts as completion. | VERIFIED | Evidence schema/history and generated views retain proof-level fields; source-only Phase 01 evidence does not manufacture higher proof levels. |
| 4 | The final offline production proof completes reliably inside the fixed 30-second feedback contract. | FAILED | Direct production failed with `deadline_exceeded` at 30.29s; full Rust integration was 9/10. A later warm broad run passed at 24.09s, demonstrating timing sensitivity rather than stable margin. |

**Score:** 3/4 truths verified

### Required Artifacts

All 44 declared artifacts passed the canonical existence/substance checker.

| Plans | Artifacts | Status | Details |
|-------|-----------|--------|---------|
| 01-01 to 01-04 | 14/14 | VERIFIED | Eight schemas, supervisor authority, production semantic module, CLI, and focused runner are substantive. |
| 01-05 to 01-07 | 12/12 | VERIFIED | Coverage/integrity/drift/security corpora and freeze/check-drift/refresh workflow exist and execute. |
| 01-08 to 01-10 | 12/12 | VERIFIED | Public, reference, decision, evidence, legacy, and selector revision chains exist and validate. |
| 01-11 to 01-12 | 6/6 | VERIFIED | Deterministic generator/views/diffs and the one-line broad-gate integration exist. |

### Key Link Verification

| Link Set | Status | Details |
|----------|--------|---------|
| Canonical machine checks | 18/20 VERIFIED | Imports, CLI-to-worker paths, validator-to-data paths, and supervisor slice wiring matched declared patterns. |
| `freeze-capability-governance.py` to policy-controlled source input | MANUALLY VERIFIED | The CLI consumes already-captured artifacts and imports `urllib.parse.urlsplit`; it has no HTTP client, socket, curl, or wget path. Its subprocess use is limited to local Git revision observation. |
| `schema-contract-smoke.sh` to focused governance runner | MANUALLY VERIFIED | The exact invocation occurs once at line 2223, immediately after commercial handoff smoke and before the final success message. Commit `90efe96` changes exactly that one line. |

### Data-Flow Trace (Level 4)

| Artifact | Source | Output | Status |
|----------|--------|--------|--------|
| Generated public/reference/legacy views | Explicit six-head `current.json` selector plus validated immutable revisions | Deterministic Markdown and compatibility output | VERIFIED |
| Drift report | Controlled official artifact, live reference roots, target root, and frozen fingerprints | Typed current/stale/unavailable report | VERIFIED |
| Broad schema gate | `schema-contract-smoke.sh` final boundary | Focused runner exit status propagates to repository gate | VERIFIED |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Python governance contracts | `python3 -m unittest discover -s scripts/tests -p 'test_*capability_governance*.py' -v` | 41 passed in 9.973s | PASS |
| Rust supervisor unit and integration behavior | `cargo test -p kiana-capability-governance-supervisor --locked --offline` plus serial integration rerun | Unit tests 24/24; integration 9/10 with production deadline failure | FAIL |
| Production proof, cold/slow observation | `target/debug/kiana-capability-governance-supervisor production` | `deadline_exceeded`, exit 1, 30.29s | FAIL |
| Broad repository contract | `bash scripts/schema-contract-smoke.sh` | All slices offline; production 24.09s; broad gate passed | PASS |
| Exact failed Rust test rerun | `cargo test ... linux_public_slices_succeed_after_prebuilt_binary -- --exact --test-threads=1` | Passed in 63.97s | PASS, FLAKY |
| Rust formatting | `cargo fmt --all --check` | Exit 0 | PASS |
| Python compilation | `python3 -m py_compile` over governance modules and tests | Exit 0 | PASS |

### Probe Execution

| Probe | Result | Status |
|-------|--------|--------|
| Focused `production` slice | Failed at 30.29s, later passed warm at 24.09s through broad gate | FAILED - timing-sensitive |
| Broad schema-contract smoke | Exit 0 and propagated every focused slice | PASS |

### Requirements Coverage

| Requirement | Source Plans | Status | Evidence |
|-------------|--------------|--------|----------|
| COD-01 | 01-01 through 01-12 | SATISFIED (implementation) | Frozen public ledger, exclusive source mapping, immutable evidence, generated view, and explicit differences exist; phase closure remains blocked by the production proof gap. |
| DIF-11 | 01-01 through 01-12 | SATISFIED (implementation) | 38/38 registry and decision coverage with license/security/owner/test/risk/evidence and reject rationale exists; phase closure remains blocked by the production proof gap. |

### Anti-Patterns Found

No unreferenced `TBD`, `FIXME`, `XXX`, placeholder, or obvious stub marker was found in Phase 01 source/governance files. Code review finding CR-01 is the only blocking issue.

### Human Verification Required After Gap Closure

1. Maintainer judgment on whether the 49 capability decompositions faithfully interpret the captured official public material (`01-08` D3).
2. Maintainer judgment on source-specific license interpretation and borrowing rationale (`01-09` D3).

These are not auto-approved. UAT is intentionally not opened while an automated production gap remains.

### Gaps Summary

The governed data, artifact inventory, link wiring, and broad-gate connection are present. Phase 01 cannot close because the same production proof alternated between fail-closed timeout and warm success under identical committed inputs. Gap planning must restore deterministic cold-path margin without weakening the 30-second fail-closed contract. After that gap closes, verifier rerun should route the two maintainer judgments to UAT.

---

_Verified: 2026-07-22T17:39:39Z_
_Verifier: Codex (inline gsd-verifier fallback)_
