# Kiana 规划体系增强设计（特性层、统一验收、发布列车）

**日期：** 2026-07-26
**状态：** 设计已确认（用户批准四缺口全补 + 全阶段特性级分解 + 路线 B 分层新增）
**适用范围：** `.planning/` 规划体系与 `docs/` 设计追溯；不修改产品代码
**项目定义：** `.planning/PROJECT.md`；**路线图：** `.planning/ROADMAP.md`

## 1. 背景与问题

用户判断：当前规划体系"无法指引 agent 完成一个功能完整且完善可商业化使用的产品"。经过对
PROJECT.md、REQUIREMENTS.md（98 项 v1 需求，100% phase traceability）、ROADMAP.md（24 阶段）、
Phase 1 治理账本（15 条 CC 黄金旅程 / 49 个 capability，带 proof level 与 evidence）、
commercial-release-readiness、以及全部设计文档的调研，确认问题不是"没规划"，而是六个结构性缺口：

1. **阶段捆绑史诗级需求，缺中间分解层。** Phase 10 捆绑 COD-02..09+16 共 9 条复合需求；Phase 13
   捆绑 RES-07..14+DIF-10。ROADMAP 从 phase 直接跳到 JIT plan，agent 每次进入阶段都要重新发明拆分。
2. **成功标准未逐条绑定验收。** proof-level + 具名旅程 + verifier 的纪律只存在于 Coding 的 49 个
   capability 上；其余需求与阶段成功标准的"完成"靠判断，违背 AF-03 要求的证据闭环。
3. **只有 Coding 有黄金旅程账本。** Research/Daily/Surfaces/Cloud/Enterprise 缺少同等严格度的
   具名验收目标。
4. **单一 1.0 里程碑，无中间发布列车。** 第一个终端可用的 Coding 价值要等到 Phase 10（8 个基础
   phase 之后），商业反馈太晚，大爆炸风险高；设计总纲 §11.1 承诺的 Alpha/Beta 在 ROADMAP 中无载体。
5. **当前实现基线未入账。** 仓库已有大量带测试的实现（parity `coverage_state`、
   commercial-release-readiness 记录），但 phase 无"已实现 vs 差距"账目，阶段无法正确定型。
6. **设计文档未可追溯挂接。** F01-F12 功能设计（1712 行）、project-OS 总设计（11411 行）等只在
   phase CONTEXT 采集时临时挂接，一个 phase 可能没拉到权威设计就被规划。

另有横切 NFR 偏弱：性能/大仓扩展、i18n、可访问性、竞品迁移只散落在个别 SURF/CORE 条目或临时门槛中。

## 2. 设计原则

- **稳定层与易变层分离。** 特性的"是什么 + 怎么验收"源自已冻结需求，稳定，现在全阶段补全；
  "改哪些文件、什么顺序"易变，保持 GSD 即时（JIT）规划，进入阶段时才写 PLAN。
- **复用 Phase 1 已验证的账本纪律**（proof level 词汇、journey→capability→evidence 结构），
  不重造工具；内部域账本用轻量 markdown，只有当某域接近真实验收、需要绑定不可变 evidence 时
  才升格为 governance JSON。
- **YAGNI。** 不建 JSON 需求库、不写校验器、不预写远端 PLAN、不改动 24 阶段编号与依赖链。
- **不破坏 GSD 解析。** ROADMAP.md 保留现有段结构（Goal/Depends/Requirements/Success Criteria/
  Plans），只增加小节与链接；REQUIREMENTS.md 保留 traceability 表结构，NFR 作为新需求分组加入。

## 3. 分层结构（文件地图）

```text
.planning/
  PROJECT.md            (不变：项目宪法)
  REQUIREMENTS.md       (增强：+ 验收词汇表, + NFR 需求组, traceability 补 NFR 行)
  ROADMAP.md            (增强：阶段索引；每阶段补 Milestone / Features 链接 / Baseline 提示)
  MILESTONES.md         (新增：M0-M6 发布列车与退出门禁)
  DESIGN-INDEX.md       (新增：设计文档 → 需求/阶段/特性 的权威索引)
  features/
    01-FEATURES.md .. 24-FEATURES.md   (新增：每阶段一份特性账本)
  journeys/
    core.md coding.md research.md daily.md surfaces.md cloud.md enterprise.md
    (新增：领域黄金旅程账本；coding.md 指向并复用 governance/ 的 CC parity 账本)
docs/agent-program/kiana-completion/governance/   (不变：Coding parity 权威，唯一外部冻结源)
```

## 4. 特性层规范（features/NN-FEATURES.md）

每阶段一份，把该阶段需求拆成 FEAT。**FEAT 是稳定的分解单元，不是计划**：它约束 scope 与验收，
不预设实现顺序或文件清单。

模板（字段全部必填，`依赖` 可为 None）：

```markdown
### FEAT-NN-XX — <标题>
- 父需求: <REQ-ID>（一个 FEAT 只挂一个父需求；跨需求支撑写入"依赖"）
- 领域旅程: <domain>.<journey-id>（journeys/<domain>.md 中的具名旅程）
- 描述: <一句用户可观察行为>
- 验收:
  - [<proof_level>] <可判定的验收条目>
  - ...
- Verifier: <验收所需证据类别：diff/命令/测试/回执/引用/审批/实验>
- 当前基线: none | partial | implemented — <一句现状与差距，对照 parity coverage_state
  与 commercial-release-readiness>
- 设计引用: <DESIGN-INDEX 中的具体文档与章节>
- 依赖: <FEAT-ID 列表 | None>
- 状态: pending | in_progress | complete | blocked
```

规则：

- **proof level 词汇（全项目统一）：** `source < local_behavior < target_environment < user_value`。
  任何 FEAT/成功标准在其要求的 proof level 证据到位前不得标 complete（AF-03 的推广）。
- FEAT 编号 `FEAT-NN-XX` 在阶段内递增，永不复用；废弃写明理由并保留。
- 一条 v1 需求的全部 FEAT 完成且父需求 proof 到位，才能在 REQUIREMENTS.md 勾选该需求。
- **与 GSD 流程的关系：** discuss/research/plan 进入阶段时以该阶段 FEATURES 文件为 scope 输入；
  PLAN 的任务引用 FEAT-ID；verifier 按 FEAT 验收逐条核对。FEATURES 文件本身可在阶段 discuss 时
  修订（视为 scope 决策，需记录理由），但不得在执行中静默改验收。
- 远端阶段（尚未进入）的 FEAT 允许粗粒度（3-8 条/需求），进入阶段时细化；细化只能拆分，不能
  删除验收。

## 5. 验收统一与领域旅程账本（journeys/）

- 每个领域一份 markdown 账本，结构：`旅程 → 必备能力点 → required proof level → 归属阶段 →
  当前 gap`。旅程 ID 形如 `coding.verify-loop`、`daily.approval-inbox`。
- **coding.md 不新建清单**，只做索引：权威是 `governance/public-baselines/`（15 journeys /
  49 capabilities，含 target_phase 正向映射）；文件职责是把该映射按阶段反查呈现，并列出
  Kiana 超集旅程（非 CC parity 的差异化部分，如 evidence-first completion）。
- 内部域（core/research/daily/surfaces/cloud/enterprise）**不需要外部源冻结/drift 机制**：
  其目标源自 REQUIREMENTS 与已批准设计，是内部受控目标。当某域接近验收需要绑定不可变
  evidence 时，按 governance README 的四 family 纪律升格为 JSON revision（升格路径预留，
  本次不建）。
- 旅程账本是 FEAT `领域旅程` 字段与 MILESTONES 退出门禁共同引用的具名验收目标。

## 6. MILESTONES.md（发布列车）

不改 24 阶段依赖链，在其上叠加发布列车。每趟列车：包含阶段、退出门禁（引用具名旅程 +
proof level）、已知限制模板（设计总纲 §11.1：Alpha/Beta 门禁不降级）。

```text
M0 Walking Skeleton    Phase 3-6 横切最薄链路：一个请求走完 event→session→policy→
                       单工具执行→typed result，CLI 可演示。证明地基贯通。
M1 Coding Alpha        Phase 10 达标：单仓理解+编辑+验证闭环真实可用。
M2 Coding Beta + 生态  Phase 11 + 16：MCP/多 Agent/Headless/CLI 完整。
M3 Research/Daily Alpha Phase 12-15：三 pack 齐头。
M4 Surfaces Beta       Phase 17-19：IDE/Desktop/Web + 跨入口连续性。
M5 Cloud/Enterprise RC Phase 20-23。
M6 1.0                 Phase 24 全量收敛门禁（已在 ROADMAP，MILESTONES 仅索引）。
```

M0 是关键新增：把第一个纵向可演示成果从 Phase 10 提前到 Phase 6 附近；它不是新 phase，
而是 Phase 3-6 各自 FEAT 中标记 `[M0]` 的最薄子集 + 一条 `core.walking-skeleton` 旅程。

## 7. 横切 NFR（补入 REQUIREMENTS.md）

新增需求组 `NFR`（v1 范围，绑定 phase 与 proof level，进 traceability 表）：

- **NFR-01 性能与大仓扩展**：冷启动（沿用 Phase 1 已证的 <25s 并按发布收紧）、大仓
  repo-map/索引、长会话内存有可测门槛与回归门禁。
- **NFR-02 可观测性基线**：结构化日志/trace/doctor 在所有入口一致（补全 CORE-14/ENT-08
  之外的个人/本地侧）。
- **NFR-03 国际化**：至少中英；UI 字符串外化；CJK/长文本布局验证。
- **NFR-04 迁移与导入**：Claude Code 等竞品的配置/session/memory 导入路径（AF-18 的
  正向承诺面）。
- **NFR-05 可访问性**：键盘导航、screen reader、reduced motion 的可验证门槛（SURF-08
  提级）。
- **NFR-06 打磨级质量门**：错误信息可操作、loading/empty/error/degraded 状态一致，作为
  各 pack 收尾验收项。

归属阶段在实施时定（初步：NFR-01→16、NFR-02→16、NFR-03→18、NFR-04→10、NFR-05→18、
NFR-06→19；最终以 REQUIREMENTS traceability 为准）。

## 8. DESIGN-INDEX.md

一张权威索引表：`设计文档 → 层级（权威/专项/历史证据/叙事）→ 覆盖的需求与阶段 → 状态`。
规则：phase 进入 discuss/research 时必须从该索引拉取其阶段的权威设计；索引与文档现状不符时
先修索引再规划。首版覆盖 docs/ 根、docs/superpowers/specs、docs/superpowers/plans、
docs/schemas、docs/agent-program 治理目录。

## 9. ROADMAP/STATE 的增强方式

- ROADMAP 每个 phase detail 增加三行：`**Milestone**: Mx`、`**Features**:
  .planning/features/NN-FEATURES.md（N 条）`、`**Baseline**: <一句已实现基线提示>`；
  不改 Goal/Depends/Requirements/Success Criteria/Plans 的现有内容与格式。
- Progress 表后追加 Milestone 索引小节。
- STATE.md 在 Accumulated Context 记录本次规划体系升级决策一条。
- REQUIREMENTS.md：新增"验收词汇表"小节（四级 proof level 定义）+ NFR 组 + traceability
  追加 NFR 行；Coverage 计数更新（98 → 104）。

## 10. 落地顺序与本次工作的完成判据

1. DESIGN-INDEX.md + REQUIREMENTS.md 增强（地基）
2. journeys/ 七份领域旅程账本
3. features/01..24-FEATURES.md（Phase 1 为回填简版；Phase 2-9 基础层最细；10-24 按
   "粗粒度但验收完整"标准）
4. MILESTONES.md
5. ROADMAP.md 补列 + STATE.md 记录
6. 自审（占位符扫描/一致性/编号唯一性/引用有效性）后交用户复审

完成判据：

- 每条 v1 需求（含新增 NFR）至少有一个 FEAT 或在 features 文件中显式标记"直达需求，无需拆分"。
- 每个 FEAT 的验收都带 proof level 且引用存在的旅程 ID。
- 每趟里程碑的退出门禁只引用具名旅程与 proof level，无自由文本"基本完成"类措辞。
- ROADMAP/REQUIREMENTS/features/journeys/MILESTONES 交叉引用可解析，无断链。

## 11. 明确边界（本次不做）

- 不改 24 阶段的编号、依赖链与成功标准原文；不插入新 phase。
- 不建 JSON 需求库与规划校验器（预留升格路径）。
- 不预写任何阶段的 PLAN.md，不替代 GSD discuss/research/plan/execute 流程。
- 不修改产品代码、schema 或治理 JSON；governance/ 仍是 Coding parity 唯一权威。
- 不把本设计的新文件当作完成证据：FEAT/旅程/里程碑记录的是目标与验收，完成仍由
  evidence 决定。
