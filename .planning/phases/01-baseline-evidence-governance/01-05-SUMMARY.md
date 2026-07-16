---
phase: 01-baseline-evidence-governance
plan: "05"
subsystem: governance-verification
tags: [negative-fixtures, source-coverage, exact-diagnostics, json, adversarial-testing]

requires:
  - phase: 01-baseline-evidence-governance
    provides: Production semantic validator and positive Rust-supervised evidence from Plan 01-04
provides:
  - Four post-implementation official-source coverage adversarial fixtures
  - Exact case manifest for missing, unmapped, duplicate, and unjustified coverage failures
  - Deterministic production execution evidence with protected-base revalidation
affects: [01-06-integrity-corpus, 01-11-generated-views, 01-12-schema-gate]

tech-stack:
  added: []
  patterns:
    - Structured jq derivation preserves a protected base while changing one governed invariant
    - Case manifests bind command arguments, exact code, subject, mutation path, status, and freshness
    - Negative execution requires the production semantic diagnostic even when structural diagnostics also apply

key-files:
  created:
    - scripts/fixtures/capability-governance/invalid/coverage/expected-errors.json
    - scripts/fixtures/capability-governance/invalid/coverage/missing-required-capability.json
    - scripts/fixtures/capability-governance/invalid/coverage/unmapped-source-entry.json
    - scripts/fixtures/capability-governance/invalid/coverage/duplicate-source-mapping.json
    - scripts/fixtures/capability-governance/invalid/coverage/unjustified-exclusion.json
  modified: []

key-decisions:
  - "Coverage fixtures are full immutable bundle derivatives with only one semantic mutation plus the required manifest head hash update."
  - "The oracle requires the exact production code and subject; a structural error cannot substitute for the semantic target."
  - "The generic coverage/ ignore is bypassed only for the five explicit planned fixture paths, leaving the dirty .gitignore untouched."

patterns-established:
  - "Post-implementation oracle: author corpus -> commit corpus -> execute production CLI -> revalidate protected base."

requirements-completed: [COD-01, DIF-11]

coverage:
  - id: D1
    description: "Four readable fixtures independently mutate required capability mapping, source-entry coverage, mapping uniqueness, and exclusion justification."
    requirement: COD-01
    verification:
      - kind: integration
        ref: "python3 -m json.tool over coverage fixture corpus plus manifest code/path assertions"
        status: pass
    human_judgment: false
  - id: D2
    description: "Every case exits 1 and emits its declared exact code and subject through the committed production CLI."
    requirement: COD-01
    verification:
      - kind: integration
        ref: "expected-errors.json production subprocess oracle"
        status: pass
    human_judgment: false
  - id: D3
    description: "Diagnostics are deterministic, sorted, bounded, redacted, and the protected offline base remains valid and byte-identical."
    requirement: DIF-11
    verification:
      - kind: integration
        ref: "two-run byte comparison, absolute-path scan, output bound, valid-base rerun, SHA-256 5ca4f2c6..."
        status: pass
    human_judgment: false

duration: 10 min
completed: 2026-07-16
status: complete
---

# Phase 1 Plan 5: Official-Source Coverage Adversarial Corpus Summary

**Four isolated post-implementation coverage mutations now prove that missing required capability mappings, unmapped official entries, duplicate mappings, and unjustified exclusions fail with exact production diagnostics.**

## Performance

- **Duration:** 10 min
- **Started:** 2026-07-16T10:20:32Z
- **Completed:** 2026-07-16T10:31:16Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments

- Derived four full invalid bundles from the protected offline-source fixture without modifying its bytes or any production code.
- Added a machine-readable case manifest carrying the command, arguments, exact code, subject, mutation path, expected status, and freshness.
- Executed every case twice against the production CLI and confirmed deterministic exact diagnostics, bounded/redacted output, and a still-valid protected base.

## Task Commits

1. **Task 1: Create the post-implementation coverage corpus** - `16908ca` (verify)
2. **Task 2: Execute every coverage oracle against production** - verification-only; no repository changes

**Plan metadata:** committed with this SUMMARY and the GSD tracking updates.

## Files Created/Modified

- `scripts/fixtures/capability-governance/invalid/coverage/expected-errors.json` - Exact four-case production oracle.
- `scripts/fixtures/capability-governance/invalid/coverage/missing-required-capability.json` - Required CLI capability remains in journeys but is absent from source coverage.
- `scripts/fixtures/capability-governance/invalid/coverage/unmapped-source-entry.json` - Official metadata entry has no mapping or reviewed exclusion.
- `scripts/fixtures/capability-governance/invalid/coverage/duplicate-source-mapping.json` - Official CLI entry is mapped twice.
- `scripts/fixtures/capability-governance/invalid/coverage/unjustified-exclusion.json` - Exclusion rationale is removed while unrelated graph state is preserved.

## Decisions Made

- Full bundle derivatives keep every unrelated identity, journey, evidence record, and revision binding unchanged; only the mutated baseline hash in the selector is recomputed.
- Oracle success requires the semantic code and subject named by the manifest. The unjustified-exclusion case also has structural errors because the schema requires the removed field, but it still must independently emit `unjustified_exclusion`.
- Production code is not adjusted during this verification plan; any future mismatch must route to an explicit implementation gap plan.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Force-added the explicitly planned corpus under a generic ignored directory name**
- **Found during:** Task 1 commit gate
- **Issue:** `.gitignore` contains a broad `coverage/` rule that silently ignored all five planned governance artifacts.
- **Fix:** Used `git add -f` only for the five exact PLAN paths instead of editing the user's dirty `.gitignore`.
- **Files modified:** None outside the planned corpus.
- **Verification:** `git ls-files` reports exactly five coverage files and the working tree is clean for that directory.
- **Committed in:** `16908ca`

---

**Total deviations:** 1 auto-fixed blocking repository rule. **Impact:** Planned artifacts are tracked; no unrelated ignore behavior changed.

## Issues Encountered

- An initial diagnostic wrapper had invalid shell/Python f-string quoting and failed before invoking production. It was discarded and the exact PLAN oracle was rerun successfully.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plan 01-06 can add post-implementation ancestry, semantic, drift, alias, and security adversarial fixtures using the same exact-oracle pattern.
- The protected offline fixture remains SHA-256 `5ca4f2c6313e6d583e6cd31f68958a547ade091d768fbbaa19993d4ef7827201` and validates successfully.

## Self-Check: PASSED

- All five declared corpus files are tracked and valid JSON.
- Each exact code/subject oracle exits 1 with deterministic sorted diagnostics.
- The valid base exits 0 after every adversarial run.
- Production files are unchanged and scoped `git diff --check` passes.

---
*Phase: 01-baseline-evidence-governance*
*Completed: 2026-07-16*
