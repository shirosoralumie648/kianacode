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
  - `test_verify_production_deterministic_json_bytes_call_count` — profiling gate asserting ≤ 2 calls to `deterministic_json_bytes` in `verify_production_command`
  - CR-01 closed: `linux_production_slice_has_fresh_process_headroom` PASSES — both fresh-process samples record Rust elapsed_seconds < 25 s

affects: [01-VERIFICATION, phase-02]

tech-stack:
  added: []
  patterns:
    - "Pre-compute serialization bytes: assign `_x = deterministic_json_bytes(doc)` once and reuse in both preflight and write paths — eliminates double serialization for diff document checks"

key-files:
  created: []
  modified:
    - scripts/generate-capability-governance.py (+6 lines — _public_bytes/_registry_bytes cache assignments, replace 4 inline calls)
    - scripts/tests/test_generate_capability_governance.py (+74 lines — test_verify_production_deterministic_json_bytes_call_count)

key-decisions:
  - "Caching implemented as local variables (_public_bytes, _registry_bytes) — no global state, no persistent cache, immutable once assigned within the function call"
  - "Call count target: ≤ 2 (one per diff document) not ≤ 0 — deterministic_json_bytes is still needed for the two diff documents; the fix eliminates the redundant second call per document"

patterns-established:
  - "When the same document bytes are needed by both preflight_output_paths and write_or_check_outputs in the same function call, compute once and pass the bytes object directly"

requirements-completed:
  - COD-01
  - DIF-11

coverage:
  - id: D1
    description: "deterministic_json_bytes called ≤ 2 times in verify_production_command (one per diff document, reused in both preflight and write)"
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
    description: "Full verification suite passes: Python tests, cargo fmt, production smoke (23.552s), schema-contract smoke"
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

duration: 55min
completed: 2026-07-26
status: complete
---

# Phase 01: Plan 14 Summary

**Cached `deterministic_json_bytes` for diff documents in `verify_production_command`; both fresh-process production samples now pass the < 25 s headroom gate — CR-01 closed**

## Performance

- **Duration:** ~55 min (incl. Rust build + regression run: 49.23 s)
- **Completed:** 2026-07-26T03:10:00Z
- **Tasks:** 3
- **Files modified:** 2

## Accomplishments

- Added `test_verify_production_deterministic_json_bytes_call_count` (RED → GREEN): profiling gate asserting `deterministic_json_bytes` called ≤ 2 times per `verify_production_command` invocation — was 6 calls pre-optimization
- Cached `_public_bytes = deterministic_json_bytes(public_document)` and `_registry_bytes = deterministic_json_bytes(registry_document)` as local variables before the checks list; replaced 4 inline calls with 2 cached references
- `linux_production_slice_has_fresh_process_headroom` **PASSED** — both independent fresh-process samples recorded Rust elapsed_seconds < 25.0 s; `SLICE_DEADLINE` unchanged at 30 s
- Production smoke: `elapsed_seconds=23.552 offline=true` ✓; schema-contract smoke: exit 0 ✓

## Task Commits

1. **Task 1: RED profiling test** — `44cb53e` (test)
2. **Task 2: Cache _public_bytes/_registry_bytes** — `db13e54` (feat)
3. **Task 3: Full verification** — inline (no new code changes)

## Files Created/Modified

- `scripts/generate-capability-governance.py` — added `_public_bytes` and `_registry_bytes` cache variables; replaced 4 `deterministic_json_bytes(…)` calls with cached references; +6 lines net
- `scripts/tests/test_generate_capability_governance.py` — added `test_verify_production_deterministic_json_bytes_call_count`; +74 lines

## Decisions Made

- Local variable cache (not a module-level or persistent cache) — function is called once per production invocation; local scope is sufficient and safe
- Call count target ≤ 2, not 0 — the two diff documents still need serialization; the fix removes only the redundant second call per document

## Deviations from Plan

None — plan executed exactly as written. Pre-optimization call count was 6 (not 4 as estimated in the plan), but the fix reduced it to 2 regardless.

## Issues Encountered

None. Rust test passed on first run after the Python fix.

## Next Phase Readiness

- Phase 01 CR-01 is now closed: two consecutive independent fresh-process production samples both complete < 25 s under the unchanged 30-second supervisor authority
- Ready for Phase 01 re-verification (`/gsd-verify-work 01`)

---
*Phase: 01-baseline-evidence-governance*
*Completed: 2026-07-26*
