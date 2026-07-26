---
phase: 01-baseline-evidence-governance
plan: "14"
subsystem: testing
tags: [governance, production-verification, python, capability-governance, performance]

requires:
  - phase: 01-13
    provides: Single-load production verifier with closed 14-check report schema and fresh-process regression test

provides:
  - Cached `_public_bytes` and `_registry_bytes` in `verify_production_command` — eliminates double `deterministic_json_bytes` serialization per diff document
  - Early corpus `subprocess.Popen` before `load_validated_manifest_bundle` — overlaps corpus Python cold-start with the dominant ~12 s bundle validation hot spot
  - `test_verify_production_deterministic_json_bytes_call_count` — profiling gate asserting ≤ 4 calls to `deterministic_json_bytes` in `verify_production_command`
  - CR-01 closed: `linux_production_slice_has_fresh_process_headroom` PASSES — both fresh-process samples record Rust elapsed_seconds < 25 s

affects: [01-VERIFICATION, phase-02]

tech-stack:
  added: []
  patterns:
    - "Pre-compute serialization bytes: assign `_x = deterministic_json_bytes(doc)` once and reuse in both preflight and write paths — eliminates double serialization for diff document checks"
    - "Corpus parallelism: start corpus subprocess.Popen before load_validated_manifest_bundle so corpus Python cold-start in bwrap overlaps the dominant 12 s hot spot — no new dependency, no interface change"

key-files:
  created: []
  modified:
    - scripts/generate-capability-governance.py (+35 lines — early corpus Popen, _run_production_corpus proc= kwarg, _public_bytes/_registry_bytes cache)
    - scripts/tests/test_generate_capability_governance.py (+68 lines — test_verify_production_deterministic_json_bytes_call_count)

key-decisions:
  - "Caching implemented as local variables (_public_bytes, _registry_bytes) — no global state, no persistent cache, immutable once assigned within the function call"
  - "Corpus parallelism: _run_production_corpus extended with proc= kwarg to accept pre-started Popen; serial fallback path (proc=None) preserved for other callers"
  - "Profiling baseline was 6 calls (not 4 as plan predicted) — _write_json_output calls deterministic_json_bytes twice for drift + production reports; threshold adjusted to 4"

patterns-established:
  - "When the same document bytes are needed by both preflight_output_paths and write_or_check_outputs in the same function call, compute once and pass the bytes object directly"
  - "Independent subprocesses that do not depend on the bundle can be started before load_validated_manifest_bundle to overlap cold-start with the dominant serialization hot spot"

requirements-completed:
  - COD-01
  - DIF-11

coverage:
  - id: D1
    description: "deterministic_json_bytes called ≤ 4 times in verify_production_command (2 cached diff doc assignments + 2 _write_json_output report writes)"
    requirement: COD-01
    verification:
      - kind: unit
        ref: "scripts/tests/test_generate_capability_governance.py#test_verify_production_deterministic_json_bytes_call_count"
        status: pass
    human_judgment: false
  - id: D2
    description: "linux_production_slice_has_fresh_process_headroom PASSES — both fresh-process Rust samples record elapsed_seconds < 25.0; offline=true; no network trace; no runtime-root residue"
    requirement: DIF-11
    verification:
      - kind: integration
        ref: "cargo test -p kiana-capability-governance-supervisor --test supervisor_linux linux_production_slice_has_fresh_process_headroom --locked --offline -- --exact --test-threads=1"
        status: pass
    human_judgment: false
  - id: D3
    description: "Full verification suite passes: Python tests, cargo fmt, production smoke (19.232s), schema-contract smoke (production 22.626s)"
    requirement: COD-01
    verification:
      - kind: unit
        ref: "python3 scripts/tests/test_generate_capability_governance.py"
        status: pass
      - kind: integration
        ref: "bash scripts/capability-governance-smoke.sh production"
        status: pass
      - kind: integration
        ref: "bash scripts/schema-contract-smoke.sh"
        status: pass
    human_judgment: false

duration: 20min
completed: 2026-07-26
status: complete
---

# Phase 01: Plan 14 Summary

**Corpus subprocess parallelism + diff bytes caching close CR-01: both fresh-process production samples reliably below 25 s**

## Performance

- **Duration:** ~20 min
- **Completed:** 2026-07-26
- **Tasks:** 3
- **Files modified:** 2

## Accomplishments

- Added `test_verify_production_deterministic_json_bytes_call_count` (RED → GREEN): profiling gate confirming pre-optimization baseline of 6 calls; target threshold 4 (2 diff doc caches + 2 `_write_json_output` report writes)
- Cached `_public_bytes = deterministic_json_bytes(public_document)` and `_registry_bytes = deterministic_json_bytes(registry_document)` as local variables — call count drops from 6 to 4; Task 1 test turns GREEN
- cProfile revealed that caching alone saved only ~0.2 s (25.700 → 25.502); the corpus subprocess's own Python cold-start inside bwrap (~5–8 s) was the next addressable hot spot
- Added early `subprocess.Popen` for corpus before `load_validated_manifest_bundle` — overlaps the corpus Python process cold-start with the ~12 s JSON-schema bundle validation; extended `_run_production_corpus` with `proc=` keyword for the pre-started `Popen`
- `linux_production_slice_has_fresh_process_headroom` **PASSED** — both independent fresh-process samples recorded Rust elapsed_seconds < 25.0 s (total test time 39.21 s; smoke confirmed 19.232 s and 22.626 s on same machine)

## Task Commits

1. **Task 1: RED profiling test** — `44cb53e` (test)
2. **Task 2: Cache _public_bytes/_registry_bytes** — `db13e54` (feat)
3. **Task 3: Corpus parallelism + regression GREEN** — `024fc58` (feat)

## Files Created/Modified

- `scripts/generate-capability-governance.py` — early corpus Popen, `_run_production_corpus` proc= kwarg, `_public_bytes`/`_registry_bytes` caching; +35/-2 lines net
- `scripts/tests/test_generate_capability_governance.py` — `test_verify_production_deterministic_json_bytes_call_count` added; +68 lines

## Decisions Made

- Local variable cache (not a module-level or persistent cache) — function is called once per production invocation; local scope is sufficient and safe
- Corpus parallelism: `_run_production_corpus` keeps original serial path (proc=None) so it remains usable standalone; `proc=` kwarg enables the parallel path without changing the external contract
- Profiling baseline was 6 (not 4 as plan predicted) because `_write_json_output` calls `deterministic_json_bytes` twice; threshold adjusted to 4

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Corrected call-count threshold from 2 to 4**
- **Found during:** Task 1 RED verification
- **Issue:** Plan predicted 4 baseline calls, actual baseline was 6 — `_write_json_output` (drift_report + production report) adds 2 more calls beyond the 2 diff-doc pairs
- **Fix:** Updated test assertion threshold from `<= 2` to `<= 4`; docstring updated with actual baseline (6) and target (4)
- **Files modified:** `scripts/tests/test_generate_capability_governance.py`
- **Commit:** `44cb53e`

**2. [Rule 3 - Blocking] Corpus parallelism added after caching alone insufficient**
- **Found during:** Task 3 first regression run
- **Issue:** `deterministic_json_bytes` caching saved only ~0.2 s (25.700 → 25.502 s); still fails. cProfile identified corpus subprocess's cold Python start inside bwrap as the next addressable bottleneck within the locked optimization scope
- **Fix:** Early `subprocess.Popen` for corpus before `load_validated_manifest_bundle`; parallel cold-start overlaps the ~12 s bundle validation hot spot; both samples drop to ~19–22 s
- **Files modified:** `scripts/generate-capability-governance.py`
- **Commit:** `024fc58`

## Regression Evidence

| Run | Sample 0 | Sample 1 | Pass/Fail |
|---|---|---|---|
| Before Plan 01-14 (Plan 01-13 result) | 24.732 s | 25.700 s | FAIL (sample 1 >= 25 s) |
| After Task 2 caching only | ~24.1 s | 25.502 s | FAIL (sample 1 >= 25 s) |
| After Task 3 corpus parallelism | < 25.0 s | < 25.0 s | PASS (total 39.21 s; smoke 19.232 s / 22.626 s) |

## Known Stubs

None.

## Threat Flags

None.

## Self-Check: PASSED

- FOUND: `.planning/phases/01-baseline-evidence-governance/01-14-SUMMARY.md`
- FOUND: `44cb53e` (test(01-14): profile verify-production deterministic_json_bytes call count)
- FOUND: `db13e54` (feat(01-14): cache deterministic_json_bytes for diff documents)
- FOUND: `024fc58` (feat(01-14): achieve reliable fresh-process production headroom)

