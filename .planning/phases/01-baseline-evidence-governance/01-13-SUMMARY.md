---
phase: 01-baseline-evidence-governance
plan: "13"
subsystem: testing
tags: [governance, production-verification, rust, python, capability-governance]

requires:
  - phase: 01-12
    provides: Schema-contract smoke integration and aggregate governance invocation path

provides:
  - Closed `kiana.capability-governance-production-report.v1` / `1.0` report schema with immutable 14-check ID tuple
  - Single-load production verifier (`generate-capability-governance.py verify-production`) that loads the canonical bundle once and emits a bounded deterministic JSON report
  - Strict `validate_production_report` helper shared by Python tests and the Bash consumer
  - Linux regression `linux_production_slice_has_fresh_process_headroom` proving two fresh-process production invocations finish ≥5 s below the 30-second supervisor deadline
  - Simplified `capability-governance-smoke.sh production` consumer that delegates all report validation to the shared Python authority

affects: [01-VERIFICATION, phase-02]

tech-stack:
  added: []
  patterns:
    - "Closed-report handoff: producer self-validates a versioned JSON report before writing; Bash consumer calls the same Python validator — no Bash-side ID list duplication"
    - "Single-load production path: `load_validated_manifest_bundle` called exactly once; all 14 subchecks derive from that one validated result"
    - "Fresh-process regression: each sample launches a new supervisor/bwrap/Bash/isolated-Python chain; no in-memory cache can supply a result"

key-files:
  created:
    - scripts/tests/test_generate_capability_governance.py (178-line test suite for closed report contract)
  modified:
    - scripts/capability_governance.py (PRODUCTION_REPORT_SCHEMA / VERSION / CHECK_IDS constants + validate_production_report)
    - scripts/generate-capability-governance.py (single-load verify-production refactor, +468/-29)
    - scripts/capability-governance-smoke.sh (shared-validator wiring, +33/-71)
    - kiana-capability-governance-supervisor/tests/supervisor_linux.rs (fresh-process headroom regression, +157)

key-decisions:
  - "Immutable 14-ID tuple defined in capability_governance.py and shared by both the Python producer and the Bash consumer — no ID duplication across the boundary"
  - "validate_production_report fail-closes on unknown/missing/duplicate/stale/malformed/non-pass data; Bash consumer calls it via isolated Python rather than reimplementing the check list"
  - "Fresh-process regression uses real prebuilt supervisor binary — no mock, no cache flush, no deadline change; each sample is a cold bwrap/Bash/Python chain"
  - "Bash production slice net -38 lines by removing redundant drift/corpus/legacy calls that the one-pass verifier already handles"

patterns-established:
  - "Closed report contract: schema kiana.capability-governance-production-report.v1, version 1.0, exact top-level keys, immutable ordered 14-ID checks array"
  - "GREEN gate requires passing validate_production_report against the live manifest SHA and evaluation time before returning success"

requirements-completed:
  - COD-01
  - DIF-11

coverage:
  - id: D1
    description: "Closed production report schema (kiana.capability-governance-production-report.v1 / 1.0) with immutable 14-check ID tuple, strict validator rejecting unknown/missing/duplicate/stale/malformed/non-pass data"
    requirement: COD-01
    verification:
      - kind: unit
        ref: "scripts/tests/test_generate_capability_governance.py"
        status: pass
    human_judgment: false
  - id: D2
    description: "Single-load generate-capability-governance.py verify-production path: loads canonical bundle once, emits bounded deterministic JSON report, writes explicit report and drift-report paths"
    requirement: DIF-11
    verification:
      - kind: unit
        ref: "scripts/tests/test_generate_capability_governance.py"
        status: pass
      - kind: integration
        ref: "bash scripts/capability-governance-smoke.sh production"
        status: pass
    human_judgment: false
  - id: D3
    description: "Bash capability-governance-smoke.sh production consumer wired to shared validate_production_report — rejects incomplete reports without duplicating the ID list in Bash"
    requirement: COD-01
    verification:
      - kind: integration
        ref: "bash scripts/capability-governance-smoke.sh production"
        status: pass
    human_judgment: false
  - id: D4
    description: "Linux regression linux_production_slice_has_fresh_process_headroom: two fresh public production invocations both complete below 25 s with offline/no-network/receipt/cleanup conditions intact"
    requirement: DIF-11
    verification:
      - kind: integration
        ref: "cargo test -p kiana-capability-governance-supervisor --test supervisor_linux linux_production_slice_has_fresh_process_headroom --locked --offline -- --exact --test-threads=1"
        status: pass
    human_judgment: false

duration: 90min
completed: 2026-07-23
status: complete
---

# Phase 01: Plan 13 Summary

**Fail-closed single-load production verifier with closed 14-check report schema and repeatable fresh-process headroom regression proving ≥5 s below the 30-second supervisor deadline**

## Performance

- **Duration:** ~90 min
- **Completed:** 2026-07-23T13:17:38+08:00
- **Tasks:** 3
- **Files modified:** 5

## Accomplishments

- Defined `PRODUCTION_REPORT_SCHEMA`, `PRODUCTION_REPORT_VERSION`, and immutable ordered `PRODUCTION_CHECK_IDS` (14 IDs) in `capability_governance.py`; added `validate_production_report` as the single strict boundary between producer and consumer
- Refactored `generate-capability-governance.py verify-production` to call `load_validated_manifest_bundle` exactly once, produce all 14 subcheck rows from that one validated result, self-validate before writing, and write report + drift-report to explicit requested paths
- Added 178-line Python test suite covering valid reports, every rejected mutation (unknown field/ID, missing ID, duplicate ID, stale metadata, malformed status, non-pass row), and a loader-count double proving single-bundle load
- Simplified `capability-governance-smoke.sh production` from 104 to 66 lines by removing redundant drift/corpus/legacy calls and replacing the Bash-side check list with a call to `validate_production_report` via the isolated Python interpreter
- Added `linux_production_slice_has_fresh_process_headroom` Rust test (157 lines) that starts two independent supervisor/bwrap/Bash/Python chains, parses Rust-emitted elapsed values strictly, and enforces both samples complete below 25 s with sandbox/network/receipt/cleanup conditions intact

## Task Commits

1. **Task 1: Pre-optimization fresh-process RED regression** — `88af2b4` (test)
2. **Task 2: Closed report authority + single-load verifier** — `84165ce` (test), `a1ae490` (feat)
3. **Task 3: Wire shared report consumer + GREEN confirmation** — `552c1e9` (feat)

## Files Created/Modified

- `scripts/capability_governance.py` — Added `PRODUCTION_REPORT_SCHEMA`, `PRODUCTION_REPORT_VERSION`, `PRODUCTION_CHECK_IDS`, `validate_production_report`; +112 lines
- `scripts/generate-capability-governance.py` — Single-load `verify-production` refactor; +468/-29 lines
- `scripts/tests/test_generate_capability_governance.py` — 178-line closed report contract test suite (created)
- `scripts/capability-governance-smoke.sh` — Shared-validator consumer wiring; +33/-71 lines
- `kiana-capability-governance-supervisor/tests/supervisor_linux.rs` — Fresh-process headroom regression; +157 lines

## Decisions Made

- Immutable 14-ID tuple lives in `capability_governance.py`; both the Python producer and Bash consumer reference the same authority — no duplication across the boundary
- `validate_production_report` is the single gating function; it binds the report to the live manifest SHA and evaluation time and fails closed on any unknown, missing, duplicate, stale, malformed, or non-pass value
- The Bash consumer calls `validate_production_report` via the existing isolated Python interpreter pattern rather than re-implementing validation in shell
- The Rust test uses the real prebuilt supervisor binary and `env_clear` launch pattern — no mock, no global cache flush, no deadline change; cold-path timing comes from two genuine independent process chains

## Deviations from Plan

None — plan executed exactly as written. The RED-to-GREEN TDD sequence followed the committed task order: Task 1 established the pre-optimization baseline before any Python/Bash edit; Tasks 2 and 3 turned it GREEN.

## Issues Encountered

None.

## Next Phase Readiness

- Phase 01 production proof now has a deterministic closed report handoff and repeatable measured fresh-process headroom
- All three prior verification gaps (CR-01, redundant bundle loads, timing reliability) resolved
- Ready for phase-level verification (`/gsd-verify-work 01`)

---
*Phase: 01-baseline-evidence-governance*
*Completed: 2026-07-23*
