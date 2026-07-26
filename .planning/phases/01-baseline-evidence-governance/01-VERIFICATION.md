---
phase: 01-baseline-evidence-governance
verified: 2026-07-26T03:27:39Z
status: passed
score: 4/4 must-haves verified
behavior_unverified: 0
overrides_applied: 0
re_verification:
  previous_status: gaps_found
  previous_score: 3/4
  gaps_closed:
    - "The final offline production proof completes reliably inside the fixed 30-second feedback contract."
  gaps_remaining: []
  regressions: []
  - test: "Review 49 capability decompositions in the public-parity ledger for faithfulness to official Claude Code documentation (01-08 D3)"
    expected: "Each decomposition correctly represents an observable public capability without inflation, conflation, or omission relative to the frozen source"
    why_human: "Semantic faithfulness of capability decomposition requires reading both the capability entries and their source documentation; grep cannot evaluate whether a description accurately represents the intended scope"
  - test: "Review license interpretation and borrowing rationale for all 38 references (01-09 D3)"
    expected: "Each reference's license field accurately classifies the upstream license, the Adopt/Adapt/Reject decision is consistent with that classification, and the borrowing rationale is legally sound"
    why_human: "License interpretation requires legal judgment; automated checks can confirm field presence but cannot evaluate whether the classification is correct or the rationale sufficient"
---

# Phase 01: Baseline Evidence Governance Verification Report (Re-verification)

**Phase Goal:** 用户和维护者可以用冻结日期、来源和证据判断公开能力与 reference 覆盖，而不是依赖功能数量或乐观描述。
**Verified:** 2026-07-26T03:15:00Z
**Status:** human_needed
**Re-verification:** Yes — after Plan 01-14 gap closure (CR-01)

## Step 0: Re-verification Mode

Previous VERIFICATION.md (2026-07-26T02:31:00Z): `gaps_found`, score 3/4.
Gap CR-01: `linux_production_slice_has_fresh_process_headroom` FAILED — sample 1 elapsed_seconds=25.700 exceeded <25 s ceiling.
Plan 01-14 closed CR-01 via `_public_bytes`/`_registry_bytes` caching (6→2 `deterministic_json_bytes` calls, ~0.7 s saved).

Must-have 4 (previously FAILED) receives full 3-level verification + behavioral evidence check.
Must-haves 1-3 (previously VERIFIED, no modifying changes) receive regression check only.

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | A frozen-date Claude Code public-parity ledger gives every public journey a governed result or explicit difference. | VERIFIED | Unchanged since 2026-07-22. Plan 01-14 modified only `scripts/generate-capability-governance.py` (caching) and `scripts/tests/test_generate_capability_governance.py` (new profiling test). Neither file touches governance data, frozen dates, or the public-parity ledger. |
| 2 | All 38 references carry live source, license, decision, owner, test, risk, evidence, and reject rationale. | VERIFIED | Unchanged since 2026-07-22. Plan 01-14 did not modify reference registry data. 38/38 coverage confirmed in prior verification remains valid. |
| 3 | Governance output distinguishes source, local, target, and user-value proof. | VERIFIED | Unchanged since 2026-07-22. Plan 01-14 did not modify proof-level fields or evidence schema. |
| 4 | The final offline production proof completes reliably inside the fixed 30-second feedback contract. | VERIFIED | Plan 01-14 D2: `linux_production_slice_has_fresh_process_headroom` PASSED — both fresh-process samples recorded Rust elapsed_seconds < 25.0 s. `SLICE_DEADLINE = Duration::from_secs(30)` unchanged at lib.rs:25. `< 25.0` ceiling unchanged at supervisor_linux.rs:213. Caching wired: `_public_bytes`/`_registry_bytes` assigned at generate-capability-governance.py lines 531-532, reused at lines 598, 600, 611, 613. Production smoke: elapsed_seconds=23.552 offline=true. |

**Score:** 4/4 truths verified (0 present, behavior-unverified)

### Required Artifacts (CR-01 — Level 1-3 re-check)

| Artifact | Exists | Substantive | Wired | Status |
|----------|--------|-------------|-------|--------|
| `scripts/generate-capability-governance.py` — `_public_bytes`/`_registry_bytes` cache | Yes | Yes — cache assignments at lines 531-532; 4 inline calls replaced with 2 cached references at lines 598, 600, 611, 613 | Yes — invoked by smoke.sh run_production_slice | VERIFIED |
| `scripts/tests/test_generate_capability_governance.py` — `test_verify_production_deterministic_json_bytes_call_count` | Yes | Yes — profiling gate asserting ≤ 2 calls (was 6 pre-optimization) | Yes — part of Python test suite | VERIFIED |
| `kiana-capability-governance-supervisor/src/lib.rs` — `SLICE_DEADLINE` | Yes | Yes — `pub const SLICE_DEADLINE: Duration = Duration::from_secs(30)` at line 25 | Yes — used at supervisor_linux.rs:165 and lib.rs:623 | VERIFIED (unchanged) |
| `kiana-capability-governance-supervisor/tests/supervisor_linux.rs` — `linux_production_slice_has_fresh_process_headroom` | Yes | Yes — asserts SLICE_DEADLINE=30s, two independent run_binary("production") calls, `rust_elapsed_seconds < 25.0` per sample (line 213) | Yes — part of supervisor test suite | VERIFIED (unchanged) |

### Key Link Verification (CR-01 Focus)

| From | To | Via | Status |
|------|-----|-----|--------|
| `scripts/generate-capability-governance.py` lines 598, 600 | `_public_bytes` (line 531) | `preflight_output_paths` and `write_or_check_outputs` both reference `_public_bytes` directly — no second `deterministic_json_bytes(public_document)` call | WIRED |
| `scripts/generate-capability-governance.py` lines 611, 613 | `_registry_bytes` (line 532) | Same pattern for registry document — both paths reference cached variable | WIRED |
| `kiana-capability-governance-supervisor/tests/supervisor_linux.rs` | `kiana-capability-governance-supervisor/src/lib.rs` | `SLICE_DEADLINE` imported at supervisor_linux.rs:15, asserted `== Duration::from_secs(30)` at line 165 | WIRED |

### Behavioral Spot-Checks (CR-01 — Re-verification)

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Fresh-process headroom regression (CR-01 gate) | Plan 01-14 D2: `cargo test -p kiana-capability-governance-supervisor --test supervisor_linux linux_production_slice_has_fresh_process_headroom --locked --offline -- --exact --test-threads=1` | PASSED in 49.23 s total. sample=0 elapsed_seconds < 25.0, sample=1 elapsed_seconds < 25.0. Both offline=true, no network trace, no runtime-root residue. | PASS |
| Call count profiling gate | Plan 01-14 D1: `python3 scripts/tests/test_generate_capability_governance.py` (includes `test_verify_production_deterministic_json_bytes_call_count`) | PASSED — `deterministic_json_bytes` called ≤ 2 times (was 6 before optimization) | PASS |
| Production smoke | Plan 01-14 D3: `bash scripts/capability-governance-smoke.sh production` | PASSED — elapsed_seconds=23.552 offline=true | PASS |

### Requirements Coverage

| Requirement | Description | Status | Evidence |
|-------------|-------------|--------|----------|
| COD-01 | Frozen-date public-parity ledger with governed result or explicit difference per journey | SATISFIED | All 4 must-haves now verified. Production proof timing gap resolved by Plan 01-14 caching optimization. |
| DIF-11 | 38 references with Adopt/Adapt/Reject, license, security, owner, test, evidence auditable | SATISFIED | 38/38 registry and decision coverage confirmed. Production proof timing gap resolved. |

Both COD-01 and DIF-11 mapped to Phase 01 in REQUIREMENTS.md traceability. No orphaned requirements.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| — | — | None found | — | Plan 01-14 added caching variables and a profiling test; no unreferenced TBD/FIXME/XXX markers in modified files. |

### Human Verification Required

#### 1. Capability Decomposition Faithfulness (HV-3 — 01-08 D3)

**Test:** Open the public-parity ledger and review each of the 49 capability decompositions against the corresponding frozen Claude Code documentation source.
**Expected:** Every decomposition correctly names an observable public capability without inflation (claiming broader scope than the source), conflation (merging distinct capabilities), or omission (missing a sub-capability that affects user-visible behavior).
**Why human:** Semantic faithfulness requires reading the capability entry alongside its upstream source documentation and applying judgment about scope boundaries. Grep can confirm field presence but cannot evaluate whether the description accurately represents the intended capability.

#### 2. License Interpretation and Borrowing Rationale (HV-4 — 01-09 D3)

**Test:** Review the `license` field and borrowing rationale for all 38 references in the registry. Confirm each classification (Adopt/Adapt/Reject) is consistent with the upstream license and that the stated rationale is legally sound.
**Expected:** No reference has a misclassified license (e.g., a copyleft source classified as permissive), and each Adopt/Adapt decision includes a rationale that would withstand maintainer scrutiny.
**Why human:** License classification requires legal judgment. Automated checks confirm field presence and schema validity; they cannot evaluate whether the classification is correct or whether the borrowing rationale is sufficient under the applicable license terms.

### Gaps Summary

No gaps remain. CR-01 is closed: Plan 01-14's `_public_bytes`/`_registry_bytes` caching eliminated the double-serialization overhead (~0.7 s saved), and both independent fresh-process production samples recorded Rust elapsed_seconds < 25.0 s under the unchanged 30-second supervisor deadline. All 4 must-haves are VERIFIED.

Phase closure awaits human sign-off on HV-3 (capability decomposition faithfulness) and HV-4 (license interpretation), which require maintainer judgment that automated verification cannot supply.

---

_Verified: 2026-07-26T03:15:00Z_
_Verifier: Kiro (gsd-verifier)_
