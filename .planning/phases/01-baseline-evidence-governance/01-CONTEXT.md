# Phase 1: 现状基线与证据治理 - Context

**Gathered:** 2026-07-15
**Status:** Ready for planning

<domain>
## Phase Boundary

本阶段只建立 Kiana 的现状事实层和证据治理合同：发布带冻结信息的 Claude Code public-parity ledger；为 38/38 `reference/` 仓库建立可审计的来源、许可证与 Adopt/Adapt/Reject 决策；统一 source、local、target 和 user-value proof 的表达与完成统计规则。

本阶段满足 `COD-01` 与 `DIF-11`，但不修复账本发现的具体功能缺口，不实现新的 Coding、Research、Daily、Cloud 或 Enterprise 能力，也不使用本地测试替代后续 phase 的目标环境和用户验收。

</domain>

<decisions>
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

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project Scope and Approved Product Contract

- `.planning/PROJECT.md` — Kiana 产品边界、clean-room/许可证规则、38-reference 治理要求和完成口径。
- `.planning/REQUIREMENTS.md` — `COD-01`、`DIF-11`、proof-level 原则、Acceptance Criteria 与 anti-features。
- `.planning/ROADMAP.md` — Phase 1 goal、requirements 和三条 observable success criteria。
- `docs/superpowers/specs/2026-07-14-kiana-complete-ai-agent-product-design.md` — 获批的完整 AI Agent 产品设计与 reference-driven 方向。

### Existing Reference Baselines and Audits

- `docs/reference-feature-matrix.md` — 当前实现/迁移证据的长篇人读记录；作为迁移输入，不继续作为独立状态源。
- `docs/reference-migration-roadmap.md` — 既有 core-loop-first 迁移顺序、reference inventory 和验证策略。
- `docs/reference_audit/kiana_personal_project_os_reference_audit_2026-07-09.md` — 38-repo coverage inventory、能力映射和归因规则。
- `docs/reference_audit/kiana_capability_matrix.md` — 现有 capability 分组、source anchors、Kiana anchors 与 gap 结构。

### Proof and Release-Governance Patterns

- `docs/commercial-release-readiness.md` — local proof 与 external/target proof 的现有边界及禁止过度声明的规则。
- `docs/schemas/kiana-commercial-release-blockers.v1.schema.json` — versioned schema、status/count consistency、owner 与 verification artifact 字段模式。
- `scripts/commercial-release-blockers-report.sh` — JSON/human 双输出、local/external blocker 分类、evidence path 和 fail-closed 汇总模式。
- `scripts/schema-contract-smoke.sh` — schema 与实例一致性的现有验证入口。

### Brownfield Codebase Guidance

- `.planning/codebase/CONCERNS.md` — dirty worktree、现有 proof 边界、release blockers 和 completion-theater 风险。
- `.planning/codebase/TESTING.md` — schema/command/smoke 测试模式与确定性 full-workspace gate。
- `.planning/codebase/STRUCTURE.md` — docs、schema、scripts、audit/report 的代码归属和依赖方向。

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets

- `docs/reference-feature-matrix.md`: 已积累大量实现、测试和外部缺口证据，但 75 行约 108 KB、单行极长，适合迁移为 structured records 后生成摘要，不适合继续手工追加。
- `docs/reference_audit/kiana_personal_project_os_reference_audit_2026-07-09.md`: 已给出当前 38-repo inventory 和初步 coverage matrix，可作为 repository registry 的 seed。
- `docs/reference_audit/kiana_capability_matrix.md`: 已有 capability group、reference anchor、Kiana anchor、gap 和 priority，可作为 capability decision records 的 seed。
- `docs/schemas/kiana-commercial-release-blockers.v1.schema.json` 与 `scripts/commercial-release-blockers-report.sh`: 已证明 versioned JSON schema、严格枚举/summary consistency、JSON/Markdown/human 输出和 local/external 分类模式。
- `kiana-commands/src/audit.rs` 与 `kiana-commands/src/report.rs`: 已有 schema-aware audit、blocking finding、progress summary 和 machine/human output 入口，可消费统一 ledger，而无需另建运行时。

### Established Patterns

- 公共机器合同包含稳定 `schema` 标识、严格 enum、结构化错误和 schema smoke fixture。
- Audit/report 在证据不完整或 summary 与明细不一致时 fail closed；local 与 external proof 分开统计。
- 复杂 workflow/evidence 状态采用追加式事实、可重建投影和内容 hash；Phase 1 ledger 应沿用不可变证据与可生成视图的方向。
- 测试先覆盖 focused schema/command behavior，再运行 `scripts/schema-contract-smoke.sh`；本仓 broad Rust gate 因全局状态风险使用 `--test-threads=1`。

### Integration Points

- 新 schema 放在 `docs/schemas/`，有效/无效 fixture 接入 `scripts/schema-contract-smoke.sh`。
- Canonical ledger 的验证和人读输出优先接入 `scripts/` 及现有 `kiana audit` / `kiana report` 边界；不在 `kiana-entrypoints/src/cli.rs` 建立第二套大型路由。
- 生成的人读 baseline/audit 应链接回 canonical record ID 和 evidence，而不是复制无法同步的状态字段。
- 规划和提交必须显式列出文件，保留当前大型 dirty worktree，禁止把用户已有实现混入 Phase 1 原子提交。

</code_context>

<specifics>
## Specific Ideas

- 用户明确选择“旅程 + 可验证子能力”两级 public-parity ledger，并授权其余灰区采用推荐方案。
- 用户要的是可解释的能力超集，不是 Claude Code 的命令/配置兼容克隆；账本必须能表达 intentional difference，而不是把所有差异都误报为缺陷。
- 38/38 coverage 的含义是每个仓库都完成身份、license 和 capability decision 治理，不是要求机械移植每个仓库的所有代码。
- 报告首页应让人立即区分 source existence、local contract、local behavior、target environment 和 user value，避免继续用功能数量或长篇进展文字制造完成错觉。

</specifics>

<deferred>
## Deferred Ideas

- 账本发现的具体 capability 缺口由 ROADMAP 中对应 Phase 2-24 实现和验证，Phase 1 只记录、分类和路由。
- 持续后台监控所有专有产品页面与 38 个 reference 上游变化，不是 Phase 1 的必要条件；本阶段先建立显式、可重复的 refresh/freeze 流程。
- 目标平台、真实云服务、企业客户和最终用户验收证据仍由对应 phase 获取，不能在本阶段制造或替代。

</deferred>

---

*Phase: 1-现状基线与证据治理*
*Context gathered: 2026-07-15*
