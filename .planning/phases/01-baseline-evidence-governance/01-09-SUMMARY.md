---
phase: 01-baseline-evidence-governance
plan: "09"
subsystem: reference-governance
tags: [reference-registry, licensing, adopt-adapt-reject, immutable-history, fingerprints]

requires:
  - phase: 01-baseline-evidence-governance
    provides: Governance schemas, ancestry validation, source identity, and reviewed public-baseline authority through Plan 01-08
provides:
  - Immutable genesis/reviewed registry revisions for all 38 live reference identities
  - Real Git/content tree and top-level license fingerprints with explicit compatibility review
  - 76 capability-level decisions separating borrowable mechanisms from explicit rejected boundaries
affects: [01-10-evidence-selector, 01-11-generated-views, 01-12-aggregate-gate, reference-driven-planning]

tech-stack:
  added: []
  patterns:
    - Each reference owns a mechanism capability and a separate negative-boundary capability
    - License uncertainty or restriction rejects code reuse instead of weakening current Adapt validation
    - Target bindings use actual artifact SHA-256 so dirty/untracked local implementation is not misrepresented by Git HEAD

key-files:
  created:
    - docs/agent-program/kiana-completion/governance/repository-registry/references-2026-07-15-genesis.json
    - docs/agent-program/kiana-completion/governance/repository-registry/references-2026-07-15.json
    - docs/agent-program/kiana-completion/governance/capability-decisions/review-2026-07-15-genesis.json
    - docs/agent-program/kiana-completion/governance/capability-decisions/review-2026-07-15.json
  modified: []

key-decisions:
  - "Every repository contributes separate mechanisms and boundary capability IDs, preventing one repository-level verdict from hiding mixed decisions."
  - "Only compatible MIT, Apache-2.0, or ISC sources receive current Adapt; PolyForm, mixed licenses, AGPL/commercial dual licensing, and missing licenses reject code reuse."
  - "Git-backed repositories bind actual HEAD plus normalized content SHA-256; the two proprietary content snapshots use content_tree_sha256 and never claim Git identity."

patterns-established:
  - "Reference review chain: seed identity -> immutable import -> license/security/target/test review -> current capability decision."
  - "Governance evidence proves a reviewed borrowing decision, never implementation or product completion."

requirements-completed: [COD-01, DIF-11]

coverage:
  - id: D1
    description: "The reviewed registry contains 38 unique live paths/IDs with real revision, tree, license, domain, alias, availability, and freshness fields."
    requirement: DIF-11
    verification:
      - kind: integration
        ref: "01-09 schema/history/38-identity automated command"
        status: pass
      - kind: other
        ref: "read-only source/license/target fingerprint closure assertions"
        status: pass
    human_judgment: false
  - id: D2
    description: "All 76 inventory capabilities have exactly one current decision, including 31 compatible Adapt records and 45 explicit Reject records."
    requirement: DIF-11
    verification:
      - kind: integration
        ref: "01-09 inventory-decision closure assertion"
        status: pass
      - kind: integration
        ref: "bash scripts/capability-governance-smoke.sh reference-governance"
        status: pass
    human_judgment: false
  - id: D3
    description: "License, security, risk, target, test, and evidence classifications preserve clean-room and non-completion boundaries for every reference."
    requirement: DIF-11
    verification:
      - kind: other
        ref: "01-09 current Adapt/Reject semantic assertions"
        status: pass
    human_judgment: true
    rationale: "Automation proves field and policy consistency; maintainers must judge the source-specific license interpretation and borrowing rationale."

duration: 14min
completed: 2026-07-17
status: complete
---

# Phase 01 Plan 09: Reference Registry And Decisions Summary

**A 38-repository immutable registry and 76-row capability review that separates compatible adaptations from explicit license and product-boundary rejections**

## Performance

- **Duration:** 14 min
- **Started:** 2026-07-17T11:11:00Z
- **Completed:** 2026-07-17T11:25:16Z
- **Tasks:** 1
- **Files modified:** 4

## Accomplishments

- 为 36 个 Git-backed repositories 保存实际 HEAD，并为两个 proprietary content snapshots 保存 `content_tree_sha256`；全部 38 个目录另有排序 path/content tree SHA-256。
- 审查 top-level license identity：32 found、3 missing、3 mixed/conflicting；PolyForm 单独记录为 found-but-incompatible。
- 每仓库拆分 `mechanisms` 和 `boundary` 两条 inventory capability，共 76 条 current decisions：31 Adapt、45 Reject。
- 每条 current Adapt 均绑定 compatible license、approved source-level security review、实际 target artifact hash 和 passing governance-contract test；所有 Reject 保留明确 reason。

## Task Commits

1. **Task 1: Freeze 38 repository and decision revision chains** - `546b7fe`

## Files Created/Modified

- `docs/agent-program/kiana-completion/governance/repository-registry/references-2026-07-15-genesis.json` - 38-reference immutable import revision。
- `docs/agent-program/kiana-completion/governance/repository-registry/references-2026-07-15.json` - reviewed identity/license successor。
- `docs/agent-program/kiana-completion/governance/capability-decisions/review-2026-07-15-genesis.json` - 76 candidate decisions in superseded import state。
- `docs/agent-program/kiana-completion/governance/capability-decisions/review-2026-07-15.json` - 76 reviewed current decisions with source/target/license/security/test bindings。

## Decisions Made

- 使用两条 capability per repository，确保可借鉴机制与明确边界能分别 Adapt/Reject。
- 混合、限制性或缺失许可不进入 current Adapt；它们保留 discovery provenance 并 Reject code reuse。
- target revision 绑定当前文件 artifact SHA-256，而不是用无法覆盖 dirty/untracked bytes 的仓库 HEAD 冒充。

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

- Production `repository_fingerprint()` 对单文件超过 16 MiB 的 GitNexus、autogen、continue 和 gstack fail closed。此次受控冻结使用相同的排序 relative-path + NUL + content 算法分块读取已知 `reference/` 输入，未放宽 production bounded-read 安全合同。Registry identity 可离线复算，但当前 live `check-drift` 对这些仓库仍会报告 source unavailable，必须在后续计划中明确可信大文件观察合同。

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plan 01-10 可为 public、registry、decision 和 alias/license/security review IDs 创建追加式 evidence records，并发布 closed path/hash-only selector。
- 四个只读 seed/audit/fixture 输入的 SHA-256 与计划开始时完全一致。
- 大文件 live drift 限制保留为 Phase 1 concern，不能用本次手工冻结结果假装持续监控已闭合。

---
*Phase: 01-baseline-evidence-governance*
*Completed: 2026-07-17*
