---
phase: 01-baseline-evidence-governance
plan: "12"
subsystem: governance-broad-gate-integration
tags: [schema-smoke, alternate-index, git-integrity, offline-governance]
requires:
  - phase: 01-baseline-evidence-governance
    provides: Completed focused capability-governance aggregate and production gate from Plan 01-11
provides:
  - Exactly one focused governance invocation at the broad schema gate boundary
  - One-path alternate-index commit with verified parent, mode, blob, and patch scope
  - Pre/post focused and broad smoke proof with unrelated index preservation
affects: [phase-01-verification, repository-schema-gate]
tech-stack:
  added: []
  patterns:
    - HEAD-derived alternate index isolates a one-line integration commit from normal-index state
    - Clean and dirty worktree preconditions are handled explicitly rather than inferred
key-files:
  created: []
  modified:
    - scripts/schema-contract-smoke.sh
key-decisions:
  - "Retain the alternate-index protocol even though the target file is now clean, so parent/tree/mode/blob/scope proofs remain identical to the planned high-assurance path."
  - "When preflight proves HEAD, normal index, and live bytes are identical, the correct postcondition is a clean target after the cacheinfo update; manufacturing a dirty hunk would falsify custody."
patterns-established:
  - "State-sensitive Git proof: preservation assertions derive from observed preflight state and fail closed on any transition mismatch."
requirements-completed: [COD-01, DIF-11]
coverage:
  - id: D1
    description: "The broad schema contract gate invokes the focused governance runner exactly once after the commercial handoff smoke and before final success."
    requirement: COD-01
    verification:
      - kind: integration
        ref: "bash scripts/capability-governance-smoke.sh"
        status: pass
      - kind: integration
        ref: "bash scripts/schema-contract-smoke.sh"
        status: pass
    human_judgment: false
  - id: D2
    description: "The integration commit changes one path by one added line while preserving mode, exact blob identity, and all unrelated normal-index entries."
    requirement: DIF-11
    verification:
      - kind: other
        ref: "01-12 alternate-index parent/tree/mode/blob/patch and unrelated-index proof"
        status: pass
    human_judgment: false
duration: 9min
completed: 2026-07-22
status: complete
---

# Phase 01 Plan 12: Governance Broad Gate Integration Summary

**The repository-wide schema smoke now invokes the focused capability-governance aggregate exactly once through a one-line alternate-index commit**

## Performance

- **Duration:** 9 min
- **Started:** 2026-07-22T17:01:35Z
- **Completed:** 2026-07-22T17:10:22Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments

- Added exactly `bash scripts/capability-governance-smoke.sh` after the commercial handoff smoke and before the broad gate's final success message.
- Created commit `90efe96` from a HEAD-derived alternate index; the commit changes one path with one insertion, retains mode `100644`, and binds blob `f6333781ad1d87c205b817ae7c23f005abf5fb60`.
- Proved the target and all unrelated normal-index entries remained consistent across the commit and path-specific `cacheinfo` update; the unrelated-index SHA-256 remained `37ece6e8ae07e1c998d7ddcdc1fdf3cd96dd4795a32b2ea10650db07666bade9`.
- Ran both focused and broad gates before and after commit; every governance slice reported `offline=true` and the broad gate completed successfully.

## Task Commits

1. **Task 1: Wire the focused runner with alternate-index preservation proof** - `90efe96`

## Verification Evidence

- Preflight HEAD: `d299e4956a2ddbdfb17bd17c5bdec363f86c2798`; target mode/blob: `100644` / `0b6963c7ee08dd5e78472bdbb16a5f6f88456308`.
- HEAD, normal index, and live target bytes were identical before editing, with SHA-256 `9559905b0e8fdbd96a72642329b71c6520ad03f2d39f5879329b2efdbc3492bf`.
- Removing only the inserted line from both edited copies reproduced the complete preflight bytes.
- Precommit focused aggregate: passed in 22.78 seconds; precommit broad gate: passed in 30.49 seconds.
- Commit parent, changed-name set, one-line patch, mode, type, path, and blob checks: passed.
- Postcommit focused aggregate: passed in 22.99 seconds; postcommit broad gate: passed in 28.96 seconds.
- Invocation count is exactly one; staged and unstaged target diffs are both empty after the verified normal-index update.

## Decisions Made

- Used the planned alternate index even though the target file was no longer dirty, preserving the strongest commit-scope proof without inventing unrelated worktree bytes.
- Replaced only the stale dirty-postcondition assertion with a clean-postcondition assertion derived from preflight; every anchor, remove-only, parent, patch, mode, blob, index, and smoke invariant remained enforced.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Adapted the dirty-user-hunk protocol to the observed clean target**
- **Found during:** Task 1 preflight
- **Issue:** The plan was authored when `scripts/schema-contract-smoke.sh` had pre-existing user changes and therefore required it to remain dirty after commit. Live preflight proved the file was now identical in HEAD, the normal index, and the worktree, so that final assertion was impossible and no user hunk existed to preserve.
- **Fix:** Kept the complete alternate-index, dual-copy, remove-only, parent/tree/mode/blob, unrelated-index, and pre/post smoke protocol. Required the target to return clean after the verified target-only normal-index update instead of fabricating a dirty change.
- **Files modified:** `scripts/schema-contract-smoke.sh` only.
- **Verification:** The commit contains exactly one insertion; the target is clean; unrelated index bytes are identical; both focused and broad gates pass before and after commit.
- **Committed in:** `90efe96`

**Total deviations:** 1 auto-fixed (1 blocking stale precondition). **Impact:** The intended custody and scope guarantees were preserved; the only changed assertion now matches the observed clean preflight state.

## Issues Encountered

None beyond the stale dirty-file assumption documented above.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- All 12 Phase 01 plans now have implementation commits and summaries.
- Phase 01 is implementation-complete and ready for canonical verification; it is not phase-complete until verification passes.
- The pre-existing user-owned `.planning/config.json` edit remains unstaged and excluded.

---
*Phase: 01-baseline-evidence-governance*
*Completed: 2026-07-22*
