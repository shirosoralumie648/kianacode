# Phase 1: 现状基线与证据治理 - Research

<user_constraints>

## Implementation Decisions

### Public-Parity Ledger

- **D-01:** Claude Code public-parity ledger 采用“用户旅程 + 可验证子能力”两级结构。旅程用于表达端到端用户价值；command、flag、IDE、Desktop/Web、model、memory、MCP、subagent/team、plugin/skill/hook、checkpoint、Chrome、Git/CI、SDK、voice 等具体行为作为独立子能力记录。
- **D-02:** 每个子能力独立绑定稳定 ID、必要性、来源、parity outcome、required proof level、当前状态和 evidence。不得用旅程级自由文本覆盖未通过的子能力。
- **D-03:** 官方公开 docs、release notes、公开 help/schema 和公开支持声明是专有产品基线的优先来源；clean-room 黑盒实测用于补充、复现和记录冲突。第三方实现和反编译快照不能单独定义 Claude Code 的公开合同。
- **D-04:** 每条来源至少保留 URL 或公开来源标识、retrieved-at、适用产品版本、内容 hash 和可复现说明。来源冲突必须显式记录，不能静默选择方便的结论。
- **D-05:** 基线按冻结日期和产品版本生成不可变 snapshot。后续刷新生成新 snapshot 与机器可读 diff；历史 snapshot 不被覆盖，报告必须显示正在使用的 snapshot ID。
- **D-06:** 一个旅程只有在全部 required 子能力均为 current、verified 且达到各自 required proof level 时才完成。optional、intentional difference、not applicable 和 unsupported 必须显式分类；禁止用平均百分比掩盖关键缺口。

### 38-Repo Reference Governance

- **D-07:** Reference 治理采用两层模型：repository registry 覆盖冻结时的 38/38 仓库；capability decision records 表达每个仓库内具体能力的 Adopt、Adapt 或 Reject 决策。
- **D-08:** 仓库本身不承载单一总判决。同一仓库可以同时贡献多个 Adopt、Adapt 和 Reject 记录；未被采用的能力也必须可追踪，避免“扫描过”等价于“已治理”。
- **D-09:** Repository registry 至少记录稳定 repo ID、路径/上游 URL、冻结 revision、scan timestamp、license 状态、来源可用性和当前 freshness。重命名、移除或替换必须保留历史身份和原因。
- **D-10:** 每条 capability decision 至少记录 capability ID、source location、decision、rationale、license compatibility、target owner、Kiana target location、risk、tests、evidence 和 review revision。Reject 必须有明确理由；缺字段的决定不能进入 38/38 完成统计。
- **D-11:** 上游 HEAD、license、来源内容或目标实现 revision 发生变化时，受影响记录变为 stale 并退出当前完成统计；复审产生新 revision，旧决定和证据继续保留。
- **D-12:** Adopt/Adapt 只表达借鉴策略，不代表 Kiana 已实现或验证该能力。Reference governance completion 与 product capability completion 必须分别统计。

### Proof Levels, Status, and Freshness

- **D-13:** 账本将 coverage state、proof level 和 freshness 分开建模，禁止用单一 `complete` 布尔值混合“代码存在”“本地测试通过”“目标环境通过”和“用户价值成立”。
- **D-14:** Proof level 固定为有序阶梯：`none`、`source`、`local_contract`、`local_behavior`、`target_environment`、`user_value`。每条 requirement/capability 显式声明 required proof level。
- **D-15:** Coverage state 至少能区分 `unassessed`、`missing`、`partial`、`implemented`、`verified`、`blocked`、`intentional_difference` 和 `not_applicable`；freshness 至少能区分 `current`、`stale` 和 `superseded`。
- **D-16:** Evidence 是追加式、带类型的不可变记录，至少绑定 source/artifact、Git revision 或发布 artifact、执行环境、命令或人工验收、时间、结果和内容 hash。失败复测不删除旧证据，而是降低当前有效状态并追加新记录。
- **D-17:** Source/version drift、Git/artifact hash 变化、目标环境变化、失败复测或证据自身声明的有效期届满都会触发 stale。采用事件/指纹驱动 freshness；是否增加时间 TTL 由 evidence type 决定，不使用一个全局天数。
- **D-18:** 只有 `freshness=current`、`coverage_state=verified` 且 `proof_level` 达到 required proof level 的条目才进入完成统计。模块、类型、命令、stub、mock response、测试数量或模型自述只能作为低等级输入，不能提升到更高 proof level。

### Canonical Data and Generated Views

- **D-19:** 版本化、schema-first JSON 是 ledger 的唯一事实源。Markdown、表格和摘要是确定性生成的人读视图，并清楚标记 generated/do-not-edit。
- **D-20:** Canonical records 按治理对象分层：public baseline/snapshots、repository registry、capability decisions 和 evidence index 分开维护，避免继续扩张单个超长 Markdown。精确文件拆分由 planner 决定，但这些对象边界不能合并为自由文本。
- **D-21:** Stable IDs 和交叉引用必须可验证。重复 ID、未知引用、缺失 38-repo registry 项、缺少 Reject 理由、`verified` 无 evidence、proof level 倒退无事件、snapshot/freshness 不一致均 fail closed。
- **D-22:** Generated Markdown 必须可重复生成；canonical JSON 与生成视图存在 diff、生成文件被手工修改或 schema 不合法时，CI/validation gate 失败。
- **D-23:** 现有 `docs/reference-feature-matrix.md`、reference audits、migration roadmap 和 commercial readiness 文档作为迁移输入或生成视图，不再各自维护第二套完成状态。
- **D-24:** 复用仓库现有 versioned schema、JSON/human dual output、audit/report 和 schema smoke 模式。Phase 1 可以通过窄脚本或现有 command 集成提供验证；是否新增具体 command 名称由 planner 根据最小变更原则决定。

### Agent Discretion

- Canonical JSON 的精确目录、单文件还是按 repo 分片，以及 stable ID 的可读前缀。
- Validator/generator 使用 Rust command、现有 Bash/Python script，或两者组合；必须复用既有 schema 验证入口并保持跨平台可复现。
- Generated Markdown 的表格分组、导航和摘要排版，只要所有 canonical 字段和阻塞原因均可查看。
- Evidence artifact 的物理存储布局；引用必须是 repo-relative 或受控外部 URI，且包含 hash/identity，不能把临时绝对路径写成权威证据。

## Deferred Ideas

- 账本发现的具体 capability 缺口由 ROADMAP 中对应 Phase 2-24 实现和验证，Phase 1 只记录、分类和路由。
- 持续后台监控所有专有产品页面与 38 个 reference 上游变化，不是 Phase 1 的必要条件；本阶段先建立显式、可重复的 refresh/freeze 流程。
- 目标平台、真实云服务、企业客户和最终用户验收证据仍由对应 phase 获取，不能在本阶段制造或替代。

</user_constraints>

<phase_requirements>

- [ ] **COD-01**: 发布一份带冻结日期的 Claude Code public-parity ledger，覆盖安装/登录、CLI flags、交互命令、IDE、Desktop/Web、models、memory、MCP、subagent/team、plugins/skills/hooks、checkpoint、Chrome、Git/CI、SDK、voice 等公开旅程；每行必须有结果证据或明确差异决策
- [ ] **DIF-11**: Audited capability superset 让 38 个 reference 和专有公开基线逐项记录 Adopt/Adapt/Reject、license、安全、owner、test 与 evidence，并能解释取舍

</phase_requirements>

**Researched:** 2026-07-15
**Domain:** Schema-first capability baseline, evidence governance, deterministic reporting, and reference/license decision governance
**Confidence:** HIGH for repository architecture and validation recommendations; MEDIUM for external-standard mappings not fetched again in this repo-first rerun. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md; .planning/ROADMAP.md:40-48; .planning/REQUIREMENTS.md:42,140]

## Summary

Phase 1 should produce a small governance compiler rather than another manually maintained progress document: four separate schema-first JSON object families, a deterministic Markdown generator, and a semantic validator that fails closed on cross-record or proof-state inconsistencies. The four canonical families are public baseline snapshots, repository registry snapshots, capability decision records, and an append-only evidence index; existing long-form reference and commercial-readiness documents are migration inputs or generated views only. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-19..D-24; docs/reference-feature-matrix.md; docs/reference-migration-roadmap.md]

The minimum implementation path is the repository's existing Python-standard-library plus Bash contract-smoke pattern. `scripts/validate-json-schema.py` validates a useful structural subset but does not implement uniqueness, cross-file references, ordered proof comparisons, 38/38 reconciliation, freshness transitions, or generated-summary consistency, so a focused semantic validator is required beside it. No third-party package is needed. [VERIFIED: scripts/validate-json-schema.py:1-153; scripts/schema-contract-smoke.sh]

Planning must keep governance completion separate from product completion. A current Adopt or Adapt decision proves only that a source capability was governed; a public journey or Kiana capability is complete only when every required child is `verified`, `current`, backed by current evidence, and at or above its declared proof threshold. Phase 1 records gaps and routes them to later phases; it must not repair those gaps or synthesize target/user proof. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-06,D-12,D-18 and Deferred Ideas; .planning/ROADMAP.md:40-48]

**Primary recommendation:** Plan one vertical governance pipeline in this order: contracts and fixtures, canonical seed/migration, semantic validation, deterministic generated views, then optional thin `audit`/`report` consumption; do not add a package or a new entrypoint-owned implementation. [VERIFIED: AGENTS.md; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-24; .planning/codebase/STRUCTURE.md]

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|---|---|---|---|
| Versioned canonical contracts | `docs/schemas/` | canonical JSON directories under `docs/agent-program/kiana-completion/` | Existing public machine contracts and fixtures are owned in `docs/schemas/`; canonical data remains documentation-domain evidence, not runtime session state. [VERIFIED: AGENTS.md; .planning/codebase/STRUCTURE.md; scripts/schema-contract-smoke.sh] |
| Public baseline snapshots | Canonical JSON data | deterministic generated Markdown | Frozen source/version/hash data and journey/capability rows are authoritative; Markdown is derived. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-01..D-06,D-19] |
| 38-repo registry | Canonical JSON data | `reference/` live reconciliation | The repository contains 38 top-level reference directories and the existing seed lists 38 records; validation must reconcile identities without making the live directory names the only history. [VERIFIED: command `find reference -mindepth 1 -maxdepth 1 -type d | wc -l` => 38; command `jq '.references | length' docs/agent-program/kiana-completion/references.json` => 38] |
| Capability decisions | Canonical JSON data | evidence index | Adopt/Adapt/Reject is per capability and can be mixed within one repository; decision state does not imply implementation state. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-07..D-12] |
| Structural validation | `scripts/validate-json-schema.py` and `docs/schemas/` | `scripts/schema-contract-smoke.sh` | The existing helper owns the repository's schema-subset checks and the smoke script exercises positive and negative fixtures. [VERIFIED: scripts/validate-json-schema.py; scripts/schema-contract-smoke.sh] |
| Semantic validation and view generation | Focused Python scripts under `scripts/` | optional `kiana-commands` adapter | Cross-file invariants and byte-for-byte generated output checks do not belong in JSON Schema; a command adapter is justified only when another product surface needs the report. [VERIFIED: scripts/validate-json-schema.py; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-21..D-24] |
| Human and machine audit/report output | `kiana-commands/src/audit.rs`, `kiana-commands/src/report.rs` | command registry | Existing commands already expose schema-aware blocking and progress reports; Phase 1 should consume their boundary rather than duplicate routing in entrypoints. [VERIFIED: kiana-commands/src/audit.rs; kiana-commands/src/report.rs; AGENTS.md] |
| Product entrypoints | `kiana-entrypoints` thin adapters only | command registry | Product surfaces must consume shared runtime/contracts and cannot own duplicate baseline logic. [VERIFIED: AGENTS.md Architecture and Constraints] |

## Project Constraints (from AGENTS.md)

- Continue Rust 2021, Tokio, Cargo workspace, and existing crate layering; shared runtime contracts go into the appropriate lower crate before surface exposure. [VERIFIED: AGENTS.md Technology Stack and Constraints]
- All product surfaces must consume the same runtime, session, tool, policy, and event contracts; do not copy core logic into Desktop, Web, IDE, or a pack. [VERIFIED: AGENTS.md Architecture]
- Command behavior belongs in `kiana-commands`, registration in `kiana-commands/src/registry.rs`, and top-level entrypoint routing must remain thin. [VERIFIED: AGENTS.md Conventions and Architecture]
- Public JSON contracts use stable schema IDs, strict enums, structured errors, schema fixtures, and matching human/JSON output where applicable. [VERIFIED: AGENTS.md Conventions; docs/schemas/kiana-commercial-release-blockers.v1.schema.json; scripts/commercial-release-blockers-report.sh]
- Local-first operation, system Keychain/server vault credential ownership, explicit sync/telemetry opt-in, and fail-closed trust/network/side-effect policies remain mandatory; this phase must not place credentials in evidence. [VERIFIED: AGENTS.md Project Constraints]
- Research claims, citations, data, experiments, and charts retain provenance; unverifiable content cannot enter final conclusions. [VERIFIED: AGENTS.md Project Constraints]
- Open-source reuse requires compatible licensing, while proprietary behavior is independently implemented from public behavior under clean-room rules. [VERIFIED: AGENTS.md Project Constraints; .planning/REQUIREMENTS.md:168-169]
- A capability cannot count as complete without a matrix entry, owner, test, and evidence. [VERIFIED: AGENTS.md Project Constraints; .planning/REQUIREMENTS.md:189,199]
- Paths must be normalized before I/O, generated evidence must not use temporary absolute paths as authority, and unsafe or inconsistent inputs fail closed. [VERIFIED: AGENTS.md Conventions; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md Agent Discretion]
- Preserve the current dirty worktree and use explicit file lists for planning and commits; Phase 1 work must not absorb unrelated in-flight implementation. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md Integration Points; .planning/codebase/CONCERNS.md:7]

## Standard Stack

### Core

| Tool or contract | Version / location | Purpose | Why this is the standard here |
|---|---|---|---|
| JSON with versioned JSON Schema | `docs/schemas/*.v1.schema.json` | Canonical object structure and public contract versioning | This is the repository's established machine-contract format and is locked by D-19/D-24. [VERIFIED: docs/schemas/; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-19,D-24] |
| Python standard library | Python 3.13.12 in current environment | JSON loading, semantic graph checks, hashing, deterministic rendering, and CLI exit status | Existing schema validation already uses only `argparse`, `json`, `pathlib`, `re`, and `sys`; no new dependency is necessary. [VERIFIED: command `python3 --version` => Python 3.13.12; scripts/validate-json-schema.py] |
| Existing schema subset validator | `scripts/validate-json-schema.py` | Structural validation of required fields, enums, patterns, local refs, arrays, and selected conditionals | It is already used throughout the schema smoke gate and must remain the shared structural entry. [VERIFIED: scripts/validate-json-schema.py; scripts/schema-contract-smoke.sh] |
| Bash contract-smoke harness | Bash 5.1.16; `scripts/schema-contract-smoke.sh` | Positive/negative fixture execution and aggregate contract gate | It is the established repository-wide schema contract gate. [VERIFIED: command `bash --version`; scripts/schema-contract-smoke.sh; .planning/codebase/CONCERNS.md:37,89] |
| Git | Git 2.34.1 in current environment | Resolve current repository revision, source drift, and review revision | Git revisions are already part of the required evidence and freshness identity. [VERIFIED: command `git --version` => 2.34.1; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-09,D-11,D-16] |
| SHA-256 via Python `hashlib` | Python standard library | Content identity for sources, canonical inputs, artifacts, and generated views | D-04/D-16 require content hashes and the repository already uses SHA-256 evidence fields. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-04,D-16; scripts/schema-contract-smoke.sh] |

### Supporting

| Tool or module | When to use | Boundary |
|---|---|---|
| `kiana-commands/src/audit.rs` | Consume a validated governance summary when CLI audit visibility is required | Do not move canonical validation into the entrypoint or duplicate semantic checks. [VERIFIED: kiana-commands/src/audit.rs; AGENTS.md] |
| `kiana-commands/src/report.rs` | Render proof-level/freshness-aware progress after the canonical compiler exists | Existing report currently focuses on workflow/task/evidence; extend only through a narrow shared contract. [VERIFIED: kiana-commands/src/report.rs] |
| `jq` | Developer inspection in shell proofs | Do not make `jq` the canonical compiler; its current environment version is 1.6 and cross-platform availability is not guaranteed by the Rust product. [VERIFIED: command `jq --version` => jq-1.6; AGENTS.md Platform Requirements] |
| Cargo focused tests | Only if a Rust command adapter is added | Run focused `kiana-commands` tests in addition to script checks; do not require broad workspace tests for every fixture edit. [VERIFIED: AGENTS.md Code Style; .planning/codebase/TESTING.md] |

### Alternatives Considered

| Instead of | Alternative | Disposition |
|---|---|---|
| Python semantic validator/generator | New Rust governance crate | Reject for the initial Phase 1 slice unless shared runtime consumers require typed Rust contracts; it increases compile surface without solving a current package gap. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-24 and Agent Discretion; AGENTS.md Prescriptive Stack Guidance] |
| Separate semantic validation | Encoding all invariants in JSON Schema | Reject; the local validator cannot express global uniqueness, arbitrary cross-file references, proof ordering, or generated-view byte equality. [VERIFIED: scripts/validate-json-schema.py] |
| Canonical JSON | Hand-maintained Markdown tables | Reject; D-19/D-22 make JSON authoritative and Markdown generated. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-19,D-22] |
| Explicit refresh command | Always-on background watcher | Defer; continuous upstream monitoring is explicitly out of Phase 1. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md Deferred Ideas] |

**Installation:** No package installation. Use the checked-in scripts and the Python/Rust/Bash toolchain already required by the repository. [VERIFIED: Cargo.toml; scripts/validate-json-schema.py; AGENTS.md Technology Stack]

## Package Legitimacy Audit

Phase 1 should introduce no external package, so the package-legitimacy gate has no package candidates and no registry lookup is required. This avoids a new dependency and supply-chain surface for a task fully covered by the standard library and existing workspace. [VERIFIED: scripts/validate-json-schema.py; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-24]

| Ecosystem | New packages | Verdict | Disposition |
|---|---:|---|---|
| crates.io | 0 | Not applicable | Keep workspace dependencies unchanged. [VERIFIED: Cargo.toml] |
| PyPI | 0 | Not applicable | Use Python standard library only. [VERIFIED: scripts/validate-json-schema.py] |
| npm | 0 | Not applicable | No Node package is part of the Phase 1 implementation. [VERIFIED: AGENTS.md Technology Stack] |

**Packages removed due to SLOP verdict:** none; the recommended Phase 1 package candidate set is empty. [ASSUMED]

**Packages flagged as suspicious:** none; the recommended Phase 1 package candidate set is empty. [ASSUMED]

## Architecture Patterns

### System Architecture Diagram

```text
official public sources            reference/ + existing audits
          |                                     |
          v                                     v
public baseline snapshot                repository registry snapshot
          |                                     |
          +----------+              +-----------+
                     v              v
                   capability decisions
                            |
                            v
                 append-only evidence index
                            |
                +-----------+------------+
                |                        |
                v                        v
        semantic validator       deterministic generator
                |                        |
                +-----------+------------+
                            v
                 generated Markdown views
                            |
                            v
                optional audit/report adapter
```

The diagram reflects the locked separation between four canonical object families and derived human views; it does not authorize capability implementation or live-monitoring services. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-03..D-05,D-07,D-19..D-24 and Deferred Ideas]

### Pattern 1: Four Canonical Object Families

Use separate versioned roots even if the planner chooses one directory and one file per family. A recommended physical layout is shown below; the exact names remain planner discretion. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-20 and Agent Discretion]

```text
docs/agent-program/kiana-completion/governance/
  public-baselines/<snapshot-id>.json
  repository-registry/<snapshot-id>.json
  capability-decisions/<review-revision>.json
  evidence/index.json
  generated/public-parity.md
  generated/reference-governance.md
docs/schemas/
  kiana-public-parity-baseline.v1.schema.json
  kiana-reference-repository-registry.v1.schema.json
  kiana-capability-decisions.v1.schema.json
  kiana-capability-evidence-index.v1.schema.json
```

Do not collapse these roots into a universal record union. Each family has different identity, lifecycle, and completeness rules, while cross-references are checked semantically. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-20,D-21]

### Pattern 2: Snapshot Identity Is Immutable and Content-Bound

A snapshot needs a stable `snapshot_id`, `frozen_at`, applicable product/source version, input source identities and hashes, and the Git revision of the Kiana governance data. Refresh creates a new file or immutable record and a machine-readable diff; it never mutates the historical snapshot in place. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-04,D-05,D-16]

Use a stable repo ID independent of commit hash. Two repository entries may legitimately share a revision, and content-only snapshots need an explicit `revision_kind` such as `git_commit` or `content_tree_sha256`; path/upstream identity and historical aliases preserve identity across rename or replacement. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-09; docs/agent-program/kiana-completion/references.json]

### Pattern 3: Journey Aggregation Is a Strict Gate

Represent public parity as `journeys[]` with child `capabilities[]` or cross-referenced capability IDs. Required and optional membership is explicit. A journey passes only when every required child satisfies the same completion predicate used by reports. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-01,D-02,D-06,D-18]

```text
counts_as_complete(entry) =
    entry.coverage_state == "verified"
    AND entry.freshness == "current"
    AND rank(entry.proof_level) >= rank(entry.required_proof_level)
    AND has_current_matching_evidence(entry)
```

The proof rank is fixed as `none < source < local_contract < local_behavior < target_environment < user_value`; reports must show the individual axes rather than serializing the comparison result as a free-standing `complete` truth source. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-13..D-18]

### Pattern 4: Repository Governance and Product Progress Are Separate Projections

The registry proves that all frozen repositories have identity, revision, source availability, license state, and freshness. Decision records prove that inventoried source capabilities received Adopt/Adapt/Reject review with rationale, ownership, risk, tests, and evidence. Neither projection raises a Kiana capability to implemented or verified. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-07..D-12]

The semantic validator should reconcile the registry against both `expected_count = 38` and the frozen identity set, then require every current registry entry to have an explicit governance disposition for every capability in its frozen inventory. Merely having one decision per repository is insufficient when multiple capabilities were inventoried. [VERIFIED: docs/agent-program/kiana-completion/references.json; .planning/REQUIREMENTS.md:140,199; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-08,D-10,D-21]

### Pattern 5: Append Evidence, Recompute Current State

Evidence records should be immutable and typed, with a unique ID, subject ID, observed revision/artifact, environment, command or acceptance method, observed time, result, proof level, and content hash. A failed rerun appends a new record and causes the subject projection to become stale or non-verified; it does not delete the prior success. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-16,D-17]

Freshness is event/fingerprint driven. Optional `expires_at` is evidence-type-specific; do not infer a single global TTL. Validator output should name the exact drift cause and the affected IDs. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-11,D-17]

### Pattern 6: Deterministic Views Are Build Artifacts

The generator reads only validated canonical JSON, sorts every collection by stable ID plus a documented secondary key, emits UTF-8 with LF newlines, excludes wall-clock generation timestamps unless sourced from the snapshot, and renders explicit `generated/do-not-edit` markers plus canonical IDs. `--check` regenerates in memory and fails on any byte difference. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-19,D-22; AGENTS.md Data and Release Quality constraints]

Summary counts are derived during validation/generation, not hand-authored. The validator recomputes journey, decision, proof-level, freshness, and blocker totals and rejects mismatches just as the current commercial blocker audit recomputes its summary from detailed checks. [VERIFIED: kiana-commands/src/audit.rs:1156-1274; scripts/commercial-release-blockers-report.sh]

### Pattern 7: Migrate, Then Retire Duplicate Status Fields

Treat `docs/reference-feature-matrix.md`, reference audit documents, migration roadmap, and commercial readiness files as input material. Seed canonical records with provenance back to the exact source section; once generated views cover the required reader workflow, remove or mark old status fields non-authoritative rather than synchronizing two completion systems. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-23; docs/reference-feature-matrix.md; docs/reference_audit/kiana_capability_matrix.md]

## Don't Hand-Roll

- Do not hand-parse JSON or JSONL with shell text processing; use Python `json` and reject duplicate IDs semantically after parsing. [VERIFIED: scripts/validate-json-schema.py]
- Do not infer a license solely from a README badge, package metadata, directory name, or upstream reputation; store discovery status, evidence path/hash, declared expression, and compatibility review separately. [VERIFIED: .planning/PROJECT.md:80; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-09,D-10]
- Do not invent one repository-level Adopt/Adapt/Reject verdict; decisions are capability-level and can differ inside one repository. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-07,D-08]
- Do not use a commit hash as the stable repository ID or overwrite a snapshot when content changes. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-05,D-09]
- Do not encode cross-file uniqueness, reference existence, proof ranking, or 38/38 reconciliation only in JSON Schema; the checked-in validator does not implement those global semantics. [VERIFIED: scripts/validate-json-schema.py]
- Do not make Markdown, row count, module count, test count, stub presence, or model self-report an authoritative completion signal. [VERIFIED: .planning/REQUIREMENTS.md:170; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-18,D-19]
- Do not create a new provider client or crawler in Phase 1; source refresh is an explicit bounded command and continuous monitoring is deferred. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-03..D-05 and Deferred Ideas; AGENTS.md Network constraint]
- Do not introduce a second audit/report implementation in `kiana-entrypoints`; reuse command and script boundaries. [VERIFIED: AGENTS.md Architecture; kiana-commands/src/audit.rs; kiana-commands/src/report.rs]
- Do not convert Adopt/Adapt into automatic source copying. License, security, target owner, tests, and evidence remain gates, and proprietary behavior remains clean-room. [VERIFIED: .planning/REQUIREMENTS.md:168-169; AGENTS.md Licensing constraint]

## Common Pitfalls

### Pitfall 1: A Valid Schema Is Mistaken for a Valid Ledger

An instance can pass `scripts/validate-json-schema.py` while containing duplicate IDs, dangling references, an incorrect summary, or a proof downgrade without an event because the helper validates one instance tree and a limited keyword subset. Require semantic negative fixtures for every D-21 failure class. [VERIFIED: scripts/validate-json-schema.py; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-21]

### Pitfall 2: Count 38 Hides Identity Drift

Checking only `len(repositories) == 38` permits one missing repository to be replaced by a duplicate or unrelated path. Check unique stable IDs, unique normalized current paths, frozen identity membership, historical aliases, and one-to-one live-path reconciliation. [VERIFIED: command `find reference -mindepth 1 -maxdepth 1 -type d | wc -l` => 38; docs/agent-program/kiana-completion/references.json]

### Pitfall 3: License Unknown Is Treated as Compatible

Missing, conflicting, or non-machine-readable license evidence must remain `unknown` or `review_required` and block Adopt code reuse; it must not silently default to compatible. Reject is still valid when it contains a reason and evidence. [VERIFIED: AGENTS.md Licensing and Safety constraints; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-10,D-21]

### Pitfall 4: Stale Evidence Still Satisfies a Current Decision

Evidence must match the source revision, target revision/artifact, environment identity, and required proof level for the subject. A historical success remains visible but exits current completion when any bound identity drifts or a newer failure supersedes it. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-11,D-16..D-18]

### Pitfall 5: Generated Markdown Is Nondeterministic

Filesystem iteration order, locale sorting, current timestamps, absolute paths, and unordered JSON maps can produce perpetual diffs. Sort explicitly, use snapshot time rather than generation time, normalize repo-relative paths, and compare exact bytes in `--check`. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-22 and Agent Discretion]

### Pitfall 6: Intentional Difference Masks Missing Required Behavior

`intentional_difference`, `not_applicable`, `optional`, and `unsupported` need explicit applicability and rationale. The validator must not let a required capability disappear from journey aggregation merely because its outcome text mentions a difference. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-02,D-06,D-15]

### Pitfall 7: Migration Creates Two Sources of Truth

If old Markdown keeps editable statuses after canonical JSON lands, reports will diverge. Add generated markers and exact canonical IDs, and fail `--check` when generated files are edited. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-19,D-22,D-23]

### Pitfall 8: Phase 1 Expands Into Capability Repair

The first discovered missing feature can pull implementation into the baseline phase and destroy traceability. Record it as a blocked/missing capability with owner and destination phase, then stop. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md Phase Boundary and Deferred Ideas]

### Pitfall 9: Untrusted Paths Escape the Governance Root

Repository paths, source locations, and evidence artifact paths are data inputs. Reject absolute temporary paths, `..` traversal, symlink escapes where files are opened, oversized inputs, and unsupported URI schemes before hashing or rendering. [VERIFIED: AGENTS.md Safety and path-validation conventions; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md Agent Discretion]

### Pitfall 10: Dirty Worktree Changes Enter the Phase Commit

The repository already has unrelated in-flight files. Every plan and commit must name exact Phase 1 paths; broad `git add` or formatter-driven unrelated rewrites are unacceptable. [VERIFIED: .planning/codebase/CONCERNS.md:7; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md Integration Points]

## Code Examples

### Semantic Proof Ordering

```python
PROOF_RANK = {
    "none": 0,
    "source": 1,
    "local_contract": 2,
    "local_behavior": 3,
    "target_environment": 4,
    "user_value": 5,
}


def counts_as_complete(entry: dict, current_evidence_ids: set[str]) -> bool:
    return (
        entry["coverage_state"] == "verified"
        and entry["freshness"] == "current"
        and PROOF_RANK[entry["proof_level"]]
        >= PROOF_RANK[entry["required_proof_level"]]
        and any(evidence_id in current_evidence_ids for evidence_id in entry["evidence_ids"])
    )
```

This map is a semantic implementation of the locked proof ladder, not a new proof taxonomy. Production validation must also verify that each evidence ID belongs to the same subject and bound revision. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-14,D-18,D-21]

### Duplicate and Dangling Reference Checks

```python
def index_unique(records: list[dict], kind: str) -> dict[str, dict]:
    indexed: dict[str, dict] = {}
    for record in records:
        record_id = record["id"]
        if record_id in indexed:
            raise GovernanceError(f"duplicate_{kind}_id:{record_id}")
        indexed[record_id] = record
    return indexed


def require_ref(index: dict[str, dict], record_id: str, owner_id: str) -> None:
    if record_id not in index:
        raise GovernanceError(f"unknown_reference:{owner_id}:{record_id}")
```

Machine-readable error prefixes should be stable so negative fixtures assert the exact failed invariant. [VERIFIED: AGENTS.md Error Handling conventions; kiana-commands/src/validate.rs]

### Deterministic Markdown Check

```python
def render_reference_view(registry: dict, decisions: dict) -> str:
    rows = sorted(decisions["records"], key=lambda row: (row["repo_id"], row["capability_id"], row["id"]))
    lines = ["<!-- generated; do not edit -->", "", "# Reference Governance", ""]
    for row in rows:
        lines.append(f"- `{row['id']}`: {row['decision']} - {row['rationale']}")
    return "\n".join(lines) + "\n"


def check_view(path: Path, expected: str) -> None:
    if path.read_text(encoding="utf-8") != expected:
        raise GovernanceError(f"generated_view_drift:{path.as_posix()}")
```

The real renderer must escape Markdown table/control characters and render all blockers and canonical IDs, not interpolate untrusted text directly into links or HTML. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-22; AGENTS.md Safety constraint]

### Suggested Fast Gate

```bash
python3 scripts/validate-json-schema.py docs/schemas/kiana-public-parity-baseline.v1.schema.json docs/agent-program/kiana-completion/governance/public-baselines/baseline-2026-07-15.json
python3 scripts/validate-capability-governance.py --root docs/agent-program/kiana-completion/governance
python3 scripts/generate-capability-governance.py --root docs/agent-program/kiana-completion/governance --check
bash scripts/schema-contract-smoke.sh
```

The exact script and directory names remain planner discretion; the required layers and order do not. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-20,D-24 and Agent Discretion]

## Validation Architecture

### Layered Gate

1. **Parse and structural schema:** load every JSON file as UTF-8, reject parse errors, and validate each object with its versioned schema through the existing helper. [VERIFIED: scripts/validate-json-schema.py; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-19,D-24]
2. **Semantic graph validation:** build unique ID indexes and validate cross-file references, 38/38 identities, decision completeness, Reject rationale, evidence subject/revision binding, proof ordering, freshness, immutable snapshot ancestry, and recomputed summaries. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-06,D-10,D-11,D-18,D-21]
3. **Deterministic generation:** render both public parity and reference governance views only after layers 1-2 pass; `--check` compares exact bytes and fails on edits or nondeterminism. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-19,D-22]
4. **Contract fixtures:** add valid fixtures and one focused negative fixture per fail-closed invariant to `scripts/schema-contract-smoke.sh` or a focused sibling invoked by it. [VERIFIED: scripts/schema-contract-smoke.sh; .planning/codebase/TESTING.md]
5. **Optional command adapter tests:** if `audit` or `report` consumes the result, add focused tests in `kiana-commands/tests/audit_command.rs` or `report_command.rs`; no adapter means no Rust test obligation for this phase. [VERIFIED: kiana-commands/tests/audit_command.rs; kiana-commands/tests/report_command.rs; AGENTS.md Conventions]

### Requirement-to-Test Map

| Requirement | Observable contract | Fast automated evidence (<30s target) | Required negative cases |
|---|---|---|---|
| `COD-01` | A frozen public snapshot covers every required named journey and each child has source, parity outcome, required/current proof, state, freshness, and evidence/difference rationale; generated view names the snapshot ID. [VERIFIED: .planning/REQUIREMENTS.md:42; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-01..D-06] | Schema positive fixture; semantic validator over a minimal complete multi-journey snapshot; generator `--check`; the Phase 1 fixture slice inside `bash scripts/schema-contract-smoke.sh`. These checks are local and bounded, and should be designed to run below 30 seconds without network access. [VERIFIED: scripts/schema-contract-smoke.sh; .planning/codebase/TESTING.md] | Missing required journey/capability; duplicate capability ID; unknown source/evidence ID; required child marked verified below threshold; stale child counted complete; snapshot ID/version/hash missing; generated view drift. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-02,D-04,D-06,D-18,D-21,D-22] |
| `DIF-11` | The frozen registry contains exactly the 38 identities; every inventoried capability has an Adopt/Adapt/Reject record with license, security/risk, owner, target, tests, evidence, review revision, and freshness; Reject has rationale. [VERIFIED: .planning/REQUIREMENTS.md:140,199; docs/agent-program/kiana-completion/references.json] | Schema positive fixture; semantic validation of the full canonical registry/decision graph; generator `--check`; same Phase 1 smoke slice. Use a small synthetic 2-repo fixture for most negative tests and one full 38-record reconciliation fixture. [VERIFIED: scripts/schema-contract-smoke.sh; docs/agent-program/kiana-completion/references.json] | 37 records; duplicate ID/path; unknown repo reference; missing capability decision; missing Reject reason; Adopt with unknown/incompatible license counted complete; target revision drift left current; decision evidence belongs to another subject; governance count used as product completion. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-07..D-12,D-21] |

### Fast Gate Budget

- Keep schema fixtures small and in-process; one full 38-record fixture is sufficient for exact-coverage reconciliation. [VERIFIED: docs/agent-program/kiana-completion/references.json]
- Do not fetch the network, build the full Rust workspace, or rescan all repository contents in the fast contract gate; source refresh and contract validation are separate commands. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-05,D-17,D-22 and Deferred Ideas]
- If a Rust command is added, its focused test joins the fast gate, while `cargo fmt --all --check` and the repository's broader serialized workspace gate remain handoff/release checks. [VERIFIED: AGENTS.md Code Style; .planning/codebase/TESTING.md]
- Capture elapsed time in CI or smoke output so the planner can prove the `<30s` target rather than assume it. [ASSUMED]

### Mandatory Semantic Invariants

| Invariant | Failure behavior |
|---|---|
| Every ID unique within its namespace; every reference resolves to the expected object type | Non-zero exit with stable `duplicate_*` or `unknown_reference` prefix. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-21] |
| Registry has 38 frozen unique identities and current path reconciliation is one-to-one | Non-zero exit; list missing, extra, duplicated, renamed, or unavailable IDs separately. [VERIFIED: docs/agent-program/kiana-completion/references.json; .planning/REQUIREMENTS.md:199] |
| Every Reject has rationale; every current Adopt/Adapt has explicit license compatibility, owner, target, risk, tests, and evidence | Non-zero exit and exclude invalid records from governed totals. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-10,D-21] |
| Every `verified` subject has matching current evidence at the subject/source/target revision | Non-zero exit and recompute the subject as not complete. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-11,D-16,D-18,D-21] |
| Proof level never decreases without an appended event/evidence explanation | Non-zero exit with subject and old/new revisions. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-16,D-21] |
| Snapshot ancestry, source hashes, freshness, and generated view snapshot IDs agree | Non-zero exit; never silently select the newest-looking record. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-04,D-05,D-21,D-22] |
| Recomputed detail counts equal every stored or rendered summary | Non-zero exit, matching existing fail-closed audit behavior. [VERIFIED: kiana-commands/src/audit.rs:1156-1274; scripts/commercial-release-blockers-report.sh] |

## Security Domain and ASVS L1 Mapping

Phase 1 processes untrusted or drift-prone remote source text, repository metadata, license files, JSON records, paths, URIs, and Markdown-bound strings. The trusted boundary is the schema/semantic validator plus normalized file access; generation and reporting occur only after validation. [VERIFIED: AGENTS.md Safety and Research Integrity constraints; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-03,D-04,D-21]

| Control area | Phase 1 threat | Required mitigation |
|---|---|---|
| ASVS V1.2.3 output encoding | Source titles, rationales, paths, or URLs inject Markdown/HTML or corrupt tables | Escape Markdown delimiters and link targets, prohibit raw HTML by default, and test hostile fixture strings before generation. [CITED: https://github.com/OWASP/ASVS/tree/v5.0.0_release] |
| ASVS V2.1.1 documented validation | Fields have ambiguous constraints and inconsistent error handling | Put required/type/enum/pattern rules in versioned schemas and document semantic invariants with stable failure codes. [CITED: https://github.com/OWASP/ASVS/tree/v5.0.0_release] |
| ASVS V2.2.1 allowlist and logical validation | Unknown status/proof/freshness values or impossible combinations enter reports | Use strict enums, reject unknown fields where contracts are closed, and apply the semantic completion predicate centrally. [CITED: https://github.com/OWASP/ASVS/tree/v5.0.0_release] |
| ASVS V2.2.2 trusted-layer validation | A generated view or command consumer bypasses canonical validation | Make every generator/report command invoke the same validator and fail before output on invalid input. [CITED: https://github.com/OWASP/ASVS/tree/v5.0.0_release] |
| ASVS V5.2.1 bounded file size | A very large source/license/evidence file causes resource exhaustion | Enforce documented maximum bytes before reading/hashing and return a structured blocker instead of truncating silently. [CITED: https://github.com/OWASP/ASVS/tree/v5.0.0_release] |
| ASVS V5.2.2 extension/content validation | A path extension claims JSON while content is malformed or points to an unexpected file type | Validate UTF-8 JSON content independently of extension; allowlist evidence URI schemes and declared artifact types. [CITED: https://github.com/OWASP/ASVS/tree/v5.0.0_release] |
| ASVS V5.3.2 path traversal | Repo/source/evidence paths escape the project or governance roots | Normalize and canonicalize paths, reject absolute/temp paths and `..`, and verify resolved paths remain inside allowed roots without following unsafe symlinks. [CITED: https://github.com/OWASP/ASVS/tree/v5.0.0_release] |

Additional project-specific controls:

- Never place API keys, OAuth tokens, cookies, SSH keys, license keys, or unrestricted environment dumps in an evidence record; store only redacted metadata and controlled artifact references. [VERIFIED: AGENTS.md Data and Safety constraints; .planning/REQUIREMENTS.md:27,173]
- Network refresh must honor the shared network policy and fail closed when a required source cannot be retrieved; it may create a blocked/stale record, not reuse an old source as current. [VERIFIED: AGENTS.md Providers and Safety constraints; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-17]
- License compatibility is a review outcome backed by evidence, not an automatic consequence of detecting an SPDX-looking string. [VERIFIED: AGENTS.md Licensing constraint; .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-10]
- Generated views must not expose local absolute home paths or transient artifact paths. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md Agent Discretion]

## Environment Availability

| Capability | Observed state | Planning consequence |
|---|---|---|
| Rust compiler | `rustc 1.96.0` | Matches the workspace minimum and is available if a thin Rust adapter is planned. [VERIFIED: command `rustc --version`; Cargo.toml] |
| Cargo | `cargo 1.96.0` | Focused command tests and formatting are available. [VERIFIED: command `cargo --version`] |
| Python | `Python 3.13.12` | Standard-library validator/generator path is available. [VERIFIED: command `python3 --version`] |
| Bash | GNU Bash `5.1.16` | Existing smoke scripts can host the Phase 1 fixture gate. [VERIFIED: command `bash --version`] |
| Git | `2.34.1` | Revision and drift checks are available locally. [VERIFIED: command `git --version`] |
| jq | `1.6` | Useful for inspection, but not required by the canonical compiler. [VERIFIED: command `jq --version`] |
| Reference inventory | 38 top-level directories | Full registry reconciliation can run locally without network for the frozen baseline. [VERIFIED: command `find reference -mindepth 1 -maxdepth 1 -type d | wc -l` => 38] |
| Existing registry seed | schema object with `expected_count` and 38 `references` entries containing `id`, `path`, and `domains` | Use as migration seed, then make the new canonical registry authoritative. [VERIFIED: docs/agent-program/kiana-completion/references.json; command `jq '{keys:keys,count:(.references|length)}' ...`] |
| Schema smoke | Large existing Bash gate invoking the local Python validator over many contracts and negative cases | Add a narrow Phase 1 slice rather than a second unrelated validation framework. [VERIFIED: scripts/schema-contract-smoke.sh] |
| Typed GSD researcher dispatch | Unavailable in this session; work executed with the injected role preamble through a generic agent | Orchestrator must label this artifact/result as a `generic-agent workaround`. [VERIFIED: active collaboration tool schema and orchestrator task payload] |

## Assumptions Log

| Assumption | Why it is acceptable for planning | How to resolve |
|---|---|---|
| The recommended `docs/agent-program/kiana-completion/governance/` layout is provisional. [ASSUMED] | The locked boundary specifies four objects but delegates exact splitting and prefixes. | Planner chooses exact paths once, then schemas/tests treat them as stable. |
| The fast Phase 1 fixture slice can remain below 30 seconds when it performs no network or full workspace build. [ASSUMED] | Inputs are small JSON fixtures and standard-library operations. | Record elapsed time on the first implementation run and split broad smoke from focused smoke if necessary. |
| A Python-first compiler is sufficient for Phase 1 consumers. [ASSUMED] | Current required outputs are files, validation diagnostics, and generated Markdown, all covered by existing script patterns. | Add a thin Rust adapter only if the plan identifies a concrete CLI/MCP/SDK consumer that cannot invoke the script boundary. |
| Public source snapshots can store source content by controlled URI plus hash rather than vendoring every page. [ASSUMED] | D-04 requires reproducibility metadata but delegates artifact layout. | Planner decides retention policy and adds a fixture proving offline identity verification. |
| `docs/agent-program/kiana-completion/references.json` is migration input, not a permanent second truth source. [ASSUMED] | D-19/D-23 forbid duplicate status authorities, while the current file is only an ID/path/domain seed. | Plan a one-time import and either generate/retire the seed or explicitly constrain it to inventory input. |

## Open Questions

These questions are planner decisions within Agent Discretion and do not block research completion. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md Agent Discretion]

1. Should canonical object families be one JSON file each, snapshot directories, or repo-sharded files with a deterministic index? Recommendation: immutable snapshot directories for public baseline and registry, one revisioned decision set, and one append-only evidence index until scale proves sharding necessary. [ASSUMED]
2. Should the semantic compiler be one script with `validate`, `generate`, `diff`, and `refresh` subcommands or two focused scripts? Recommendation: one shared Python module plus thin validator/generator CLIs only if doing so avoids duplicated parsing and error codes. [ASSUMED]
3. Does Phase 1 need a user-facing `kiana audit/report` adapter, or are checked-in generated views and scripts sufficient? Recommendation: defer the adapter unless a success criterion cannot be demonstrated from generated views; if added, consume a serialized summary contract. [ASSUMED]
4. What source retention is required for proprietary public documentation: content blob, normalized excerpt, or URL/version/hash plus reproducible fetch instruction? Recommendation: retain enough immutable content or artifact identity to reproduce a baseline after the public page changes, without storing proprietary non-public material. [ASSUMED]
5. How should content-only reference snapshots compute revision identity? Recommendation: deterministic tree SHA-256 over normalized relative paths and bytes, recorded with `revision_kind`, while stable repo ID remains independent. [ASSUMED]
6. Which existing Markdown files become generated outputs in the first migration, and which remain narrative docs linking to generated views? Recommendation: migrate status-bearing tables first; keep architectural rationale narrative but remove independent completion fields. [ASSUMED]

## Sources and Metadata

### Repository Sources

| Source | Role in this research | Confidence |
|---|---|---|
| `.planning/phases/01-baseline-evidence-governance/01-CONTEXT.md` | Locked decisions, discretion, scope boundary, canonical references, and deferred work. [VERIFIED: file read 2026-07-15] | HIGH |
| `.planning/REQUIREMENTS.md` | Verbatim COD-01/DIF-11 requirements, anti-features, proof hierarchy, and 38/38 acceptance. [VERIFIED: .planning/REQUIREMENTS.md:19,42,140,168-173,189-200] | HIGH |
| `.planning/ROADMAP.md` | Phase goal, dependencies, and three observable success criteria. [VERIFIED: .planning/ROADMAP.md:40-48] | HIGH |
| `.planning/STATE.md` | Confirms Phase 1 is ready to plan with no completed plans. [VERIFIED: .planning/STATE.md] | HIGH |
| `AGENTS.md` | Stack, architecture, ownership, safety, licensing, data, and testing constraints. [VERIFIED: AGENTS.md] | HIGH |
| `docs/agent-program/kiana-completion/references.json` | Current 38-record ID/path/domain seed and expected count. [VERIFIED: command `jq '.references | length' ...` => 38] | HIGH |
| `reference/` | Current local inventory used for one-to-one registry reconciliation. [VERIFIED: command `find reference -mindepth 1 -maxdepth 1 -type d | wc -l` => 38] | HIGH |
| `docs/reference-feature-matrix.md`, `docs/reference-migration-roadmap.md`, and `docs/reference_audit/*` | Migration inputs for capabilities, anchors, gaps, decisions, and narrative rationale. [VERIFIED: files read in canonical-reference pass] | HIGH |
| `docs/commercial-release-readiness.md`, blocker schema, and blocker script | Existing separation of local/external evidence plus fail-closed summary/report pattern. [VERIFIED: docs/commercial-release-readiness.md; docs/schemas/kiana-commercial-release-blockers.v1.schema.json; scripts/commercial-release-blockers-report.sh] | HIGH |
| `scripts/validate-json-schema.py` and `scripts/schema-contract-smoke.sh` | Existing structural validator and contract fixture harness; establishes the no-new-package path. [VERIFIED: files read 2026-07-15] | HIGH |
| `kiana-commands/src/audit.rs` and `kiana-commands/src/report.rs` | Existing machine/human audit and progress integration boundaries. [VERIFIED: files read 2026-07-15] | HIGH |
| `.planning/codebase/CONCERNS.md`, `TESTING.md`, and `STRUCTURE.md` | Dirty-worktree, ownership, smoke, and serialized broad-gate constraints. [VERIFIED: files read in canonical-reference pass] | HIGH |

### External Standards and Public Baseline Metadata

| Source | Use | Metadata / confidence |
|---|---|---|
| `https://code.claude.com/docs/llms.txt` | Discoverable official Claude Code public documentation index for public-parity source capture. | Official source index; not re-fetched during this strict repo-first rerun. [CITED: https://code.claude.com/docs/llms.txt] |
| `https://json-schema.org/draft/2020-12/json-schema-core` and `https://json-schema.org/draft/2020-12/json-schema-validation` | Normative schema vocabulary reference; implementation remains limited to the checked-in validator subset unless expanded explicitly. | Official specification. [CITED: https://json-schema.org/draft/2020-12/json-schema-core] [CITED: https://json-schema.org/draft/2020-12/json-schema-validation] |
| `https://spdx.github.io/spdx-spec/v3.0.1/annexes/spdx-license-expressions/` | License-expression syntax reference; expression parsing alone is not a compatibility decision. | Official SPDX specification. [CITED: https://spdx.github.io/spdx-spec/v3.0.1/annexes/spdx-license-expressions/] |
| `https://github.com/OWASP/ASVS/tree/v5.0.0_release` | ASVS L1 control mapping for input validation, output encoding, file bounds, and traversal. | Official OWASP release tag; not re-fetched during this repo-first rerun. [CITED: https://github.com/OWASP/ASVS/tree/v5.0.0_release] |

### Research Method Metadata

- Method: strict repo-first inspection of locked Phase 1 inputs and their canonical references; no live external fetch or MCP research-plan was needed in the rerun because repository evidence established the implementation boundary. [VERIFIED: orchestrator task payload; files listed above]
- Confidence rule: repository/source-code claims are HIGH; official external documents cited but not fetched again in the rerun are MEDIUM; proposed layout/timing choices are explicitly `[ASSUMED]`. [VERIFIED: gsd-phase-researcher role preamble]

## Planning Checklist

- [ ] Create four separate versioned canonical contracts and positive/negative fixtures. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-19..D-21]
- [ ] Seed immutable public and 38-repo snapshots with source/revision/hash metadata. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-04,D-05,D-09]
- [ ] Implement semantic validation for every D-21 fail-closed case and strict journey/proof aggregation. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-06,D-18,D-21]
- [ ] Generate byte-deterministic public-parity and reference-governance Markdown with `--check`. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-19,D-22]
- [ ] Map COD-01 and DIF-11 to focused offline gates designed for `<30s`, then record measured duration. [VERIFIED: .planning/ROADMAP.md:40-48] [ASSUMED]
- [ ] Add an audit/report adapter only if needed for the observable success criteria. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md D-24]
- [ ] Keep capability fixes, continuous monitoring, target environments, cloud/enterprise, and user acceptance out of Phase 1. [VERIFIED: .planning/phases/01-baseline-evidence-governance/01-CONTEXT.md Deferred Ideas]

---

*Phase: 01-baseline-evidence-governance*
*Research mode: generic-agent workaround, strict repo-first rerun*
