---
phase: 01-baseline-evidence-governance
plan: "07"
subsystem: governance-lifecycle
tags: [python, rust-supervisor, freeze, drift, refresh, immutable-history, offline]

requires:
  - phase: 01-baseline-evidence-governance
    provides: Exact production drift diagnostics, immutable ancestry validation, and the offline Rust authority from Plans 01-04 through 01-06
provides:
  - Explicit offline freeze, production check-drift, test-only drift-case, and immutable refresh commands
  - Exact eight-case repository, license, content, official-source, target, and unavailable-source lifecycle fixture
  - Append-only stale/supersession evidence with verified predecessor hashes and path/hash-only successor selector
  - Rust-supervised drift-refresh slice with exclusive output publication and protected-input preservation
affects: [01-08-governance-views, 01-10-legacy-authority, 01-11-generated-views, 01-12-schema-gate]

tech-stack:
  added: []
  patterns:
    - Controlled source bytes enter lifecycle commands only as already captured offline artifacts
    - Drift test fixtures materialize deterministic observations but call the same production detect_drift authority
    - Refresh appends revision-scoped evidence bindings while preserving every predecessor record byte
    - Git HEAD observation reads bounded repository metadata directly instead of spawning Git under the offline supervisor

key-files:
  created:
    - scripts/freeze-capability-governance.py
    - scripts/fixtures/capability-governance/valid/refresh-request.json
    - scripts/fixtures/capability-governance/valid/refresh-result.json
    - scripts/tests/test_freeze_capability_governance.py
  modified:
    - scripts/capability_governance.py
    - scripts/capability-governance-smoke.sh
    - kiana-capability-governance-supervisor/src/lib.rs
    - kiana-capability-governance-supervisor/tests/supervisor_linux.rs

key-decisions:
  - "Fixture mode is test-only and cannot replace the production manifest plus live reference, target, and official-artifact inputs."
  - "A refresh appends new evidence records and family heads; it never rewrites historical successes, and single-case refresh advances only affected families plus evidence."
  - "Revision-scoped evidence references receive new supersession bindings even when their proof remains current; only drift-affected subjects become stale or blocked."
  - "The checked refresh result is a path/hash-only selector, while current.json carries the complete immutable bundle so temporary output roots remain independently validatable."

patterns-established:
  - "Explicit lifecycle: controlled freeze -> one-shot drift comparison -> immutable refresh -> review-blocked successor."
  - "Atomic publication: validate the complete staged successor, then rename into an absent or explicitly empty output root."

requirements-completed: [COD-01, DIF-11]

coverage:
  - id: D1
    description: "Controlled official artifacts freeze offline with canonical identity, retrieval provenance, exclusive atomic create, and no overwrite."
    requirement: COD-01
    verification:
      - kind: unit
        ref: "scripts/tests/test_freeze_capability_governance.py#CapabilityGovernanceFreezeTests"
        status: pass
      - kind: integration
        ref: "01-07 Task 1 automated freeze command"
        status: pass
    human_judgment: false
  - id: D2
    description: "The no-drift control and seven exact non-current cases expose stable exit, status, diagnostic, freshness, affected-subject, and event contracts."
    requirement: DIF-11
    verification:
      - kind: unit
        ref: "scripts/tests/test_freeze_capability_governance.py#CapabilityGovernanceDriftCliTests"
        status: pass
      - kind: integration
        ref: "bash scripts/capability-governance-smoke.sh drift-refresh"
        status: pass
    human_judgment: false
  - id: D3
    description: "Refresh preserves predecessor bytes, appends valid source/public/registry/decision/evidence successors, and rejects clean, inconsistent, escaped, or existing-output requests."
    requirement: DIF-11
    verification:
      - kind: unit
        ref: "scripts/tests/test_freeze_capability_governance.py#CapabilityGovernanceRefreshTests"
        status: pass
      - kind: integration
        ref: "python3 scripts/validate-capability-governance.py validate --manifest <refresh>/current.json --json"
        status: pass
    human_judgment: false
  - id: D4
    description: "The complete drift-refresh lifecycle runs through the Rust sandbox authority with exact receipt identity, no network, deadline, cleanup, and protected-input checks."
    requirement: DIF-11
    verification:
      - kind: integration
        ref: "cargo test -p kiana-capability-governance-supervisor --locked --offline"
        status: pass
      - kind: integration
        ref: "drift-refresh elapsed_seconds=2.060 offline=true"
        status: pass
    human_judgment: false

duration: 1h 4m
completed: 2026-07-17
status: complete
---

# Phase 1 Plan 7: Offline Freeze, Drift, and Immutable Refresh Summary

**Controlled source, repository, license, content, and target identities now move through an explicit offline freeze/check-drift/refresh lifecycle that publishes validated successors without rewriting history.**

## Performance

- **Duration:** 1h 4m
- **Started:** 2026-07-17T09:36:00Z
- **Completed:** 2026-07-17T10:40:00Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments

- Added the exact `freeze`, production/test-only `check-drift`, and `refresh` CLI contracts with atomic exclusive output and no direct network path.
- Added an eight-case deterministic request plus exact refresh result covering Git HEAD/tree/license, content-only tree, official source, target revision, and unavailable source.
- Refresh now preserves predecessor files and evidence prefixes byte-for-byte, appends revision-scoped rebind and drift events, validates full ancestry, and advances only affected families.
- Registered `drift-refresh` as a first-class Rust supervisor slice; the final focused run completed in 2.060 seconds with `offline=true`.

## Task Commits

1. **Task 1 RED: Freeze and drift lifecycle contract** - `bc0e42c`
2. **Task 1 GREEN: Controlled freeze and exact drift workflow** - `99ae980`
3. **Task 2 RED: Immutable refresh and supervisor contract** - `9b92d2e`
4. **Task 2 GREEN: Immutable successors, evidence events, and offline slice** - `6f44a7f`

**Plan metadata:** committed with this SUMMARY; STATE/ROADMAP tracking follows in the close-out commit.

## Files Created/Modified

- `scripts/freeze-capability-governance.py` - Thin explicit lifecycle CLI, deterministic fixture observations, successor construction, and atomic publication.
- `scripts/fixtures/capability-governance/valid/refresh-request.json` - Exact eight-case test input and aggregate checked drift request.
- `scripts/fixtures/capability-governance/valid/refresh-result.json` - Exact path/hash-only successor selector and event-code result.
- `scripts/tests/test_freeze_capability_governance.py` - 11 lifecycle contract tests with observed RED failures before implementation.
- `scripts/capability_governance.py` - Historical official-artifact binding validation and bounded Git metadata fingerprinting.
- `scripts/capability-governance-smoke.sh` - Supervised `drift-refresh` worker plus explicit governance-bundle fixture set.
- `kiana-capability-governance-supervisor/src/lib.rs` - Explicit `DriftRefresh` slice and receipt identity.
- `kiana-capability-governance-supervisor/tests/supervisor_linux.rs` - Six-slice public execution matrix.

## Decisions Made

- Kept direct HTTP, curl, background polling, watchers, and TTL freshness outside the lifecycle CLI; official bytes must already carry controlled provenance.
- Kept fixture dispatch isolated from production arguments and delegated every case to the same `detect_drift` authority.
- Used append-only rebind evidence because evidence records bind to a family revision; historical records remain exact prefixes and cannot be edited to point at a successor.
- Read Git HEAD from bounded `.git/HEAD`, loose refs, worktree common dirs, or `packed-refs`, eliminating a traced subprocess without weakening the frozen Git identity.
- Published a self-contained `current.json` for temporary-root validation and a separate path/hash-only result selector for downstream plans.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Added immutable official-artifact history validation**
- **Found during:** Task 2 successor design
- **Issue:** The prior validator compared every historical public-baseline revision to the single current official artifact, so a legitimate source successor invalidated old bindings.
- **Fix:** Validate official predecessor path/hash ancestry and resolve each historical baseline against its own bound artifact when it differs from current.
- **Files modified:** `scripts/capability_governance.py`
- **Verification:** The refreshed full bundle validates while the original fixture remains byte-identical; malformed predecessor bindings fail closed.
- **Committed in:** `6f44a7f`

**2. [Rule 3 - Blocking] Registered the lifecycle with the Rust supervisor**
- **Found during:** Task 2 runner integration
- **Issue:** The shell slice could not become a public offline authority until Rust explicitly accepted its name and receipt identity.
- **Fix:** Added RED parser/integration coverage, `DriftRefresh` enum mapping, and the six-slice public matrix.
- **Files modified:** `kiana-capability-governance-supervisor/src/lib.rs`, `kiana-capability-governance-supervisor/tests/supervisor_linux.rs`
- **Verification:** 24 unit and 10 Linux integration tests pass.
- **Committed in:** `9b92d2e`, `6f44a7f`

**3. [Rule 1 - Bug] Removed traced subprocess amplification from lifecycle fixtures**
- **Found during:** Task 2 supervised execution
- **Issue:** The build-bound Conda Python under supervisor strace spent about eight seconds per nested process, making repeated Git/CLI subprocesses exceed the fixed 30-second deadline.
- **Fix:** Read bounded Git metadata directly, materialize deterministic detached HEAD identity, and run all exact cases through one isolated Python process.
- **Files modified:** `scripts/capability_governance.py`, `scripts/freeze-capability-governance.py`, `scripts/capability-governance-smoke.sh`
- **Verification:** The focused supervisor run completes in 2.060 seconds with `offline=true`.
- **Committed in:** `6f44a7f`

**4. [Rule 1 - Bug] Kept non-bundle lifecycle fixtures out of bundle schema extraction**
- **Found during:** Full supervisor regression
- **Issue:** The schema slice glob treated the new request/result documents as complete governance bundles and exited before issuing a receipt.
- **Fix:** Made the schema slice's four governance bundles explicit; lifecycle fixture checks remain owned by `drift-refresh`.
- **Files modified:** `scripts/capability-governance-smoke.sh`
- **Verification:** All six public slices pass in the 10-test Linux integration suite.
- **Committed in:** `6f44a7f`

---

**Total deviations:** 4 auto-fixed (2 missing/blocking contracts, 2 implementation bugs). **Impact:** Each change is required for immutable correctness or the existing fail-closed supervisor boundary; no product scope, protected predecessor, dirty user file, or network restriction was weakened.

## Issues Encountered

- The initial deadline hypothesis reduced temporary Git setup but did not clear the failure. A controlled bwrap/strace comparison isolated the actual Conda nested-process cost before the single-process fix was applied.
- Aggregate refresh initially advanced all families even for a single target drift. An additional RED test now proves that target-only refresh advances only decisions and evidence.

## User Setup Required

None - no external service, account, or network configuration is required.

## Next Phase Readiness

- Plan 01-08 can consume deterministic current/stale/unavailable reports and immutable successor selectors.
- Later generated-view and schema gates can run `drift-refresh` alongside the existing supervised slices.
- No unresolved Plan 07 freeze, drift, refresh, ancestry, offline, overwrite, or protected-input blocker remains.

## Self-Check: PASSED

- All eight declared implementation/test files exist and the four Plan 07 task commits are present.
- 35 Python contract tests, both exact task commands, Python/Bash syntax, static no-network checks, Rust formatting, 24 supervisor unit tests, and 10 Linux integration tests pass.
- Exact refresh bytes match `refresh-result.json`; `current.json` validates through the production validator; predecessor fixture bytes remain unchanged.
- Plan 07 commits do not include the dirty plan, `scripts/validate-json-schema.py`, or unrelated user worktree changes.

---
*Phase: 01-baseline-evidence-governance*
*Completed: 2026-07-17*
