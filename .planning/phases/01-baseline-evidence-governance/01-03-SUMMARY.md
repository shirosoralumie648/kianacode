---
phase: 01-baseline-evidence-governance
plan: "03"
subsystem: governance-testing
tags: [bash, python, json-schema, fixtures, offline-validation, tdd]

requires:
  - phase: 01-baseline-evidence-governance
    provides: Eight closed governance schemas and immutable ancestry contracts from Plans 01-01 and 01-02
provides:
  - Offline named runner slices for all eight governance contracts and fixture-shape checks
  - Complete cross-family genesis/successor graph with canonical predecessor and head hashes
  - Exact 38-reference identity, alias, domain, and capability-decision oracle
  - Hostile-rendering and deterministic offline-source identity fixtures
affects:
  - 01-04-negative-coverage-fixtures
  - 01-05-integrity-and-security-fixtures
  - 01-06-semantic-validator
  - 01-07-drift-refresh
  - 01-10-legacy-authority
  - 01-11-generated-views

tech-stack:
  added: []
  patterns:
    - Bash 5 named slices backed only by Python standard-library JSON and the existing structural validator
    - Canonical fixture hashes use sorted compact UTF-8 JSON and path/hash-only manifest bindings
    - Positive security fixtures separate hostile data from executable markup and embed normalized source bytes as Base64

key-files:
  created:
    - scripts/capability-governance-smoke.sh
    - scripts/fixtures/capability-governance/valid/minimal-graph.json
    - scripts/fixtures/capability-governance/valid/full-38-repositories.json
    - scripts/fixtures/capability-governance/valid/hostile-rendering.json
    - scripts/fixtures/capability-governance/valid/offline-source-identity.json
  modified: []

key-decisions:
  - "Fixture manifests bind embedded objects through canonical sorted compact JSON SHA-256 values, so ancestry and selected heads can be verified without production files."
  - "The minimal graph keeps capability data small but carries 38 synthetic repository rows because the frozen registry schema itself requires exactly 38 entries."
  - "Full-reference fingerprints remain explicit fixture-only values; only IDs, live paths, imported domains, aliases, and decision coverage are reconciled to the checked-in seed."
  - "Offline source bytes use inline-base64 entry locators and deterministic LF joining, allowing entry and artifact identities to be rehashed with no retrieval."

patterns-established:
  - "Progressive runner: later plans extend the same two named slices while missing owned fixtures fail with a stable fixture_required diagnostic."
  - "Exact fixture bundles: seven top-level keys, closed head bindings, mandatory ancestry, unique identities, and exclusive source mapping or reviewed exclusion."

requirements-completed: [COD-01, DIF-11]

coverage:
  - id: D1
    description: "Offline runner parses all eight Phase 1 schemas and structurally validates every applicable fixture object and derived diff."
    requirement: COD-01
    verification:
      - kind: integration
        ref: "bash scripts/capability-governance-smoke.sh schemas"
        status: pass
    human_judgment: false
  - id: D2
    description: "Minimal graph preserves mandatory genesis/successor ancestry and canonical source, predecessor, and manifest hashes across all five revision families."
    requirement: COD-01
    verification:
      - kind: integration
        ref: "bash scripts/capability-governance-smoke.sh fixture-shapes"
        status: pass
    human_judgment: false
  - id: D3
    description: "Full fixture reconciles 38 unique normalized IDs and live paths, exact legacy aliases/domains, and one current decision per inventory capability."
    requirement: DIF-11
    verification:
      - kind: integration
        ref: "Task 2 exact Python fixture contract command plus fixture-shapes slice"
        status: pass
    human_judgment: false
  - id: D4
    description: "Hostile rendering and inline normalized source fixtures prove safe synthetic text and offline reproducible entry/artifact identity."
    requirement: COD-01
    verification:
      - kind: integration
        ref: "Task 3 JSON parse command plus fixture-shapes slice"
        status: pass
    human_judgment: false

duration: 28 min
completed: 2026-07-15
status: complete
---

# Phase 01 Plan 03: Offline Governance Fixture Runner Summary

**Bash/Python 离线 runner 现在能在 2 秒级完成八份治理合同、38-reference identity、跨族 ancestry、hostile rendering 与 normalized source bytes 的正例采样。**

## Performance

- **Duration:** 28 min
- **Started:** 2026-07-15T09:54:32Z
- **Completed:** 2026-07-15T10:23:08Z
- **Tasks:** 3
- **Files modified:** 5

## Accomplishments

- 建立仅暴露 `schemas` 与 `fixture-shapes` 的 Bash 5 runner；缺失或未知 slice 返回 usage 退出码 2，成功输出 measured elapsed seconds 和 `offline=true`。
- 建立 complete minimal graph：public baseline、registry、decisions、evidence 与 legacy 均含 genesis/successor，predecessor 与 manifest head SHA-256 可从嵌入对象离线复算。
- 建立 38-reference full oracle：normalized stable IDs、38 个 live paths、seed domains、76 个七键 aliases，以及每个 inventory capability 的唯一 current Adopt/Adapt/Reject decision。
- 建立 hostile-rendering 和 offline-source fixtures：前者包含 Markdown/括号/HTML-like/newline 数据但无 executable markup、secret 或本地绝对路径，后者可从 inline Base64 bytes 重算 entry/content/artifact hashes。

## Task Commits

每个 TDD task 均以独立 RED/GREEN 提交落盘：

1. **Task 1 RED: Offline runner assertions** - `35784af` (test)
2. **Task 1 GREEN: Complete cross-family graph** - `33f9782` (feat)
3. **Task 2 RED: 38-reference identity and decision assertions** - `82b6897` (test)
4. **Task 2 GREEN: Full 38-reference valid fixture** - `c9fe5ce` (feat)
5. **Task 3 RED: Hostile and offline-source assertions** - `8949fb0` (test)
6. **Task 3 GREEN: Rendering and source identity fixtures** - `1ad04e1` (feat)

**Plan metadata:** 与本 SUMMARY 及 GSD tracking 更新单独提交。

## Files Created/Modified

- `scripts/capability-governance-smoke.sh` - 两个稳定 named slices、离线环境、结构提取、fixture semantic shape 与 30 秒预算检查。
- `scripts/fixtures/capability-governance/valid/minimal-graph.json` - 五个 revision family 的完整 ancestry 和 path/hash-only current selector。
- `scripts/fixtures/capability-governance/valid/full-38-repositories.json` - 38 个 seed/live identities、兼容 aliases、imported domains 与 capability decisions。
- `scripts/fixtures/capability-governance/valid/hostile-rendering.json` - 后续 renderer escaping 的 hostile-but-valid 正例数据。
- `scripts/fixtures/capability-governance/valid/offline-source-identity.json` - 可离线解码、逐 entry 与整体复算的 controlled official-source artifact。

## Decisions Made

- 使用 canonical sorted compact JSON 作为 fixture 内部 hash 计算口径；这只是测试身份口径，不新增 production authority。
- `minimal-graph` 的 capabilities 保持小型，但 registry 仍采用 38 行，以直接满足已冻结 schema 的 `minItems/maxItems=38` 合同。
- Full fixture 的 fingerprint 均标识为 synthetic fixture values；不把本计划的正例数据表述为 license、source 或 product completion 证据。
- Hostile fixture 只使用不可执行的 HTML-like 标签；offline fixture 采用 `inline-base64-entry-lf-join` 使 Plan 01-07 能冻结并复算同一字节表示。

## TDD Evidence

- Task 1 RED 在 runner 断言已存在而 `minimal-graph.json` 尚不存在时，以 `fixture_required` 和退出码 1 失败；GREEN 后 `schemas=0.896s`、`fixture-shapes=0.055s`。
- Task 2 RED 在 minimal graph 继续通过后，以缺失 `full-38-repositories.json` 失败；GREEN 后 seed/live/alias/decision checks 通过，schema slice 为 `1.240s`。
- Task 3 RED 在前两组 fixture 继续通过后，以缺失 `hostile-rendering.json` 失败；GREEN 后安全文本与离线字节复算通过。
- 最终新鲜门禁结果：`schemas=1.895s`、`fixture-shapes=0.143s`，均远低于 30 秒且未调用 Cargo、网络或包管理器。

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Normalized GSD tracking output**
- **Found during:** Plan metadata update
- **Issue:** `state.update-progress` wrote frontmatter `percent: 0` while leaving the body at 17%; `state.record-metric` appended its row outside the Plan History table; `roadmap.update-plan-progress` introduced repository-wide Markdown spacing churn.
- **Fix:** Recalculated 3-plan velocity/progress values, moved the 01-03 metric into Plan History, restored ROADMAP formatting from the clean pre-plan version, and retained only the intended 01-03 checkbox/count updates.
- **Files modified:** `.planning/STATE.md`, `.planning/ROADMAP.md`
- **Verification:** Focused tracking diff, `state.load`, line-count check, and `git diff --check`.
- **Committed in:** Plan metadata commit.

---

**Total deviations:** 1 auto-fixed (1 Rule 1 bug).
**Impact on plan:** Tracking metadata now reflects Plan 01-03 without formatter-only churn; fixture scope and task commits are unchanged.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Known Stubs

None. Fixture-only hashes and decisions are deliberately synthetic test data, are explicitly labeled as such, and do not block this plan's positive-oracle goal.

## Next Phase Readiness

- Plans 01-04/01-05 可以在同一 runner 上加入 coverage、integrity、drift 与 security negative corpora。
- Plan 01-06 可以直接消费四份 valid bundle 作为 semantic validator 的 GREEN oracles，并保留当前 stable slice names。
- Plan 01-07 可以使用 `inline-base64-entry-lf-join` 重算 controlled source bytes，无需网络 retrieval。
- `scripts/schema-contract-smoke.sh` 的既有 dirty hunk 保持 byte-identical，直到 Plan 01-12 执行既定 broad-gate integration。

---
*Phase: 01-baseline-evidence-governance*
*Completed: 2026-07-15*

## Self-Check: PASSED

- All five planned files and this SUMMARY exist.
- RED commits `35784af`, `82b6897`, and `8949fb0` precede GREEN commits `33f9782`, `c9fe5ce`, and `1ad04e1` for their respective tasks.
- Every task commit contains only Plan 01-03 declared paths; no tracked file deletion occurred.
- Coverage classification reports 4/4 deliverables as auto-covered with passing verification.
- Protected `scripts/schema-contract-smoke.sh` and `.planning/config.json` retain their pre-execution working-tree hashes.
