---
phase: 01-baseline-evidence-governance
plan: "02"
subsystem: governance-contracts
tags: [json-schema, evidence-history, legacy-authority, deterministic-governance]

requires:
  - phase: 01-baseline-evidence-governance
    provides: Canonical genesis/successor ancestry and closed schema patterns from Plan 01-01
provides:
  - Immutable evidence revision and record-chain contract
  - Deterministic public-baseline and repository-registry diff contract
  - Revisioned exact-path legacy-authority classification contract
  - Non-authoritative canonical-head selector with explicit evaluation time
affects:
  - 01-03-governance-fixtures
  - 01-05-integrity-mutations
  - 01-06-semantic-validator
  - 01-10-production-governance
  - 01-11-generated-views

tech-stack:
  added: []
  patterns:
    - Closed draft-2020-12 metadata schemas with local references only
    - Genesis/successor ancestry as genesis reason XOR complete predecessor identity triple
    - Selector bindings carry identity, repository-relative path, and SHA-256 without copied status

key-files:
  created:
    - docs/schemas/kiana-capability-evidence-index.v1.schema.json
    - docs/schemas/kiana-capability-governance-diff.v1.schema.json
    - docs/schemas/kiana-legacy-authority-classification.v1.schema.json
    - docs/schemas/kiana-capability-governance-bundle.v1.schema.json
  modified: []

key-decisions:
  - "Evidence and legacy histories reuse the exact Plan 01-01 ancestry field names and require the full predecessor triple for every successor."
  - "Diff family values match the downstream producer CLI exactly: public-baseline and repository-registry."
  - "The bundle exposes evidence_head to match the production ancestry-traversal contract and keeps evaluation_time at the closed root as checked-in validation configuration."

patterns-established:
  - "Metadata-only diff: from/to IDs and hashes plus unique added, removed, and changed ID arrays; ordering remains a semantic validator check."
  - "Legacy retirement: every inventoried path is hash-bound to one classification, rationale, replacement view, evidence set, and review revision."
  - "Canonical selection: official artifact uses artifact_id while revision heads use revision_id; every binding also requires path and sha256."

requirements-completed: [COD-01, DIF-11]

coverage:
  - id: D1
    description: "Evidence revisions require immutable ancestry, closed typed records, record hashes, and explicit stale, supersession, or proof-decrease events."
    requirement: COD-01
    verification:
      - kind: other
        ref: "01-02 Task 1 automated verification command"
        status: pass
      - kind: other
        ref: "Draft202012Validator evidence genesis/successor, record closure, and invalidation negative checks"
        status: pass
    human_judgment: false
  - id: D2
    description: "Governance diffs bind exact from/to revision IDs and hashes and expose only unique ID-only change arrays for supported producer families."
    requirement: DIF-11
    verification:
      - kind: other
        ref: "01-02 Task 2 automated verification command"
        status: pass
      - kind: other
        ref: "Draft202012Validator diff family, duplicate ID, hash, and extra-field negative checks"
        status: pass
    human_judgment: false
  - id: D3
    description: "Legacy-authority revisions bind every entry path and content hash to one classification, rationale, replacement view, evidence set, and review revision."
    requirement: DIF-11
    verification:
      - kind: other
        ref: "01-02 Task 2 automated verification command"
        status: pass
      - kind: other
        ref: "Draft202012Validator legacy ancestry, missing hash, empty evidence, path, and closure negative checks"
        status: pass
    human_judgment: false
  - id: D4
    description: "The closed bundle selects the official artifact plus five revision heads by ID, path, and hash and pins deterministic RFC3339 evaluation configuration."
    requirement: COD-01
    verification:
      - kind: other
        ref: "01-02 Task 2 automated verification command"
        status: pass
      - kind: other
        ref: "Draft202012Validator bundle binding, forbidden state, repository output, hash, ID-kind, and time negative checks"
        status: pass
    human_judgment: false

duration: 39 min
completed: 2026-07-15
status: complete
---

# Phase 01 Plan 02: Evidence and Governance Metadata Contracts Summary

**四份 closed JSON Schema 冻结了 evidence history、derived diff、legacy-authority inventory 与 canonical-head selection，并把确定性 expiry evaluation 保持为非完成状态的显式配置。**

## Performance

- **Duration:** 39 min
- **Started:** 2026-07-15T08:46:01Z
- **Completed:** 2026-07-15T09:25:30Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- 建立 append-only evidence revision contract：coverage、proof、freshness 分轴，records 绑定 source/target/environment/artifact，并保留 stale、supersession 与 proof-decrease 事件。
- 建立仅含精确 from/to revision identity 和三组 ID-only changes 的 deterministic diff contract，family 与后续 producer CLI 一致。
- 建立带 mandatory ancestry 的 legacy-authority contract，每个既有状态文档都必须按 path/hash/classification/rationale/replacement/evidence/review 归档。
- 建立 path/hash-only canonical selector，显式选择 official artifact、四个 D-20 family heads 与 legacy head，并固定根层 `evaluation_time`。

## Task Commits

Each task was committed atomically:

1. **Task 1: Freeze immutable evidence revisions and mandatory ancestry** - `e839a33` (test)
2. **Task 2: Freeze diff, legacy-authority, and canonical-head metadata contracts** - `9014ca8` (test)

**Plan metadata:** committed with this SUMMARY and the GSD tracking updates.

## Files Created/Modified

- `docs/schemas/kiana-capability-evidence-index.v1.schema.json` - Immutable evidence revisions, typed records, record hashes, and invalidation events.
- `docs/schemas/kiana-capability-governance-diff.v1.schema.json` - Deterministic from/to bindings and unique ID-only change sets.
- `docs/schemas/kiana-legacy-authority-classification.v1.schema.json` - Revisioned exact-path legacy classification and replacement contract.
- `docs/schemas/kiana-capability-governance-bundle.v1.schema.json` - Closed artifact/head selector with deterministic evaluation configuration.

## Decisions Made

- `evidence_head` is the selector key because Plan 01-10 names that exact production ancestry-traversal pattern.
- Diff families are restricted to `public-baseline|repository-registry`, the exact interface Plan 01-11 generates and checks.
- `evaluation_time` accepts a checked-in UTC RFC3339 instant and is not a generated timestamp or status projection.
- JSON Schema enforces unique diff IDs; lexicographic order and cross-file path/hash equality remain explicit semantic checks for later validator plans.

## TDD Evidence

- Task 1 RED failed with the expected `FileNotFoundError` before its schema existed. GREEN passed the original plan command, draft-2020-12 meta-schema validation, positive genesis/successor instances, and ancestry/record/invalidation negatives.
- Task 2 RED failed with the expected three missing-file errors before its schemas existed. GREEN passed the original plan command, all three meta-schemas, positive genesis/successor/bundle instances, and diff/legacy/bundle closure and binding negatives.
- Final plan verification re-ran both original task commands plus all four meta-schemas, 6 positive instances, 16 negative mutations, closed-object checks, local-reference checks, whitespace checks, and stub scans from committed content.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Normalized GSD performance tracking output**
- **Found during:** Plan metadata update
- **Issue:** `state.record-metric` appended the Plan 01-02 row outside the Plan History table and left the velocity, phase totals, and progress display at their Plan 01-01 values; `roadmap.update-plan-progress` also introduced broad Markdown spacing churn.
- **Fix:** Moved the metric row into the existing table, recalculated the 2-plan totals and 17% progress, removed formatter-only ROADMAP churn, and retained only the intended 01-02 checkbox/count updates.
- **Files modified:** `.planning/STATE.md`, `.planning/ROADMAP.md`
- **Verification:** Focused tracking diff, table inspection, STATE line-count check, and `git diff --check`.
- **Committed in:** Plan metadata commit.

---

**Total deviations:** 1 auto-fixed (1 Rule 1 bug).
**Impact on plan:** Tracking metadata accurately reflects the completed plan; schema scope and both task commits are unchanged.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plan 01-03 can build valid fixture bundles against stable evidence, diff, legacy, and selector field names.
- Plans 01-05/01-06 can add cross-revision prefix, sorted-ID, reference, expiry, and raw-byte hash semantic oracles without changing these structural contracts.
- Plans 01-10/01-11 can use `evidence_head`, root `evaluation_time`, and the two exact diff family values without adapter aliases.
- No package, dependency, Rust adapter, database, validation script, or dirty legacy document changed in this plan.

---
*Phase: 01-baseline-evidence-governance*
*Completed: 2026-07-15*

## Self-Check: PASSED

- All four key schema files exist and pass their original task verification commands.
- Task commits `e839a33` and `9014ca8` exist and contain only their planned schema paths.
- Coverage metadata maps all four shipped deliverables to fresh passing automated verification.
- The task commits add 956 schema lines with no deletions and introduce no dependency or executable surface.
