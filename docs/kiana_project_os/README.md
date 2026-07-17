# Kiana 个人项目操作系统规格库

日期：2026-07-09

本目录是 Kiana “个人项目操作系统”方向的深度设计规格库。它不是代码实现计划，也不是营销文档，而是软件逻辑、功能边界、状态模型、路由模型、并行执行、权限策略、验证交付和 EDA 领域流程的产品/架构级设计集合。

主设计文档仍然保留在：

- `docs/superpowers/specs/2026-07-09-kiana-personal-project-os-design.md`

参考审计和 38 个 `reference/` 目录覆盖矩阵保留在：

- `docs/reference_audit/kiana_personal_project_os_reference_audit_2026-07-09.md`

## 1. 规格库目标

这套规格库要回答四类问题：

1. Kiana 到底是什么软件，不是什么软件。
2. 每个功能模块的职责、输入、输出、状态和失败路径是什么。
3. 长期项目如何被拆解、恢复、并行推进、验证和汇报。
4. 后续实现时，每个模块如何验收，而不是只留下抽象口号。

这里的“上万行体量”不是靠堆叙述，而是靠分册化展开：

- 每个核心域单独成卷。
- 每卷明确对象、命令、状态机、事件、边界、失败场景、验收。
- 主规格只保存产品级决策。
- 深度细节进入本目录，避免主文档读不动。

## 2. 文档分册

| 分册 | 文件 | 主题 |
| --- | --- | --- |
| Volume 01 | `01-system-overview-and-product-logic.md` | 产品逻辑、用户模型、模式边界、端到端闭环 |
| Volume 02 | `02-workflow-runtime-and-state-machine.md` | WorkflowRun、DAG、状态机、恢复、一致性 |
| Volume 03 | `03-intent-router-and-command-semantics.md` | Intent Router、L0-L5 档位、命令语义、升级降级 |
| Volume 04 | `04-project-orchestrator-wbs-kanban.md` | WBS、Kanban、Task Card、Project Board、next task |
| Volume 05 | `05-evidence-verification-review-ledger.md` | Evidence Ledger、Verification Gate、Review、Report |
| Volume 06 | `06-context-memory-repo-intelligence.md` | ContextPack、Memory、Repo Map、GitNexus/graphify 吸收 |
| Volume 07 | `07-bounded-swarm-worker-model.md` | bounded swarm、WorkPacket、path lock、worker 集成 |
| Volume 08 | `08-trust-policy-plugin-mcp-hooks.md` | 权限、policy、插件、MCP、hooks、shell、网络 |
| Volume 09 | `09-eda-domain-workflow.md` | 嘉立创/EasyEDA、BOM、DFM、bring-up、硬件 gate |
| Volume 10 | `10-p0-p1-p2-delivery-slices.md` | P0/P1/P2 交付切片、验收、测试证据 |
| Volume 11 | `11-data-contracts-and-event-taxonomy.md` | 数据契约、事件分类、packet 生命周期、兼容策略 |
| Volume 12 | `12-cli-tui-product-surface.md` | CLI/TUI 产品面、命令输出、状态投影、交互原则 |
| Volume 13 | `13-plugin-skill-ecosystem-design.md` | 插件、skill、command、agent、rules、marketplace 生态 |
| Volume 14 | `14-mcp-tooling-and-provenance.md` | MCP server/tool/resource/prompt、可见性、来源、失败策略 |
| Volume 15 | `15-security-audit-and-commercial-readiness.md` | 安全审计、商用化门禁、release proof、企业交付 |
| Volume 16 | `16-reporting-learning-and-memory-governance.md` | 中文汇报、学习闭环、memory 治理、stale 管控 |
| Volume 17 | `17-enterprise-offline-and-cloud-workspace.md` | 企业离线、云端 workspace、remote worker、同步边界 |
| Volume 18 | `18-reference-to-feature-playbook.md` | 38 个参考仓库到 Kiana 功能的落地剧本 |
| Volume 19 | `19-testing-evaluation-and-smoke-strategy.md` | 测试、评估、smoke、fixture、验收证据策略 |
| Volume 20 | `20-roadmap-governance-and-scope-control.md` | 路线治理、scope 控制、版本节奏、反膨胀规则 |
| Volume 21 | `21-scheduler-dependency-and-priority-model.md` | 调度器、依赖图、优先级、ready 队列、critical path |
| Volume 22 | `22-error-failure-and-blocker-taxonomy.md` | 错误分类、失败恢复、blocker 分级、重试与升级 |
| Volume 23 | `23-artifact-storage-retention-and-redaction.md` | artifact 存储、保留策略、压缩、脱敏、导出 |
| Volume 24 | `24-command-catalog-and-output-contracts.md` | 命令目录、输入输出契约、JSON 形态、人类摘要 |
| Volume 25 | `25-gate-engine-and-quality-profiles.md` | Gate 引擎、quality profile、verify/review/ship 门禁 |
| Volume 26 | `26-eda-rules-deep-checklists.md` | EDA 深度规则：电源、接口、BOM、DFM、bring-up |
| Volume 27 | `27-worker-integration-and-conflict-resolution.md` | worker 集成、冲突分类、merge policy、postflight |
| Volume 28 | `28-memory-formation-and-stale-invalidation.md` | memory 形成、置信度、失效、召回与治理 |
| Volume 29 | `29-report-template-library.md` | 中文报告模板库：进度、审计、交接、EDA、商用化 |
| Volume 30 | `30-implementation-governance-without-code.md` | 非代码实施治理：规格转计划、验收映射、变更控制 |
| Volume 31 | `31-project-board-data-model-and-queries.md` | Project Board 数据模型、查询、视图投影、项目管理语义 |
| Volume 32 | `32-workpacket-schema-deep-dive.md` | WorkPacket 深潜：字段、边界、拆分、合并、验收 |
| Volume 33 | `33-approval-and-human-decision-protocol.md` | 人工决策协议、approval、decision log、灰区处理 |
| Volume 34 | `34-domain-rules-and-conditional-injection.md` | 领域规则、条件注入、rule pack 冲突与适用范围 |
| Volume 35 | `35-dashboard-projection-model.md` | Dashboard 投影模型、指标、状态摘要、报告面 |
| Volume 36 | `36-release-proof-and-commercial-blocker-ledger.md` | release proof、商用阻塞账本、交付完整性 |

本次先落 Volume 01-10 的第一版深度设计。后续继续扩展时，应按卷追加，而不是把所有内容塞回主规格。

## 3. 设计原则

### 3.1 软件逻辑优先

本规格库只描述：

- 功能行为。
- 数据对象。
- 状态迁移。
- 事件语义。
- 命令语义。
- 权限边界。
- 并发约束。
- 验收证据。

不直接写：

- Rust 代码。
- 数据库建表语句。
- 具体 UI 像素稿。
- 具体模型 prompt 全量模板。
- 自动生成的 mock 数据。

### 3.2 Evidence-first

Kiana 的任何 Done 状态都必须有 evidence。Evidence 可以是：

- 命令结果。
- 测试结果。
- diff 摘要。
- 审查 finding。
- 用户批准记录。
- 外部文件校验。
- EDA 资料检查结果。

没有 evidence 的完成声明视为未完成。

### 3.3 Live truth 优先

优先级：

1. 当前文件和 git 状态。
2. 当前命令/测试/构建输出。
3. 当前 schema 和 release proof。
4. 当前 workflow eventlog。
5. 当前 memory 中有来源和置信度的记录。
6. 旧文档和旧总结。

旧 memory 只能帮助检索，不允许覆盖 live truth。

### 3.4 小任务轻流程，大任务重协议

Kiana 不应该把所有请求都拖进完整项目流程：

- L0/L1 快速响应。
- L2 单任务闭环。
- L3 长期项目 OS。
- L4 有界并行。
- L5 高风险治理。

Router 必须能升级，也必须能降级。

### 3.5 并行不是无序

并行的前提：

- task 已 Ready。
- 依赖已满足。
- allowed files 不冲突。
- forbidden files 明确。
- 验证命令明确。
- 集成者唯一。

worker 不能自由扩大范围。

## 4. 规格阅读顺序

建议按以下顺序阅读：

1. `01-system-overview-and-product-logic.md`
2. `02-workflow-runtime-and-state-machine.md`
3. `03-intent-router-and-command-semantics.md`
4. `04-project-orchestrator-wbs-kanban.md`
5. `05-evidence-verification-review-ledger.md`
6. `06-context-memory-repo-intelligence.md`
7. `07-bounded-swarm-worker-model.md`
8. `08-trust-policy-plugin-mcp-hooks.md`
9. `09-eda-domain-workflow.md`
10. `10-p0-p1-p2-delivery-slices.md`
11. `11-data-contracts-and-event-taxonomy.md`
12. `12-cli-tui-product-surface.md`
13. `13-plugin-skill-ecosystem-design.md`
14. `14-mcp-tooling-and-provenance.md`
15. `15-security-audit-and-commercial-readiness.md`
16. `16-reporting-learning-and-memory-governance.md`
17. `17-enterprise-offline-and-cloud-workspace.md`
18. `18-reference-to-feature-playbook.md`
19. `19-testing-evaluation-and-smoke-strategy.md`
20. `20-roadmap-governance-and-scope-control.md`
21. `21-scheduler-dependency-and-priority-model.md`
22. `22-error-failure-and-blocker-taxonomy.md`
23. `23-artifact-storage-retention-and-redaction.md`
24. `24-command-catalog-and-output-contracts.md`
25. `25-gate-engine-and-quality-profiles.md`
26. `26-eda-rules-deep-checklists.md`
27. `27-worker-integration-and-conflict-resolution.md`
28. `28-memory-formation-and-stale-invalidation.md`
29. `29-report-template-library.md`
30. `30-implementation-governance-without-code.md`
31. `31-project-board-data-model-and-queries.md`
32. `32-workpacket-schema-deep-dive.md`
33. `33-approval-and-human-decision-protocol.md`
34. `34-domain-rules-and-conditional-injection.md`
35. `35-dashboard-projection-model.md`
36. `36-release-proof-and-commercial-blocker-ledger.md`

## 5. 规格完成标准

一个分册达到“可指导实现”的最低标准：

- 每个功能对象都有定义。
- 每个核心命令有输入、动作、输出、错误。
- 每个状态机有允许迁移和禁止迁移。
- 每个 gate 有 pass/fail/blocked 行为。
- 每个高风险操作有 policy/approval。
- 每个完成声明有 evidence 类型。
- 每个 P0 能力能映射到 schema、命令、测试和验收输出。

## 6. 后续扩展路线

这套规格库可以继续扩到上万行，建议按以下批次追加：

| 批次 | 目标 | 内容 |
| --- | --- | --- |
| Batch A | 核心运行逻辑 | Runtime、Router、State、Evidence、Policy |
| Batch B | 项目管理逻辑 | WBS、Kanban、Milestone、Backlog、Report |
| Batch C | 智能体并行 | WorkPacket、Worker、Path Lock、Integration |
| Batch D | 知识与代码理解 | Memory、Repo Map、Impact、Graph、Search |
| Batch E | 插件生态 | Plugin、Skill、MCP、Hook、Marketplace Trust |
| Batch F | EDA 领域 | Schematic、BOM、DFM、Gerber、Bring-up |
| Batch G | 产品化 | CLI/TUI、Dashboard、Enterprise Offline、Cloud Workspace |
| Batch H | 项目治理深水区 | Project Board 查询、WorkPacket 深潜、人工决策、规则注入 |
| Batch I | 商用交付闭环 | Dashboard 投影、release proof、commercial blocker ledger |
