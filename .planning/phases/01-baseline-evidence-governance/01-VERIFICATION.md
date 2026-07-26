---
phase: 01-baseline-evidence-governance
verified: 2026-07-26T02:31:00Z
status: gaps_found
score: 3/4 must-haves verified
behavior_unverified: 0
overrides_applied: 0
re_verification:
  previous_status: gaps_found
  previous_score: 3/4
  gaps_closed: []
  gaps_remaining:
    - "The final offline production proof completes reliably inside the fixed 30-second feedback contract."
  regressions: []
gaps:
  - truth: "The final offline production proof completes reliably inside the fixed 30-second feedback contract."
    status: failed
    reason: "linux_production_slice_has_fresh_process_headroom executed two fresh-process production samples; sample 1 recorded Rust elapsed_seconds=25.700 which exceeds the required <25s ceiling. The single-load optimization (Plan 01-13) reduced worst-case from 30.29s (deadline_exceeded) to 25.7s but did not achieve the required >=5s headroom with two consecutive independent samples on the verification machine."
    artifacts:
      - path: kiana-capability-governance-supervisor/tests/supervisor_linux.rs
        issue: "Test linux_production_slice_has_fresh_process_headroom FAILED: sample 1 elapsed_seconds=25.700 >= 25.0s ceiling (sample 0 passed at elapsed_seconds=24.732). Both samples succeeded and met sandbox/network/cleanup conditions; timing alone is the failure."
      - path: scripts/generate-capability-governance.py
        issue: "verify-production command, despite single-load consolidation, still takes >25s on cold fresh-process runs on the verification machine (shirosora Linux 6.8.0-134-generic)."
    missing:
      - "Further reduction of cold-path production work to bring both independent fresh-process samples reliably below 25 seconds."
      - "Alternatively, document the machine spec and measured elapsed values from the environment where the 01-13 executor observed two passing samples, and confirm the regression gate is tied to that hardware class."
---

# Phase 01: Baseline Evidence Governance Verification Report (Re-verification)

**Phase Goal:** 用户和维护者可以用冻结日期、来源和证据判断公开能力与 reference 覆盖，而不是依赖功能数量或乐观描述。
**Verified:** 2026-07-26T02:31:00Z
**Status:** gaps_found
**Re-verification:** Yes — after Plan 01-13 gap-closure attempt

## Step 0: Re-verification Mode

Previous VERIFICATION.md (2026-07-22T17:39:39Z): `gaps_found`, score 3/4.
Gap CR-01: production proof timing-sensitive (deadline_exceeded at 30.29s, later warm pass at 24.09s).
Plan 01-13 claimed to resolve CR-01 via single-load verifier and fresh-process headroom regression.

Failed item receives full 3-level verification. Truths 1-3 receive regression check only.

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | A frozen-date Claude Code public-parity ledger gives every public journey a governed result or explicit difference. | VERIFIED | Unchanged since 2026-07-22. Selected official-source and public-baseline ancestry validates; generated public parity wired from `current.json`; Plan 01-13 did not modify governance data. |
| 2 | All 38 references carry live source, license, decision, owner, test, risk, evidence, and reject rationale. | VERIFIED | Unchanged since 2026-07-22. 38/38 registry and decision coverage confirmed; Plan 01-13 did not modify reference data. |
| 3 | Governance output distinguishes source, local, target, and user-value proof instead of treating modules, stubs, mocks, or test counts as completion. | VERIFIED | Unchanged since 2026-07-22. Evidence schema and generated views retain proof-level fields; Plan 01-13 did not change proof semantics. |
| 4 | The final offline production proof completes reliably inside the fixed 30-second feedback contract. | FAILED | `linux_production_slice_has_fresh_process_headroom` ran and failed: sample 0 elapsed_seconds=24.732 (pass), sample 1 elapsed_seconds=25.700 (FAIL — exceeds <25s ceiling). Both samples succeeded offline with no network trace and clean runtime roots. Timing is the sole failure. See Behavioral Spot-Checks. |

**Score:** 3/4 truths verified

### Required Artifacts (CR-01 Focus — Level 1-3 re-check)

| Artifact | Exists | Substantive | Wired | Status |
|----------|--------|-------------|-------|--------|
| `scripts/capability_governance.py` — PRODUCTION_REPORT_SCHEMA / VERSION / CHECK_IDS / validate_production_report | Yes | Yes — 14-ID tuple, strict validator (lines 31-250) | Yes — imported by generate-capability-governance.py and smoke.sh consumer | VERIFIED |
| `scripts/generate-capability-governance.py` — verify-production single-load command | Yes | Yes — `load_validated_manifest_bundle` called once at line 467; all 14 checks derive from one bundle; self-validates before writing | Yes — invoked by smoke.sh run_production_slice | VERIFIED |
| `kiana-capability-governance-supervisor/tests/supervisor_linux.rs` — linux_production_slice_has_fresh_process_headroom | Yes | Yes — 157 lines; asserts SLICE_DEADLINE=30s, two independent run_binary("production") calls, rust_elapsed_seconds < 25.0 per sample | Yes — part of supervisor test suite | VERIFIED (substantive + wired) |
| `scripts/capability-governance-smoke.sh` — shared-validator wiring | Yes | Yes — run_production_slice calls validate_production_report via isolated Python (lines 1526-1551); no Bash-side ID list duplication; 30s elapsed guard retained | Yes — production slice executes one-pass verifier then shared validator | VERIFIED |

### Key Link Verification (CR-01 Focus)

| From | To | Via | Status |
|------|-----|-----|--------|
| `scripts/capability-governance-smoke.sh` | `scripts/capability_governance.py` | `run_production_slice` calls `governance.validate_production_report(report, manifest_sha256=..., evaluation_time=...)` via `run_python - "$production_report" "$manifest"` (lines 1526-1551) | WIRED |
| `kiana-capability-governance-supervisor/tests/supervisor_linux.rs` | `target/debug/kiana-capability-governance-supervisor` | Two independent `run_binary("production")` calls parse Rust-emitted `elapsed_seconds` and require both < 25.0 | WIRED |

### Behavioral Spot-Checks (Re-verification)

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Fresh-process headroom regression (CR-01 gate) | `cargo test -p kiana-capability-governance-supervisor --test supervisor_linux linux_production_slice_has_fresh_process_headroom --locked --offline -- --exact --test-threads=1` | FAILED in 50.67s. sample=0 elapsed_seconds=24.732 (pass). sample=1 elapsed_seconds=25.700 >= 25.0s ceiling. Both samples succeeded offline, empty stderr, no network trace, clean runtime roots. | FAIL |

**Raw test output (sample 1 failure):**
```
sample=1 exit_status=Some(0) succeeded=true wall_elapsed_seconds=25.788
rust_elapsed_seconds=Some(25.7) elapsed_parse_error=None
runtime_roots_before={} runtime_roots_after={}
stdout:
OK: capability governance corpus cases=2 valid_fixtures=0 protected_inputs=12
OK: production governance report, drift, evidence, ancestry, and generated bytes pass
OK: slice=production elapsed_seconds=25.700 offline=true
```

### Plan 01-13 Structural Work — What IS Resolved

Plan 01-13's structural changes are correctly implemented. The following gaps from the earlier diagnosis are addressed in code:

| Item | Was Missing | Now Present | Verified |
|------|------------|-------------|---------|
| Closed report schema | No | `PRODUCTION_REPORT_SCHEMA`, `PRODUCTION_REPORT_VERSION`, `PRODUCTION_CHECK_IDS` in `capability_governance.py` lines 31-48 | Yes |
| Single-load verifier | No | `load_validated_manifest_bundle` called once at `generate-capability-governance.py:467`; test double in `test_verify_production_loads_the_complete_bundle_once` asserts call count = 1 | Yes |
| Bash consumer uses shared validator | No | `run_production_slice` calls `validate_production_report` via isolated Python (lines 1526-1551); no Bash ID list duplication | Yes |
| Headroom regression test exists | No | `linux_production_slice_has_fresh_process_headroom` present and wired; enforces two samples < 25s + SLICE_DEADLINE = 30s unchanged | Yes |

What remains unresolved: the test enforcing the headroom threshold itself does not pass on the verification machine. The optimized path is marginally faster (~25.7s) but not reliably within the 25-second ceiling.

### Requirements Coverage

| Requirement | Description | Status | Evidence |
|-------------|-------------|--------|----------|
| COD-01 | Frozen-date public-parity ledger with governed result or explicit difference per journey | SATISFIED (implementation) | Frozen public ledger, exclusive source mapping, immutable evidence, generated view, and explicit differences exist. Phase closure blocked by production proof timing gap. |
| DIF-11 | 38 references with Adopt/Adapt/Reject, license, security, owner, test, evidence auditable | SATISFIED (implementation) | 38/38 registry and decision coverage confirmed. Phase closure blocked by production proof timing gap. |

Both COD-01 and DIF-11 are mapped to Phase 1 in REQUIREMENTS.md traceability table. No orphaned requirements found.

### Anti-Patterns Found

No unreferenced `TBD`, `FIXME`, or `XXX` markers found in Plan 01-13 modified files. The production path's behavioral contract is structurally sound; the only issue is execution time on the verification machine.

### Human Verification Required

None at this stage. The automated gap (Must-Have 4) must close first before UAT items are opened:
- Maintainer judgment on 49 capability decompositions (01-08 D3) — deferred until automated gap resolves
- Maintainer judgment on license interpretation (01-09 D3) — deferred until automated gap resolves

### Gaps Summary

Plan 01-13's structural work is correct and fully wired: the single-load verifier, closed 14-check report schema, shared Python validator in the Bash consumer, and headroom regression test all exist as specified. The regression test itself, however, failed on direct execution: sample 0 passed at 24.732s but sample 1 failed at 25.700s, exceeding the < 25-second ceiling by 0.700s.

The root cause is that the production path, while improved by approximately 4.6 seconds (from ~30.3s to ~25.7s), is not yet deterministically within the 5-second headroom budget on the verification machine (Linux 6.8.0-134-generic, shirosora). Whether the two-sample pass observed by the 01-13 executor reflects a faster machine, warmer cache conditions, or a different load state is unknown.

Next gap-closure plan should reduce cold-path production time further OR document the machine specification and timing evidence from the environment where the 01-13 executor observed passing samples, and confirm the regression gate is expected to pass on that hardware class. The 30-second supervisor deadline and the < 25-second headroom ceiling must remain unchanged.

---

_Verified: 2026-07-26T02:31:00Z_
_Verifier: Kiro (gsd-verifier)_
