---
phase: 01-baseline-evidence-governance
plan: "01"
subsystem: governance-contracts
tags: [json-schema, public-parity, reference-governance, evidence]

requires: []
provides:
  - Controlled official-public source artifact contract
  - Public-parity journey, capability, and source-index contract
  - Frozen 38-reference repository identity and alias contract
  - Capability-level Adopt/Adapt/Reject review contract
affects:
  - 01-02-evidence-contract
  - 01-03-governance-fixtures
  - 01-06-semantic-validator
  - 01-08-public-baseline-data
  - 01-09-reference-governance-data

tech-stack:
  added: []
  patterns:
    - Closed draft-2020-12 JSON schemas with stable instance schema IDs
    - Canonical genesis/successor ancestry with predecessor path and SHA-256 binding
    - Structural contracts separated from later cross-file semantic validation

key-files:
  created:
    - docs/schemas/kiana-official-source-artifact.v1.schema.json
    - docs/schemas/kiana-public-parity-baseline.v1.schema.json
    - docs/schemas/kiana-reference-repository-registry.v1.schema.json
    - docs/schemas/kiana-capability-decisions.v1.schema.json
  modified: []

key-decisions:
  - "Official-source coverage remains inside the public-baseline family through an exclusive capability mapping or reviewed exclusion."
  - "Repository domains remain imported classification labels, aliases use one exact seven-key shape, and review decisions remain capability-level."
  - "Current Adopt/Adapt records fail closed unless license, security, target revision, tests, and evidence bindings are explicit."

patterns-established:
  - "Canonical ancestry: revision_id plus revision_kind, with genesis_reason XOR the complete predecessor triple."
  - "Governance/product separation: registry and decision contracts contain no repository-wide verdict or product-completion field."

requirements-completed: [COD-01, DIF-11]

coverage:
  - id: D1
    description: "受控 official-public source artifact schema，绑定官方 URI、不可变 archive、normalization、content hash 和稳定 entry IDs。"
    requirement: COD-01
    verification:
      - kind: other
        ref: "python3 -m json.tool docs/schemas/kiana-official-source-artifact.v1.schema.json"
        status: pass
      - kind: other
        ref: "Draft202012Validator genesis/successor and closed-object contract check"
        status: pass
    human_judgment: false
  - id: D2
    description: "Public-parity schema 将 15 个 journeys、capability proof axes 与 exhaustive official_source_index 保持在同一 canonical family。"
    requirement: COD-01
    verification:
      - kind: other
        ref: "01-01 Task 1 automated verification command"
        status: pass
      - kind: other
        ref: "Draft202012Validator exclusive mapping and fifteen-journey negative checks"
        status: pass
    human_judgment: false
  - id: D3
    description: "38-reference registry schema 区分 Git/content-tree identity，并冻结 license fingerprints、domains 与 exact aliases。"
    requirement: DIF-11
    verification:
      - kind: other
        ref: "01-01 Task 2 automated verification command"
        status: pass
      - kind: other
        ref: "Draft202012Validator git/content, ancestry, and alias negative checks"
        status: pass
    human_judgment: false
  - id: D4
    description: "Capability-level decision schema 绑定 source/target revisions、license/security、owner、risk、tests、evidence 和 review freshness。"
    requirement: DIF-11
    verification:
      - kind: other
        ref: "01-01 Task 2 automated verification command"
        status: pass
      - kind: other
        ref: "Draft202012Validator current Adopt license/security/target/test negative checks"
        status: pass
    human_judgment: false

duration: 22 min
completed: 2026-07-15
status: complete
---

# Phase 01 Plan 01: Baseline Evidence Governance Contracts Summary

**四份 closed JSON Schema 冻结了 official source、public parity、38-reference identity 与 capability review 的唯一字段合同，并以 immutable ancestry 和 fail-closed revision bindings 防止治理状态冒充产品完成。**

## Performance

- **Duration:** 22 min
- **Started:** 2026-07-15T08:06:54Z
- **Completed:** 2026-07-15T08:28:59Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- 建立受控 official-public source artifact 与 public-parity sibling contracts，source entry 必须映射到非空 capability IDs 或带 evidence/review 的排除项。
- 建立 38-reference registry contract，Git HEAD、normalized tree、license fingerprint、domains 和七键 alias history 各自具有明确结构。
- 建立 capability-level Adopt/Adapt/Reject contract；current Adopt/Adapt 必须通过 compatible license、approved security review、target revision、tests 与 evidence gates。
- 四个 canonical roots 统一使用 `revision_id`、`revision_kind` 以及互斥的 genesis/successor ancestry 字段。

## Task Commits

Each task was committed atomically:

1. **Task 1: Freeze controlled official-source and public-baseline contracts** - `9b8452b` (test)
2. **Task 2: Freeze repository identity and capability-decision contracts** - `a702d9f` (test)

**Plan metadata:** committed with this SUMMARY and the GSD tracking updates.

## Files Created/Modified

- `docs/schemas/kiana-official-source-artifact.v1.schema.json` - 官方公开来源的受控 normalization、archive、hash 与 entry identity contract。
- `docs/schemas/kiana-public-parity-baseline.v1.schema.json` - Public journeys、capabilities、source artifact binding 与 exclusive source index contract。
- `docs/schemas/kiana-reference-repository-registry.v1.schema.json` - 38-reference identity、revision、license、domains 与 alias history contract。
- `docs/schemas/kiana-capability-decisions.v1.schema.json` - Capability-level source/target/license/security/risk/test/evidence review contract。

## Decisions Made

- `official_source_index` 的排除分类固定为 `not_public_contract|duplicate_navigation|non_capability_metadata|out_of_scope_product`，并预留 `missing_required_capability`、`unmapped_source_entry`、`duplicate_source_mapping`、`unjustified_exclusion` 四个后续 semantic oracle 名称。
- `domains` 只保存导入的 compatibility labels；它不能表示 proof、decision 或 completion。
- 所有 decision 都绑定 Kiana target revision 与 tests；current Adopt/Adapt 进一步要求 `license_compatibility=compatible` 和 approved security review。

## TDD Evidence

- Task 1 RED 在 schema 创建前以预期的 `FileNotFoundError` 失败；创建合同后，原计划命令、draft-2020-12 meta-schema 与正负实例检查全部 GREEN。
- Task 2 RED 同样在目标合同不存在时按预期失败；实现后，原计划命令以及 ancestry、Git/content、alias、target/license/test guards 的正负实例检查全部 GREEN。
- Plan frontmatter 为 `type: execute` 且 `workflow.tdd_mode=false`；schema 本身是本计划的 contract-test artifact，因此遵循计划规定的每任务一个 `test(01-01)` 原子提交。

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Normalized GSD performance tracking output**
- **Found during:** Plan metadata update
- **Issue:** `state.record-metric` appended a four-column metric row outside the existing custom Performance Metrics tables, leaving the velocity summaries at zero and producing malformed tracking Markdown.
- **Fix:** Updated the existing velocity/by-phase values, moved the task/file metric into a matching Plan History table, and normalized the ROADMAP progress-cell spacing without touching product files.
- **Files modified:** `.planning/STATE.md`, `.planning/ROADMAP.md`
- **Verification:** `state.load`, focused ROADMAP/STATE inspection, and `git diff --check`.
- **Committed in:** Plan metadata commit.

---

**Total deviations:** 1 auto-fixed (1 Rule 1 bug).
**Impact on plan:** Tracking metadata now reflects the completed plan; implementation scope and task commits are unchanged.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plan 01-02 可以复用相同 ancestry 与 closed-object pattern 创建 evidence contract。
- Plans 01-03/01-06/01-08/01-09 可以直接消费已冻结的 source、public、registry 与 decision field names。
- Structural schema success 仍不代表 semantic ledger completion；cross-file uniqueness、coverage、freshness 与 hash equality 保留给后续 validator plans。

---
*Phase: 01-baseline-evidence-governance*
*Completed: 2026-07-15*

## Self-Check: PASSED

- All four key schema files exist.
- Task commits `9b8452b` and `a702d9f` exist and contain only their planned files.
- Coverage classification reports 4/4 deliverables as auto-covered with passing verification.
