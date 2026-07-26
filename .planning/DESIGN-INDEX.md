# Kiana 设计文档权威索引（DESIGN-INDEX）

**Created:** 2026-07-26（规划体系增强，见 `docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md`）
**规则：** phase 进入 discuss/research 时必须从本索引拉取该阶段的权威设计；索引与文档现状不符时先修索引再规划。特性账本（`.planning/features/`）的 `设计引用` 字段只能引用本索引中的条目。

## 层级定义

| Tier | 含义 | 使用方式 |
|------|------|----------|
| T0 权威宪法 | 定义范围、需求、约束与路线的唯一权威 | 冲突时最终裁决源 |
| T1 领域权威设计 | 各子系统的当前有效设计 | 规划与实现的直接依据 |
| T2 治理与账本 | 冻结基线、能力决策、发布证据 | 完成判定与基线对照的事实源 |
| T3 历史专项/证据 | 已实现入代码或已被后续设计覆盖 | 只作背景与证据，不作新范围依据 |

## T0 权威宪法

| 文档 | 覆盖 | 状态 |
|------|------|------|
| `.planning/PROJECT.md` | 项目定位、约束、Key Decisions、非 TDD 执行规则 | 权威 |
| `.planning/REQUIREMENTS.md` | 104 项 v1 需求（含 NFR）、验收词汇表、anti-features、traceability | 权威 |
| `.planning/ROADMAP.md` | 24 阶段、依赖链、成功标准 | 权威 |
| `.planning/MILESTONES.md` | M0-M6 发布列车与退出门禁 | 权威 |
| `docs/superpowers/specs/2026-07-14-kiana-complete-ai-agent-product-design.md` | 产品总纲：架构、执行模型、包、入口、商业门禁 | 权威 |

## T1 领域权威设计

| 文档 | 覆盖需求域 | 主要消费阶段 |
|------|-----------|--------------|
| `docs/superpowers/specs/2026-07-17-kiana-control-plane-architecture-design.md` | 控制平面：Client→Protocol→Daemon→Core→Broker | 3-9 |
| `docs/superpowers/plans/2026-07-18-kiana-command-query-migration.md` | Command/Query 迁移执行序 | 3-6 |
| `docs/kiana-product-functional-and-implementation-design.md` | F01-F12 产品功能地图 + Intent Router/Scheduler/事件一致性 + P0 纵向切片 | 6, 8-11, 16 |
| `docs/kiana_project_os/01..36-*.md`（36 篇分册） | Project OS 全量：workflow/state machine(02)、intent router(03)、WBS/看板(04)、evidence/review(05)、context/memory(06)、bounded swarm(07)、trust/plugin/MCP(08)、EDA(09,26)、交付切片(10)、数据契约/事件税则(11)、CLI/TUI(12)、plugin 生态(13)、MCP provenance(14)、安全/商业(15)、报告/记忆治理(16)、企业/云(17)、reference playbook(18)、测试评测(19)、roadmap 治理(20)、调度(21)、错误税则(22)、artifact 保留(23)、命令目录(24)、gate 引擎(25)、worker 集成(27)、记忆形成(28)、报告模板(29)、实现治理(30)、board 数据模型(31)、WorkPacket schema(32)、审批协议(33)、领域规则注入(34)、dashboard 投影(35)、release proof(36) | 全阶段按册对应 |
| `docs/workflow-runtime-design.md` | WorkflowRun/EventLog 运行时 | 4, 8 |
| `docs/sdk-runtime-events.md` | RuntimeEvent SDK 合同 | 3, 16 |
| `docs/schemas/*.schema.json` | 全部 pinned JSON 合同（runtime event、blockers、signature、parity、swarm、eval 等） | 3 及消费方 |
| `docs/superpowers/specs/2026-07-16-capability-governance-supervisor-design.md` | 治理监督器（Rust supervisor） | 2, 治理维护 |
| `docs/superpowers/specs/2026-07-11-offline-eval-harness-design.md` | 离线 eval 合同（`kiana eval run`） | 8, 16（NFR-01 回归门禁复用） |

## T2 治理与账本（完成判定事实源）

| 文档/目录 | 内容 | 消费规则 |
|-----------|------|----------|
| `docs/agent-program/kiana-completion/governance/` | 四 family 治理：public-baselines（15 journeys/49 capabilities，含 target_phase）、repository-registry、capability-decisions、evidence；`current.json` selector | Coding parity 唯一权威；`.planning/journeys/coding.md` 是其阶段反查索引 |
| `docs/agent-program/kiana-completion/`（acceptance/baseline/dags/tasks/*.json） | agent program 任务与验收数据 | 历史 program 数据，读取需对照 governance |
| `docs/reference-feature-matrix.md` | reference→Kiana 域迁移状态 + 商业硬化记录 | 基线对照（FEAT `当前基线` 字段） |
| `docs/reference_audit/` | 38 reference 逐仓审计 | Adopt/Adapt/Reject 背景 |
| `docs/reference-migration-roadmap.md` | core-loop-first 迁移依据 | 历史排序依据 |
| `docs/commercial-release-readiness.md` | 本地发布机制现状 + 外部 blocker 清单 | 基线对照；Phase 2/24 直接输入 |
| `docs/release-checklist.md`、`docs/distribution-channels.md` | 发布操作与渠道 | Phase 2, 19, 24 |
| `docs/proof-templates/` | 非接受态证明模板 | 发布 phase |
| `docs/eval/fixtures/` | eval 基线 fixture | NFR-01/eval 门禁 |

## T3 历史专项设计与实现计划（证据层）

`docs/superpowers/specs/` 其余各篇（bounded-swarm 系列、evidence-ledger、recovery-journal、workflow-integrity、project-trust、EDA netlist/workbench、project-board、hundred-agent program、personal-project-os 2026-07-09 版）与 `docs/superpowers/plans/` 全部 25 篇：**已实现入代码或被 T0/T1 覆盖**。作为设计决策证据与实现背景阅读；新增范围以 T0/T1 为准。`docs/kiana-personal-project-os-complete-design.md`（11411 行单册版）已被 `docs/kiana_project_os/` 36 篇分册取代为分册优先，单册保留作全景叙述。

## 阶段 → 必读设计映射

| Phase | 必读（T0 全体默认必读，此处列增量） |
|-------|--------------------------------------|
| 2 | commercial-release-readiness、capability-governance-supervisor、release-checklist、distribution-channels |
| 3 | control-plane 架构、sdk-runtime-events、schemas/、project_os 11 |
| 4 | workflow-runtime-design、project_os 02/23/28 |
| 5 | project_os 08/33、trust 相关 T3 证据 |
| 6 | functional-design §8(F04)/§17、project_os 03/06/21 |
| 7 | functional-design §17-18、project_os 03/21 |
| 8 | workflow-runtime-design、project_os 02/05/22/25、offline-eval-harness |
| 9 | project_os 07/13/27/32、functional-design §11(F07) |
| 10 | functional-design §5-§9(F01-F05)/§12(F08)/§13(F09)、project_os 04/05/06、governance parity(memory/checkpoint 域) |
| 11 | functional-design §10(F06)/§15(F11)、project_os 13/14、governance parity(chrome/git/ci 域) |
| 12-13 | project_os 09/26/34、EDA T3 specs、RES 需求组 |
| 14-15 | project_os 33/34、DAY 需求组 |
| 16 | project_os 12/24、sdk-runtime-events、governance parity(cli/commands/mcp/sdk 域) |
| 17 | governance parity(ide 域)、SURF-04 |
| 18 | project_os 35、governance parity(desktop/web 域) |
| 19 | project_os 17、governance parity(install/voice/surfaces 域)、distribution-channels |
| 20-21 | project_os 17、CLOUD 需求组 |
| 22-23 | project_os 15/17/36、ENT 需求组 |
| 24 | project_os 36、commercial-release-readiness、proof-templates |

---
*Maintained by: 规划体系增强流程。新设计文档合入时必须同时更新本索引。*
