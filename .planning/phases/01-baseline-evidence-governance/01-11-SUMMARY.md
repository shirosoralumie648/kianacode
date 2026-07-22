---
phase: 01-baseline-evidence-governance
plan: "11"
subsystem: governance-generated-views
tags: [deterministic-rendering, ancestry-diffs, offline-supervisor, production-gate]
requires:
  - phase: 01-baseline-evidence-governance
    provides: Canonical public, repository, decision, evidence, legacy, and selector heads from Plans 01-03, 01-06, and 01-10
provides:
  - Deterministic and root-confined public, reference, and legacy governance rendering
  - Exact-ID/hash-bound public and repository ancestry diffs with tamper detection
  - Offline generated-views and production slices plus the nine-slice aggregate gate
  - Refreshed reviewed governance heads for the live TUI source fingerprint
affects: [01-12-broad-gate-integration, phase-01-verification]
tech-stack:
  added: []
  patterns:
    - Render and diff output is compiled only from validated explicit canonical heads
    - Historical diff targets must be exact members of the selected family ancestry
    - Family diff validation checks every ancestor in that family without repeatedly validating unrelated families
key-files:
  created:
    - scripts/generate-capability-governance.py
    - docs/agent-program/kiana-completion/governance/generated/public-parity.md
    - docs/agent-program/kiana-completion/governance/generated/reference-governance.md
    - docs/agent-program/kiana-completion/governance/generated/legacy-authority.md
    - docs/reference_audit/CAPABILITY-GOVERNANCE.generated.txt
    - docs/agent-program/kiana-completion/governance/capability-decisions/review-2026-07-22.json
    - docs/agent-program/kiana-completion/governance/drift/references-2026-07-22.json
    - docs/agent-program/kiana-completion/governance/evidence/revisions/evidence-2026-07-22.json
    - docs/agent-program/kiana-completion/governance/repository-registry/references-2026-07-22.json
  modified:
    - scripts/capability_governance.py
    - scripts/capability-governance-smoke.sh
    - docs/agent-program/kiana-completion/governance/current.json
    - kiana-capability-governance-supervisor/src/lib.rs
    - kiana-capability-governance-supervisor/tests/supervisor_linux.rs
key-decisions:
  - "The 30-second supervisor deadline remains unchanged; production work must fit inside the existing release boundary."
  - "Historical diff targets are legal only when they are exact revisions in the selector's current family ancestry, and every revision in that chain is structurally validated."
  - "Unrelated governance families are validated by render and production, not revalidated for every family-specific diff invocation."
patterns-established:
  - "Validated compiler views: generated status is exact-byte reproducible and never treated as a separate authority."
  - "Bounded ancestry validation: family diffs retain fail-closed chain validation without unrelated bundle amplification."
requirements-completed: [COD-01, DIF-11]
coverage:
  - id: D1
    description: "Deterministic default and compatibility rendering confines writes to explicit roots and preserves bytes and mtimes under check mode."
    requirement: COD-01
    verification:
      - kind: integration
        ref: "01-11 Task 1 exact automated command"
        status: pass
      - kind: unit
        ref: "scripts/tests/test_generate_capability_governance.py"
        status: pass
    human_judgment: false
  - id: D2
    description: "Public and repository diffs bind exact revision IDs and hashes, traverse selected-family ancestry, and reject tampered bytes without rewriting them."
    requirement: DIF-11
    verification:
      - kind: integration
        ref: "01-11 Task 2 exact automated command"
        status: pass
      - kind: unit
        ref: "scripts/tests/test_generate_capability_governance.py#selected-family ancestry regressions"
        status: pass
    human_judgment: false
  - id: D3
    description: "The focused governance runner exposes generated-views and production slices and runs all nine slices offline below the 30-second supervisor deadline."
    requirement: DIF-11
    verification:
      - kind: integration
        ref: "bash scripts/capability-governance-smoke.sh"
        status: pass
      - kind: integration
        ref: "bash scripts/capability-governance-smoke.sh production"
        status: pass
    human_judgment: false
duration: 5d 4h 55m elapsed across interrupted sessions
completed: 2026-07-22
status: complete
---

# Phase 01 Plan 11: Deterministic Governance Views And Production Gate Summary

**Root-confined governance rendering, ancestry-bound tamper-sensitive diffs, and a nine-slice offline production gate with refreshed live-source custody**

## Performance

- **Duration:** 5d 4h 55m elapsed across interrupted sessions
- **Started:** 2026-07-17T12:00:05Z
- **Completed:** 2026-07-22T16:54:56Z
- **Tasks:** 3
- **Files modified:** 27

## Accomplishments

- Added deterministic `render`, `render-compat`, and `diff` behavior with explicit output roots, allowlisted repository compatibility output, exact-byte checks, hostile-value escaping, and no-write failure paths.
- Generated canonical public, reference, and legacy views plus exact revision/hash-bound public and repository diffs whose independent tamper checks fail without rewriting evidence.
- Added `generated-views` and `production` supervisor slices and a no-argument nine-slice aggregate; fresh runs remained offline and below the existing 30-second per-slice deadline.
- Refreshed reviewed governance custody to `governance-review-2026-07-22-v1` after live TUI source drift while preserving the unchanged 2026-07-18 registry bytes and revision identity.

## Task Commits

1. **Task 1: Implement and directly gate deterministic render confinement** - `6496976`
2. **Task 2: Directly generate and tamper-check production ledgers and diffs** - `4936ce2`
3. **Task 3: Generate directory warning and finish all progressive runner slices** - `52c17bd`
4. **Recovery: Refresh governance after target drift and bound family diff validation** - `cdf70cf`

## Verification Evidence

- Task 1 exact `<automated>` command: passed.
- Task 2 exact `<automated>` command: passed.
- Python governance unit suite: 41 tests passed; `py_compile` passed.
- `cargo fmt --all --check`: passed.
- Governance bundle validation: `valid`; live drift: `current`.
- `generated-views`: 14.657 seconds, `offline=true`.
- `semantic-negative`: 17.023 seconds, `offline=true`.
- No-argument nine-slice aggregate: passed; slowest slice was `production` at 23.639 seconds.
- Explicit `production`: 19.347 seconds, `offline=true`.
- Exact scoped `git diff --check`: passed.

## Decisions Made

- Kept the supervisor deadline at 30 seconds and removed repeated unrelated-family validation from individual diff calls instead of weakening the release gate.
- Kept historical `diff --to` support fail closed: the target must be an exact selected-family ancestor, and malformed intermediate predecessors remain rejected.
- Published a new reviewed custody revision for the changed TUI SHA rather than mutating prior review facts or pretending the older evidence was current.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Refreshed canonical governance after live target drift**
- **Found during:** Task 3 production verification
- **Issue:** Two reviewed decisions still bound the previous SHA-256 for `kiana-entrypoints/src/tui.rs`; production correctly rejected the stale custody record.
- **Fix:** Published review revision `reference-reviewed-2026-07-22-tui-refresh`, evaluation time `2026-07-22T16:14:52Z`, new dated decision/evidence/drift artifacts, and a dated registry copy. The registry content and revision remained byte-identical to the 2026-07-18 canonical registry.
- **Files modified:** `docs/agent-program/kiana-completion/governance/current.json` and the 2026-07-22 reviewed governance artifacts.
- **Verification:** Canonical bundle reports `valid`; live drift reports `current`; production passes offline.
- **Committed in:** `cdf70cf`

**2. [Rule 1 - Bug] Removed unrelated bundle amplification from family diff validation**
- **Found during:** Task 3 `generated-views` deadline verification
- **Issue:** Each historical diff CLI invocation revalidated the entire governance bundle, causing `generated-views` to exceed the fixed 30-second supervisor deadline.
- **Fix:** Diff validation now validates the complete selected-family ancestry and exact selector binding without revalidating unrelated families per invocation. Added regressions for required current ancestry and malformed intermediate predecessors.
- **Files modified:** `scripts/generate-capability-governance.py`, `scripts/tests/test_generate_capability_governance.py`.
- **Verification:** 41 unit tests pass; `generated-views` completes in 14.657 seconds and the aggregate production slice remains below 30 seconds.
- **Committed in:** `cdf70cf`

**Total deviations:** 2 auto-fixed (1 blocking custody refresh, 1 validation performance bug). **Impact:** Both were required to retain fail-closed current-source proof and the existing supervisor deadline; no completion semantics or release threshold were weakened.

## Issues Encountered

- Execution was interrupted after production commits existed but before the required SUMMARY and planning-state close-out. The recovery revalidated all task commands and production modes before creating this summary.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plan 01-12 can wire the focused runner exactly once into `scripts/schema-contract-smoke.sh`.
- The live target file is currently clean, so Plan 01-12 must re-evaluate its original dirty-user-hunk assumption before applying its alternate-index preservation protocol.
- The only remaining worktree change is the pre-existing user-owned `.planning/config.json` edit, which remains unstaged and excluded from this plan.

---
*Phase: 01-baseline-evidence-governance*
*Completed: 2026-07-22*
