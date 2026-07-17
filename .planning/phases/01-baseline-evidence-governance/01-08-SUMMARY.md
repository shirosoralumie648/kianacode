---
phase: 01-baseline-evidence-governance
plan: "08"
subsystem: public-baseline-governance
tags: [official-source, public-parity, claude-code, immutable-history, proof-boundaries, offline]

requires:
  - phase: 01-baseline-evidence-governance
    provides: Versioned schemas, semantic history validation, offline identity rehashing, and immutable refresh workflow from Plans 01-01 through 01-07
provides:
  - Controlled offline-rehashable Claude Code official public source artifact with honest retrieval and archive identity
  - Immutable public-parity genesis and reviewed successor covering exactly 15 journeys and 49 required capability children
  - Exhaustive source-entry mapping, explicit source-version conflict, proof-level caps, owners, destination phases, gaps, and differences
affects: [01-09-reference-governance, 01-10-evidence-selector, 01-11-generated-views, phase-10-coding-baseline, phase-11-coding-ecosystem]

tech-stack:
  added: []
  patterns:
    - Controlled public bytes are stored as inline Base64 entries whose per-entry and LF-joined SHA-256 identities rehash offline
    - Baseline filenames may retain an approved planning label while retrieved_at records the later real observation time without backdating claims
    - Journey completion is derived only from independently governed required children and never from a journey-level percentage
    - Public source conflicts remain blocked capability rows until matching-version evidence resolves them

key-files:
  created:
    - docs/agent-program/kiana-completion/governance/README.md
    - docs/agent-program/kiana-completion/governance/source-artifacts/claude-code-public-2026-07-15.json
    - docs/agent-program/kiana-completion/governance/public-baselines/cc-public-2026-07-15-genesis.json
    - docs/agent-program/kiana-completion/governance/public-baselines/cc-public-2026-07-15.json
  modified: []

key-decisions:
  - "The approved 2026-07-15 baseline label is retained, but retrieval is truthfully recorded as 2026-07-17 and no current rolling page is claimed to be a July 15 byte snapshot."
  - "The source artifact uses four controlled entries: official docs index, commit-bound release notes, public CLI help/schema, and support/deployment declarations."
  - "The public ledger uses exactly 15 COD-01 journeys and 49 stable cc.* required children; Phase 1 proof remains source-only even where Kiana implementation entry points exist."
  - "Release-notes 2.1.211 versus local CLI help 2.1.209 is preserved as blocked cc.cli.forward-subagent-text rather than silently selecting either source."

patterns-established:
  - "Source import chain: controlled public bytes -> offline identity -> genesis import -> reviewed successor -> later evidence-index binding."
  - "Honest parity row: coverage state, parity outcome, required/current proof, freshness, evidence ID, owner, phase, gap, and difference remain separate fields."

requirements-completed: [COD-01, DIF-11]

coverage:
  - id: D1
    description: "The controlled official artifact binds four public source groups to genuine official/archive URIs, retrieval/product metadata, per-entry hashes, and an offline-rehashable whole-content hash."
    requirement: COD-01
    verification:
      - kind: integration
        ref: "01-08 Task 1 automated schema/freeze/unique-entry command"
        status: pass
      - kind: other
        ref: "post-commit offline Base64 entry/content rehash assertion"
        status: pass
    human_judgment: false
  - id: D2
    description: "Genesis and reviewed public heads traverse exact predecessor bytes and expose 15 journeys whose 49 required children all resolve and map to the frozen source set."
    requirement: COD-01
    verification:
      - kind: integration
        ref: "01-08 Task 2 schema/history/source-index command"
        status: pass
      - kind: other
        ref: "post-commit 15-journey/49-child/source-mapping semantic assertion"
        status: pass
    human_judgment: false
  - id: D3
    description: "The reviewed ledger separates implemented, partial, missing, and blocked observations from proof, records a real source-version conflict, and produces zero false completed journeys."
    requirement: COD-01
    verification:
      - kind: other
        ref: "post-commit proof-cap/conflict/zero-complete semantic assertion"
        status: pass
    human_judgment: true
    rationale: "Automation proves contract shape and conservative proof bounds; maintainers must still judge whether the 49 capability decompositions adequately interpret the captured public material."

duration: 16min
completed: 2026-07-17
status: complete
---

# Phase 01 Plan 08: Official Public Baseline Summary

**A content-bound Claude Code public artifact and immutable 15-journey/49-capability parity chain with source-only proof and an explicit version-conflict blocker**

## Performance

- **Duration:** 16 min
- **Started:** 2026-07-17T10:51:49Z
- **Completed:** 2026-07-17T11:08:11Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- 冻结四组官方公开材料，逐条保存 inline Base64 bytes 和 SHA-256，并明确区分 `2026-07-15` 基线标签与 `2026-07-17` 实际检索时间。
- 建立 source-complete genesis/reviewed revision chain：15 个 COD-01 journeys、49 个独立 required capability rows、4/4 source entries 唯一映射和真实 predecessor hash。
- reviewed head 保留 `19 implemented / 22 partial / 7 missing / 1 blocked` 的低等级观察，全部 proof 为 `source`，因此没有 journey 被虚假标记完成。
- 将 release notes 2.1.211 新增 flag 与本地 CLI help 2.1.209 的版本偏差显式记录为 blocked row，等待匹配版本重新采集。

## Task Commits

Each task was committed atomically:

1. **Task 1: Freeze the controlled official source artifact and authority contract** - `9b7abba`
2. **Task 2: Create genesis and reviewed public-parity revisions** - `91f568b`

## Files Created/Modified

- `docs/agent-program/kiana-completion/governance/README.md` - 定义四类 canonical family、selector、不可变工作流、离线命令和 proof 边界。
- `docs/agent-program/kiana-completion/governance/source-artifacts/claude-code-public-2026-07-15.json` - 保存官方 docs index、release notes、CLI help/schema 和 support/deployment 的受控字节与 hash。
- `docs/agent-program/kiana-completion/governance/public-baselines/cc-public-2026-07-15-genesis.json` - 保存 source-complete 但尚未解释的 immutable import revision。
- `docs/agent-program/kiana-completion/governance/public-baselines/cc-public-2026-07-15.json` - 保存已复审的 capability classification、gap/difference、owner、destination phase 和 predecessor identity。

## Decisions Made

- 基线日期只作为批准的 snapshot label；`retrieved_at` 必须记录实际检索时间，禁止把滚动页面倒签为 7 月 15 日内容。
- `archive_uri` 使用 Anthropic 官方仓库真实不可变 commit；它锚定仓库/release-note revision，不冒充滚动 docs 的历史 Web Archive。
- 所有 public capability 的当前 proof 限制为 `source`；`implemented` 只表达观察到实现入口，不能替代 local behavior、target environment 或 user value evidence。
- CLI source version skew 作为独立 blocked child 保留，避免宽泛 flags 行隐藏未观察的新公开行为。

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

- 一次性采集脚本的 Python `urllib` TLS 握手返回 `SSL: UNEXPECTED_EOF_WHILE_READING`，且未写出目标 JSON。逐 URL 诊断确认 `curl` 对全部 9 个官方 URL 均返回 HTTP 200 后，改用带 bounded retry/timeout 的 `curl` 传输并保留同一规范化与 hash 算法；随后 Task 1 全部验收重新通过。

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plan 01-09 可直接消费本 summary，冻结 38/38 repository registry 与 capability-decision genesis/reviewed chains。
- Plan 01-10 仍需为本计划的 `ev.public.*` IDs 创建匹配的追加式 evidence records 和 path/hash-only selector；在此之前，baseline 本身不声称 evidence family 已闭合。
- 公开 capability 分解的语义完整性保留为后续 Phase 1 conversational UAT 项；所有机器可验证的结构、identity、ancestry 和 proof 上限已通过 fresh post-commit gate。

---
*Phase: 01-baseline-evidence-governance*
*Completed: 2026-07-17*
