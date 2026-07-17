---
phase: 01-baseline-evidence-governance
plan: "10"
subsystem: evidence-and-legacy-authority
tags: [evidence-ledger, legacy-authority, selector, offline-supervisor]
requires:
  - phase: 01-baseline-evidence-governance
    provides: Reviewed public, repository, and capability-decision heads from Plans 01-08 and 01-09
provides:
  - Immutable 249-record evidence genesis and 537-record current successor
  - Exact hash classification for 20 legacy status-bearing inputs
  - Closed current selector, deterministic evaluation time, and safe compatibility allowlist
  - Rust-supervised legacy-authority slice
affects: [01-11-generated-views, 01-12-aggregate-gate]
tech-stack:
  added: []
  patterns:
    - Evidence successor preserves the complete genesis prefix and appends review records only
    - Legacy inputs remain byte-untouched frozen migration inputs with canonical replacement paths
key-files:
  created:
    - docs/agent-program/kiana-completion/governance/evidence/revisions/evidence-2026-07-15-genesis.json
    - docs/agent-program/kiana-completion/governance/evidence/revisions/evidence-2026-07-15.json
    - docs/agent-program/kiana-completion/governance/legacy-authority/legacy-authority-2026-07-15-genesis.json
    - docs/agent-program/kiana-completion/governance/legacy-authority/legacy-authority-2026-07-15.json
    - docs/agent-program/kiana-completion/governance/current.json
    - docs/agent-program/kiana-completion/governance/compat-outputs.json
  modified:
    - scripts/capability-governance-smoke.sh
    - kiana-capability-governance-supervisor/src/lib.rs
    - kiana-capability-governance-supervisor/tests/supervisor_linux.rs
key-decisions:
  - "No expired, failed, target-environment, or user-value evidence is fabricated when no genuine observation exists."
  - "Legacy classification evidence is appended to the current evidence successor while preserving the committed genesis prefix."
  - "current.json selects six exact canonical heads by path and canonical hash with evaluation_time 2026-07-17T11:32:00Z."
patterns-established:
  - "Explicit selection: schema-valid path/hash bindings replace newest-file discovery."
requirements-completed: [COD-01, DIF-11]
coverage:
  - id: D1
    description: "Evidence history preserves a 249-record genesis prefix inside a 537-record hash-chained successor with source-only proof."
    requirement: COD-01
    verification:
      - kind: integration
        ref: "01-10 evidence schema/history/prefix/hash assertions"
        status: pass
    human_judgment: false
  - id: D2
    description: "Twenty live legacy paths are classified and hash-bound without editing them; current heads are explicitly selected and rehashed."
    requirement: DIF-11
    verification:
      - kind: integration
        ref: "bash scripts/capability-governance-smoke.sh legacy-authority"
        status: pass
      - kind: integration
        ref: "production manifest semantic validation"
        status: pass
    human_judgment: false
duration: 18min
completed: 2026-07-17
status: complete
---

# Phase 01 Plan 10: Evidence And Legacy Authority Summary

**A 537-record source-only evidence chain, 20-path legacy retirement ledger, and explicit canonical-head selector guarded by the Rust offline supervisor**

## Performance

- **Duration:** 18 min
- **Started:** 2026-07-17T11:25:16Z
- **Completed:** 2026-07-17T11:43:19Z
- **Tasks:** 2
- **Files modified:** 9

## Accomplishments

- 建立 249-record evidence genesis；current successor 保留完整 prefix，并追加 public、registry、decision、security、alias 和 legacy review，总计 537 records。
- 没有制造 expiry、failed retest、target environment 或 user acceptance 事实；全部 proof 保守为 `source`。
- 对 16 个 `docs/reference_audit/*.md` 和四个 named legacy inputs 做逐路径 SHA-256/classification/replacement review，原文件零修改。
- `current.json` 绑定 official/public/registry/decision/evidence/legacy 六个明确 head；compat allowlist 只允许一个新 repository warning 和三个 generated-root views。
- `legacy-authority` 经 Rust supervisor 运行，验证 inventory、hash、ancestry 和 selector，不猜 newest filename。

## Task Commits

1. **Task 1: Bind immutable evidence genesis and current successor** - `6a0f601`
2. **Task 2: Classify legacy authority, bind production heads, and add its runner slice** - `8a387f1`

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Appended legacy classification evidence after inventory creation**
- **Found during:** Task 2
- **Issue:** Legacy schema requires evidence IDs, but Task 1 could not know the later exact path inventory; omitting records would make the full production bundle invalid.
- **Fix:** Preserved the evidence genesis and existing 497-record successor prefix, then appended 40 import/review legacy records and updated the selected evidence-head hash.
- **Verification:** Full `validate --manifest current.json` returns `status=valid`.
- **Committed in:** `8a387f1`

**2. [Rule 3 - Blocking] Extended the Rust supervisor progressive slice allowlist**
- **Found during:** Task 2
- **Issue:** Bash recognized `legacy-authority`, but the sole Rust runtime authority rejected it as usage.
- **Fix:** Added the typed slice and Linux integration coverage; corrected an initial heredoc placement error found by `receipt_invalid` diagnostics.
- **Verification:** 24 unit and 10 Linux integration tests pass; legacy slice reports `offline=true`.
- **Committed in:** `8a387f1`

**Total deviations:** 2 auto-fixed. **Impact:** Both close mandatory evidence/runtime authority contracts without changing legacy inputs.

## Issues Encountered

None beyond the auto-fixed items above.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plan 01-11 can consume the now-valid production manifest to generate deterministic public/reference/legacy views and two family diffs.
- Legacy Markdown remains dirty/user-owned where it was before and was never staged.

---
*Phase: 01-baseline-evidence-governance*
*Completed: 2026-07-17*
