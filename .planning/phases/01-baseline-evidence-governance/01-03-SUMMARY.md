---
phase: 01-baseline-evidence-governance
plan: "03"
subsystem: governance-testing
tags: [bash, python, jsonschema, json-schema, fixtures, evidence-ledger, fd-handshake, strace, watchdog, offline-validation, tdd]

requires:
  - phase: 01-baseline-evidence-governance
    provides: Eight closed governance schemas and immutable ancestry contracts from Plans 01-01 and 01-02
provides:
  - Offline named runner slices for all eight governance contracts and fixture-shape checks
  - Complete cross-family genesis/successor graph with canonical predecessor and head hashes
  - Exact 38-reference identity, alias, domain, and capability-decision oracle
  - Hostile-rendering and deterministic offline-source identity fixtures
  - Closed evidence heads with immutable record prefixes, hash chains, and exact cross-family bindings
  - Fail-closed 30-second watchdog and syscall-level network denial for both public slices
  - Seed-authoritative 38-reference validation that also runs from tracked snapshots without reference/
  - Full Draft 2020-12 schema validation and redacted runtime diagnostics
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
    - timeout supervises a strace worker that injects EPERM into every network syscall and rejects any non-empty trace
    - Draft202012Validator with FormatChecker runs after the existing lightweight validator for every extracted object

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
  - "Every current evidence head defines every referenced ID; successor heads preserve the complete prior record prefix and append newly bound records."
  - "Proxy variables are defense in depth only; timeout plus strace syscall injection is the fail-closed offline and deadline authority."
  - "A parent-generated nonce delivered through a one-use inherited anonymous FD authenticates workers; caller environment alone cannot select worker mode."
  - "The checked-in references.json catalog is authoritative; a live reference/ tree adds reconciliation only when present."

patterns-established:
  - "Progressive runner: later plans extend the same two named slices while missing owned fixtures fail with a stable fixture_required diagnostic."
  - "Exact fixture bundles: seven top-level keys, closed head bindings, mandatory ancestry, unique identities, and exclusive source mapping or reviewed exclusion."
  - "Evidence closure: references from capabilities, exclusions, aliases, licenses, security reviews, decisions, legacy entries, supersession, and transition events resolve against the selected head."
  - "Schema closure: the existing validator remains wired, followed by complete Draft 2020-12 keyword and format validation with stable keyword diagnostics."

requirements-completed: [COD-01, DIF-11]

coverage:
  - id: D1
    description: "Offline runner parses all eight Phase 1 schemas and validates every applicable fixture object and derived diff with both the existing validator and Draft202012Validator."
    requirement: COD-01
    verification:
      - kind: integration
        ref: "bash scripts/capability-governance-smoke.sh schemas"
        status: pass
    human_judgment: false
  - id: D2
    description: "Minimal graph preserves mandatory genesis/successor ancestry, immutable evidence prefixes, record hashes, exact bindings, and canonical source/predecessor/manifest hashes across all five revision families."
    requirement: COD-01
    verification:
      - kind: integration
        ref: "bash scripts/capability-governance-smoke.sh fixture-shapes"
        status: pass
    human_judgment: false
  - id: D3
    description: "Full fixture reconciles 38 unique normalized IDs and paths against the checked-in seed, optionally checks live paths, and preserves exact legacy aliases/domains plus one current decision per inventory capability."
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

**Bash/Python 离线 runner 现在能在 30 秒预算内完成八份治理合同、闭合 evidence ledger、38-reference identity、跨族 ancestry、hostile rendering 与 normalized source bytes 的正例采样。**

## Performance

- **Duration:** 28 min
- **Started:** 2026-07-15T09:54:32Z
- **Completed:** 2026-07-15T10:23:08Z
- **Tasks:** 3
- **Files modified:** 5
- **Review remediation:** 7 atomic test/fix commits across two reviews on 2026-07-15

## Accomplishments

- 建立仅暴露 `schemas` 与 `fixture-shapes` 的 Bash 5 runner；缺失或未知 slice 返回 usage 退出码 2，成功输出 measured elapsed seconds 和 `offline=true`。
- 建立 complete minimal graph：public baseline、registry、decisions、evidence 与 legacy 均含 genesis/successor，predecessor 与 manifest head SHA-256 可从嵌入对象离线复算。
- 建立 38-reference full oracle：normalized stable IDs、38 个 live paths、seed domains、76 个七键 aliases，以及每个 inventory capability 的唯一 current Adopt/Adapt/Reject decision。
- 建立 hostile-rendering 和 offline-source fixtures：前者包含 Markdown/括号/HTML-like/newline 数据但无 executable markup、secret 或本地绝对路径，后者可从 inline Base64 bytes 重算 entry/content/artifact hashes。
- 闭合四份 evidence ledger：full/hostile/minimal/offline current head 分别保留 156/86/172/86 条唯一记录，所有引用均解析且 minimal successor 完整保留 86-record genesis prefix。
- 用 `timeout` + `strace` 将 30 秒预算与 offline 声明变成运行时边界：缺失工具 fail closed，network syscall 被注入 `EPERM`，非空 trace 和 watchdog timeout 均返回非零。
- 关闭 caller-env bypass 和 raw trace disclosure：worker 必须消费 parent nonce/anonymous FD，network diagnostics 仅保留脱敏 syscall name，EXIT/INT/TERM 统一清理。
- 将 38-reference 对比改为 seed-first，并在 existing validator 后运行完整 Draft 2020-12 validation；fresh archive 无 `reference/` 时两 slice 仍通过。

## Task Commits

每个 TDD task 均以独立 RED/GREEN 提交落盘：

1. **Task 1 RED: Offline runner assertions** - `35784af` (test)
2. **Task 1 GREEN: Complete cross-family graph** - `33f9782` (feat)
3. **Task 2 RED: 38-reference identity and decision assertions** - `82b6897` (test)
4. **Task 2 GREEN: Full 38-reference valid fixture** - `c9fe5ce` (feat)
5. **Task 3 RED: Hostile and offline-source assertions** - `8949fb0` (test)
6. **Task 3 GREEN: Rendering and source identity fixtures** - `1ad04e1` (feat)
7. **Review RED: Evidence and runtime boundary assertions** - `760cfd1` (test)
8. **Review GREEN: Closed fixture evidence ledgers** - `371e085` (fix)
9. **Review GREEN: Fail-closed offline runtime guard** - `77dee00` (fix)
10. **Review 2 RED: Env/archive/schema/trace regressions** - `ebe1cc3` (test)
11. **Review 2 GREEN: Authenticated worker and redacted cleanup** - `17a6816` (fix)
12. **Review 2 RED: Full schema keyword mutations** - `4fed8c6` (test)
13. **Review 2 GREEN: Portable seed and Draft 2020-12 validation** - `25ac327` (fix)

**Plan metadata:** 原计划 tracking 已在 `48d53b3` 提交；review remediation 只更新本 SUMMARY，不再次修改 STATE、ROADMAP 或 REQUIREMENTS。

## Files Created/Modified

- `scripts/capability-governance-smoke.sh` - 两个稳定 named slices、结构提取、evidence closure/binding 检查，以及 fail-closed syscall guard 与 watchdog。
- `scripts/fixtures/capability-governance/valid/minimal-graph.json` - 五个 revision family 的完整 ancestry、86-to-172 immutable evidence prefix 和 path/hash-only current selector。
- `scripts/fixtures/capability-governance/valid/full-38-repositories.json` - 38 个 seed/live identities、兼容 aliases、imported domains 与 capability decisions。
- `scripts/fixtures/capability-governance/valid/hostile-rendering.json` - 后续 renderer escaping 的 hostile-but-valid 正例数据。
- `scripts/fixtures/capability-governance/valid/offline-source-identity.json` - 可离线解码、逐 entry 与整体复算的 controlled official-source artifact。

## Decisions Made

- 使用 canonical sorted compact JSON 作为 fixture 内部 hash 计算口径；这只是测试身份口径，不新增 production authority。
- `minimal-graph` 的 capabilities 保持小型，但 registry 仍采用 38 行，以直接满足已冻结 schema 的 `minItems/maxItems=38` 合同。
- Full fixture 的 fingerprint 均标识为 synthetic fixture values；不把本计划的正例数据表述为 license、source 或 product completion 证据。
- Hostile fixture 只使用不可执行的 HTML-like 标签；offline fixture 采用 `inline-base64-entry-lf-join` 使 Plan 01-07 能冻结并复算同一字节表示。
- Evidence records 按 owning capability/repository/decision/revision 绑定真实 subject、source revision、target revision 和 environment；共享 license review 只在同一 repository identity 内复用。
- Public CLI 继续只接受 `schemas|fixture-shapes`；socket/hang probes 仅通过隐藏测试环境触发，不形成第三个 slice。
- 四份 bundle 是相互隔离的 positive oracles；允许 bundle-local evidence IDs 重复，但记录其未来合并/聚合时的 collision 维护风险，本计划不再重写 29k fixture lines。
- Checked-in fixture generator 超出 01-03 白名单；后续维护应补 generator/rehash tool，而不是在本次 review 中新增未声明文件。

## TDD Evidence

- Task 1 RED 在 runner 断言已存在而 `minimal-graph.json` 尚不存在时，以 `fixture_required` 和退出码 1 失败；GREEN 后 `schemas=0.896s`、`fixture-shapes=0.055s`。
- Task 2 RED 在 minimal graph 继续通过后，以缺失 `full-38-repositories.json` 失败；GREEN 后 seed/live/alias/decision checks 通过，schema slice 为 `1.240s`。
- Task 3 RED 在前两组 fixture 继续通过后，以缺失 `hostile-rendering.json` 失败；GREEN 后安全文本与离线字节复算通过。
- Review RED 在旧 fixture 上稳定产生 minimal `history_removal`、其余 bundle `unknown_reference`，并在结构校验后产生 `runtime_guard_required`。
- Evidence GREEN 后四份 current heads 的 dangling reference 均为 0，minimal evidence records 从 86 条追加到 172 条；隔离 runtime 项时 `schemas` 与 `fixture-shapes` 均通过。
- Runtime GREEN 后两个公开 slices 在串行与并行复核中均低于 30 秒；正常 trace 为 0 字节，socket probe 显示 `EPERM (INJECTED)`，hang probe 返回 `slice_timeout`，缺失 `timeout`/`strace` 均 fail closed。
- Review 2 RED 分别复现 forged env bypass、tracked archive `FileNotFoundError`、`uniqueItems|maxItems|oneOf|not` false green，以及 raw strace leakage。
- Review 2 GREEN 后 7 个隐藏 regressions 全部通过；fresh archive 两 slice 通过，seed missing/invalid、jsonschema missing、tracer failure/trace missing 均稳定 fail closed，INT/TERM 后无 trace 或 hang child 残留。

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Normalized GSD tracking output**
- **Found during:** Plan metadata update
- **Issue:** `state.update-progress` wrote frontmatter `percent: 0` while leaving the body at 17%; `state.record-metric` appended its row outside the Plan History table; `roadmap.update-plan-progress` introduced repository-wide Markdown spacing churn.
- **Fix:** Recalculated 3-plan velocity/progress values, moved the 01-03 metric into Plan History, restored ROADMAP formatting from the clean pre-plan version, and retained only the intended 01-03 checkbox/count updates.
- **Files modified:** `.planning/STATE.md`, `.planning/ROADMAP.md`
- **Verification:** Focused tracking diff, `state.load`, line-count check, and `git diff --check`.
- **Committed in:** Plan metadata commit.

**2. [Rule 1 - Bug] Closed evidence and runtime proof boundaries**
- **Found during:** Post-plan code review
- **Issue:** Positive bundles referenced 48/155 undefined evidence IDs, minimal successor removed its prior record prefix, and proxy/elapsed-after-completion checks neither blocked direct sockets nor enforced the deadline.
- **Fix:** Added exact closure/prefix/hash/binding assertions, rebuilt all four evidence heads, and wrapped both slices with timeout-supervised strace fault injection.
- **Files modified:** The five declared Plan 01-03 implementation files only.
- **Verification:** RED/GREEN slices, zero-byte normal trace, injected socket probe, hang probe, missing-tool checks, usage status checks, and focused `git diff --check`.
- **Committed in:** `760cfd1`, `371e085`, and `77dee00`.

**3. [Rule 1 - Security/Portability] Closed second-review boundaries**
- **Found during:** Independent quality review after first remediation
- **Issue:** Caller env could select worker mode, tracked archives lacked `reference/`, the lightweight schema helper accepted critical keyword violations, and raw strace lines exposed PIDs/arguments.
- **Fix:** Added anonymous-FD worker authentication, parent-only trace ownership/redaction/cleanup, seed-first optional-live reconciliation, and complete Draft 2020-12 validation after the existing helper.
- **Files modified:** `scripts/capability-governance-smoke.sh` only; metadata is recorded in this SUMMARY.
- **Verification:** Seven hidden regressions, fresh archive slices, four schema mutations, seed/jsonschema/tracer failures, signal cleanup, public CLI statuses, and focused `git diff --check`.
- **Committed in:** `ebe1cc3`, `17a6816`, `4fed8c6`, and `25ac327`.

---

**Total deviations:** 3 auto-fixed (2 Rule 1 bugs, 1 security/portability closure).
**Impact on plan:** Tracking metadata remains unchanged by remediation; fixture scope and public slice names are unchanged, while the positive oracles and offline/deadline claims are now executable.

## Issues Encountered

Two review passes found incomplete evidence closure, advisory-only isolation, env-auth bypass, archive coupling, incomplete schema semantics, and raw tracer disclosure. Every issue was reproduced before repair and is covered by committed regressions.

## User Setup Required

No external service or account is required. The runner requires Bash 5, Python with `jsonschema`, GNU `timeout`, and Linux `strace`; missing schema/runtime guard tools fail closed without install or network fallback.

## Known Stubs

None. Fixture-only hashes and decisions remain deliberately synthetic and labeled as such; hidden review/socket/hang probes are test controls, not product surfaces. Bundle-local ID reuse and the absent checked-in generator are documented maintenance risks, not completion claims.

## Next Phase Readiness

- Plans 01-04/01-05 可以在同一 runner 上加入 coverage、integrity、drift 与 security negative corpora。
- Plan 01-06 可以直接消费四份 valid bundle 作为 semantic validator 的 GREEN oracles，并保留当前 stable slice names。
- Plan 01-06 可以复用 current-head reference closure、immutable prefix、record-chain 和 exact binding assertions，而不需要修补正例数据。
- Plan 01-07 可以使用 `inline-base64-entry-lf-join` 重算 controlled source bytes，无需网络 retrieval。
- `scripts/schema-contract-smoke.sh` 的既有 dirty hunk 保持 byte-identical，直到 Plan 01-12 执行既定 broad-gate integration。
- 后续 fixture 扩展应在允许新增文件的计划中补 checked-in generator/rehash command，并在任何跨-bundle aggregation 前定义 evidence ID namespace policy。

---
*Phase: 01-baseline-evidence-governance*
*Completed: 2026-07-15*

## Self-Check: PASSED

- All five planned files and this SUMMARY exist.
- RED commits `35784af`, `82b6897`, and `8949fb0` precede GREEN commits `33f9782`, `c9fe5ce`, and `1ad04e1` for their respective tasks.
- Review RED `760cfd1` precedes evidence/runtime GREEN commits `371e085` and `77dee00`.
- Review 2 RED commits `ebe1cc3` and `4fed8c6` precede GREEN commits `17a6816` and `25ac327`.
- Every original and remediation commit contains only Plan 01-03 declared paths; no tracked file deletion occurred, and remediation did not touch STATE, ROADMAP, or REQUIREMENTS.
- Coverage classification reports 4/4 deliverables as auto-covered with passing verification.
- All evidence references resolve in current heads; record chains, minimal immutable prefix, exact bindings, network denial, watchdog, usage statuses, and missing-tool failures are exercised.
- Forged env/direct worker entry, raw trace disclosure, fresh archive portability, seed failures, full schema keywords, jsonschema absence, tracer/trace failure, and INT/TERM cleanup are exercised.
- Protected `scripts/schema-contract-smoke.sh` and `.planning/config.json` retain their pre-execution working-tree hashes.
