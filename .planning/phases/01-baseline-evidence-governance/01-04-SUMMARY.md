---
phase: 01-baseline-evidence-governance
plan: "04"
subsystem: governance-validation
tags: [python, json-schema, semantic-validation, ancestry, evidence, drift, sandbox, rust-supervisor]

requires:
  - phase: 01-baseline-evidence-governance
    provides: Eight closed governance schemas, valid fixture graphs, and the Rust offline supervisor from Plans 01-01 through 01-03
provides:
  - One production Python authority for governance semantics, ancestry, evidence freshness, completion, mutations, and drift
  - Thin validate, validate-history, and check-drift CLI with deterministic JSON diagnostics and exact 0/1/2 exits
  - Rust-supervised public-baseline and reference-governance positive slices under the 30-second per-slice budget
affects: [01-05-coverage-corpus, 01-06-integrity-corpus, 01-07-freeze-refresh, 01-10-legacy-authority, 01-11-generated-views, 01-12-schema-gate]

tech-stack:
  added: []
  patterns:
    - Shared production semantics imported by thin CLI and smoke consumers
    - Bounded UTF-8 JSON reads, repository-relative containment, deterministic redaction, and sorted typed diagnostics
    - Highest-sequence evidence and explicit evaluation time control freshness and completion
    - Rust supervisor allowlists every public semantic slice and preserves receipt/offline authority

key-files:
  created:
    - scripts/capability_governance.py
    - scripts/validate-capability-governance.py
  modified:
    - scripts/capability-governance-smoke.sh
    - kiana-capability-governance-supervisor/src/lib.rs
    - kiana-capability-governance-supervisor/tests/supervisor_linux.rs

key-decisions:
  - "Semantic validity and governance completion remain separate: an incomplete decision inventory is valid data, while duplicate or out-of-inventory current decisions fail closed."
  - "The thin CLI resolves its sibling production module explicitly under Python isolated mode and never trusts PYTHONPATH or caller cwd."
  - "Public-baseline and reference-governance are first-class Rust supervisor slices; shell-only aliases cannot satisfy the receipt authority contract."
  - "Direct checks cover minimal/full bundles while supervised slices cover offline/hostile and full-38 inputs, keeping each traced slice below its 30-second deadline."

patterns-established:
  - "Production-first governance: negative corpora consume capability_governance.py after this plan; fixtures do not own semantic branches."
  - "Validation order: bounded load -> structural helper -> semantic graph -> ancestry/evidence -> manifest binding -> optional drift."

requirements-completed: [COD-01, DIF-11]

coverage:
  - id: D1
    description: "A single production module exposes proof ranking, completion, ancestry, mutation, semantic validation, and drift operations behind bounded fail-closed inputs."
    requirement: COD-01
    verification:
      - kind: integration
        ref: "python3 -m py_compile scripts/capability_governance.py scripts/validate-capability-governance.py"
        status: pass
      - kind: integration
        ref: "python3 scripts/validate-capability-governance.py validate --fixture-bundle scripts/fixtures/capability-governance/valid/minimal-graph.json --json"
        status: pass
    human_judgment: false
  - id: D2
    description: "All four protected valid fixture families pass the production CLI without changing fixture, schema, or structural-helper bytes."
    requirement: DIF-11
    verification:
      - kind: integration
        ref: "direct minimal/full validation plus protected SHA-256 manifest"
        status: pass
      - kind: integration
        ref: "bash scripts/capability-governance-smoke.sh public-baseline"
        status: pass
      - kind: integration
        ref: "bash scripts/capability-governance-smoke.sh reference-governance"
        status: pass
    human_judgment: false
  - id: D3
    description: "The Rust supervisor recognizes four public slices and retains sandbox, trace, deadline, cleanup, and exact receipt enforcement."
    requirement: DIF-11
    verification:
      - kind: integration
        ref: "cargo test -p kiana-capability-governance-supervisor --locked --offline --no-fail-fast"
        status: pass
    human_judgment: false

duration: 35 min
completed: 2026-07-16
status: complete
---

# Phase 1 Plan 4: Production Governance Semantics Summary

**One bounded production validator now owns governance semantics, immutable ancestry, evidence freshness/completion, drift fingerprints, and deterministic diagnostics, with positive execution inside the existing Rust offline authority.**

## Performance

- **Duration:** 35 min
- **Started:** 2026-07-16T09:38:05Z
- **Completed:** 2026-07-16T10:13:35Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments

- Added `capability_governance.py` as the single production implementation for bounded loading, structural and cross-family validation, ancestry, evidence chains, completion, mutations, canonical hashes, and drift detection.
- Added a thin CLI with exact `validate`, `validate-history`, and `check-drift` surfaces, deterministic JSON diagnostics, redaction, and 0/1/2 status behavior.
- Added Rust-supervised positive `public-baseline` and `reference-governance` slices and proved the full supervisor crate with 24 unit and 10 Linux integration tests.
- Preserved all protected valid fixture, schema, and structural-helper SHA-256 values byte-for-byte.

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement the complete production semantic validator and CLI** - `16169e5` (feat)
2. **Task 2: Add and run post-implementation positive semantic verification** - `b9e83f0` (verify)

**Plan metadata:** committed with this SUMMARY and the GSD tracking updates.

## Files Created/Modified

- `scripts/capability_governance.py` - Shared bounded semantic, ancestry, evidence, completion, mutation, and drift authority.
- `scripts/validate-capability-governance.py` - Thin isolated-mode-safe CLI and deterministic result renderer.
- `scripts/capability-governance-smoke.sh` - Positive production CLI slices plus protected-input hash assertions.
- `kiana-capability-governance-supervisor/src/lib.rs` - Public slice allowlist for the two production semantic families.
- `kiana-capability-governance-supervisor/tests/supervisor_linux.rs` - Four-slice sandbox/receipt integration coverage.

## Decisions Made

- Missing decision rows describe incomplete governance, while duplicate or out-of-inventory current decisions are invalid; this keeps minimal valid graphs usable without weakening 38/38 completion reporting.
- The CLI explicitly imports only from its own trusted script directory under `python -I`, eliminating ambient import influence.
- Supervisor slices remain explicit enum variants so receipt identity cannot be forged through shell aliases.
- The traced public slice performs two CLI invocations, while direct checks cover minimal/full, avoiding a three-process 30-second deadline overrun without dropping fixture coverage.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Extended the Rust supervisor slice allowlist**
- **Found during:** Task 2 (positive semantic verification)
- **Issue:** The plan listed only the shell runner, but the Rust supervisor rejected both new public slice names before the worker could run.
- **Fix:** Added two `Slice` variants, parser/string mappings, a RED/GREEN parser assertion, and four-slice Linux integration coverage.
- **Files modified:** `kiana-capability-governance-supervisor/src/lib.rs`, `kiana-capability-governance-supervisor/tests/supervisor_linux.rs`
- **Verification:** Full supervisor crate passed 24 unit and 10 Linux integration tests.
- **Committed in:** `b9e83f0`

**2. [Rule 1 - Bug] Made the thin CLI import-safe under Python isolated mode**
- **Found during:** Task 2 supervisor GREEN
- **Issue:** `/usr/bin/python3 -I` excludes the script directory from `sys.path`, causing `ModuleNotFoundError` and a missing completion receipt.
- **Fix:** Resolve and prepend only the CLI's own trusted directory before importing the sibling production module.
- **Files modified:** `scripts/validate-capability-governance.py`
- **Verification:** The isolated CLI and both supervised slices pass while ambient `PYTHONPATH` remains ignored.
- **Committed in:** `b9e83f0`

**3. [Rule 1 - Bug] Kept traced positive slices within the fixed deadline**
- **Found during:** Task 2 supervisor GREEN
- **Issue:** Three independent Python/jsonschema processes accumulated supervisor tracing overhead and exceeded the 30-second slice deadline even though each direct validation took less than 0.25 seconds.
- **Fix:** Kept minimal/full in the required direct checks, assigned offline/hostile to `public-baseline`, and full-38 to `reference-governance`.
- **Files modified:** `scripts/capability-governance-smoke.sh`
- **Verification:** Supervised elapsed times were 18.125 seconds and 9.621 seconds; the four-slice integration test passed.
- **Committed in:** `b9e83f0`

---

**Total deviations:** 3 auto-fixed (1 blocking prerequisite, 2 implementation bugs). **Impact:** All fixes are required to satisfy the approved supervisor boundary and time budget; no governance scope or negative fixture was added.

## Issues Encountered

- The first filtered Rust RED command used `--exact` and ran zero tests. It was discarded as evidence and rerun correctly, producing the expected failing allowlist assertion before implementation.
- Concurrent supervisor runs created resource contention during one diagnostic batch. All authoritative supervisor checks were rerun serially.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plan 01-05 can now generate and execute the official-source coverage adversarial corpus against stable production codes.
- Plan 01-06 can consume the same API for ancestry, semantic, drift, alias, and security adversarial cases.
- No protected fixture or pre-existing dirty user file was staged or rewritten.

## Self-Check: PASSED

- All declared key files exist.
- `git log --all --grep=01-04` contains both task commits.
- Direct minimal/full validation, both Rust-supervised slices, full supervisor tests, Python/Bash syntax, Rust formatting, and scoped `git diff --check` passed.
- Protected valid fixtures, schemas, and `scripts/validate-json-schema.py` retain their recorded SHA-256 values.

---
*Phase: 01-baseline-evidence-governance*
*Completed: 2026-07-16*
