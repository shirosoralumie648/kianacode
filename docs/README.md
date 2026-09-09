# Kiana 文档总入口

> 本页是 `docs/` 的地图和阅读规则，不重复定义领域字段、状态机或安全条款。
>
> 当前行为、当前版本和当前证据以 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准；其余 CompanyOS 文档主要描述目标规范、架构决策或实施计划——**规范里写的是"要成为什么"，不是"现在是什么"**。

## 0. 第一次来？

- **完全没头绪、术语看不懂** → 先读 [`company-os-overview.md`](company-os-overview.md)（白话总览：心智模型、任务走读、术语词典、FAQ）。
- **只想跑起来用** → [`../USER.md`](../USER.md) 有全部可运行的命令。
- **想知道现在真的做到哪了** → 只看 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md)。
- **发现两份文档说法不一致** → 按下面 §3 的权威顺序取舍：事实压过规范，规范压过计划；不要用愿景覆盖运行事实。

## 1. 先判断你要查什么

| 你的问题 | 第一入口 | 继续阅读 |
|---|---|---|
| 这套系统到底是什么、术语什么意思 | [`company-os-overview.md`](company-os-overview.md) | [`company-os-design.md`](company-os-design.md) |
| 当前到底能运行什么 | [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) | [`../USER.md`](../USER.md)、[`../README.md`](../README.md) |
| CompanyOS 的产品蓝图是什么 | [`company-os-design.md`](company-os-design.md) | [`../COMPANY.md`](../COMPANY.md) |
| 目标、项目、验收这些业务对象怎么定义 | [`company-os-domain-contracts.md`](company-os-domain-contracts.md) | [`company-os-spec-index.md`](company-os-spec-index.md) |
| Runtime、Memory、Workflow、MCP、Cache 怎么设计 | [`company-os-platform-architecture.md`](company-os-platform-architecture.md) | [`company-os-reference-matrix.md`](company-os-reference-matrix.md) |
| UI 和交互如何呈现状态 | [`company-os-ui-ux.md`](company-os-ui-ux.md) | [`../USER.md`](../USER.md)、[`company-os-platform-architecture.md`](company-os-platform-architecture.md) |
| 身份、调度、成本、恢复、数据治理怎么管 | [`company-os-operations-governance.md`](company-os-operations-governance.md) | [`company-os-security-constitution.md`](company-os-security-constitution.md) |
| 如何评测、防退步、扩展插件 | [`company-os-quality-ecosystem.md`](company-os-quality-ecosystem.md) | [`company-os-reference-matrix.md`](company-os-reference-matrix.md) |
| 现在该实现什么、验收标准是什么 | [`company-os-implementation-outline.md`](company-os-implementation-outline.md) | [`../PHASES.md`](../PHASES.md)、[`../PROCESS.md`](../PROCESS.md) |
| 接下来每一步做什么、谁做、怎么算做完 | [`roadmap.md`](roadmap.md) | [`company-os-implementation-outline.md`](company-os-implementation-outline.md)、[`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) |
| 什么事绝对不能发生（安全底线） | [`company-os-security-constitution.md`](company-os-security-constitution.md) | [`company-os-platform-architecture.md`](company-os-platform-architecture.md) |
| Coding 能力哪些完成了、哪些冻结了 | [`coding-pack-matrix.md`](coding-pack-matrix.md) | [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) |
| 参考项目为什么这样取舍 | [`company-os-reference-matrix.md`](company-os-reference-matrix.md) | [`reference-agent-audit/README.md`](reference-agent-audit/README.md)、[`reference-agent-audit/00-unified-agent-flow.md`](reference-agent-audit/00-unified-agent-flow.md) |
| 哪些对象已有机器可校验的 schema | [`schemas/README.md`](schemas/README.md) | [`company-os-spec-index.md`](company-os-spec-index.md) |

## 2. 文档地图：每份文档一句话

```text
docs/
├── 入门导读
│   └── company-os-overview.md ······· 白话总览：比喻、走读、术语词典、FAQ（非规范）
│
├── 产品与业务（"为什么做、交付什么"）
│   ├── company-os-design.md ········· 总蓝图：北极星、部门模型、核心对象、风险分级、路线图
│   └── company-os-domain-contracts.md 业务对象合同：Objective → Project → Acceptance → Outcome
│
├── Agent 平台（"怎么运行、怎么扩展"）
│   ├── company-os-platform-architecture.md 九个平台平面：Runtime/Memory/MCP/Workflow/Swarm…
│   ├── company-os-operations-governance.md 长期运行保障：身份/调度/成本/恢复/数据治理
│   └── company-os-quality-ecosystem.md ···· 质量与生态：评测/反馈/版本治理/插件/供应链
│
├── 体验（"用户看到什么"）
│   └── company-os-ui-ux.md ·········· CLI/Web/Desktop 的交互规范：状态卡/审批卡/取消/错误
│
├── 安全与注册表（"底线和户口本"）
│   ├── company-os-security-constitution.md 12 条安全宪法 + 风险矩阵 + 负向验收 + 发布门
│   ├── company-os-spec-index.md ····· 规范索引：概念归谁管（canonical owner）、依赖边界
│   └── schemas/ ····················· 机器可校验的 JSON schema 合同
│
├── 功能走读（现实层，非规范）
│   └── features/ ··················· 每篇讲一个功能的代码真实现状（01–10，索引见 features/README.md）
│
├── 实施与审计（"下一步做什么、凭什么算完成"）
│   ├── roadmap.md ·················· 逐步执行路线图：每步做什么/谁做/验收/门禁（可执行待办）
│   ├── company-os-implementation-outline.md 工程切片 A–M、验收条件、Gate 0、90 天序列
│   ├── coding-pack-matrix.md ········ v1.0 Coding pack 审计底表：逐行为的现状/owner/证据
│   └── reference-agent-audit/ ······· 26 个外部 Agent 项目的源码审计（只作设计参考）
│
└── 根目录相关：CURRENT_STATUS.md（当前事实）、USER.md（可运行命令）、
    COMPANY.md（组织愿景）、DESIGN/PROCESS/PHASES.md（版本/流程/阶段）
```

各层内容有硬边界（完整规则见各文档自身，这里只列最容易踩的）：

- **当前事实**：只记录已绑定源码快照、精确命令和证据的事实；规范里的 `target`/`deferred`/`not_supported` 不得反推为已完成；
- **产品与业务**：定义 Sponsor、Objective、Project、WorkPacket、Acceptance 等公司事实；不定义 Provider 流格式，也不定义具体 UI；
- **Agent 平台**：定义 Session/Run、Memory、Capability/MCP、Workflow、Swarm 等平台能力；必须复用同一个 ControlPlane、Event Store 和 Receipt 体系；
- **体验**：UI 只能投影 State/Event，不能创建第二个 Agent loop 或权限边界；
- **安全与注册表**：安全宪法优先于任何便利功能；领域字段和状态机只在 canonical owner 处定义；
- **实施与审计**：实施大纲里的 `partial`/`target` 不改变当前状态账本；参考项目的代码、文档和规模不能证明 Kiana 的实现状态，也不应被复制成第二套 runtime。

## 3. 文档权威顺序

遇到内容冲突时按以下顺序处理，**不要用愿景文档覆盖运行事实**：

```text
1. 源码、精确测试回执和 CURRENT_STATUS.md
2. README.md / USER.md 的当前用户面
3. 安全宪法和已批准的 ADR
4. CompanyOS 领域与平台规范
5. 实施大纲、PHASES.md、PROCESS.md
6. reference/ 和 reference-agent-audit/ 审计材料
```

各层回答的问题不同：

- **当前事实**回答"现在能不能运行"；
- **规范**回答"目标必须满足什么合同"；
- **架构**回答"各模块应该如何协作"；
- **实施计划**回答"先做什么、如何验收"；
- **参考审计**回答"哪些外部设计值得借鉴"，不证明 Kiana 已实现这些能力。

## 4. 状态和证据的两个维度

所有能力必须同时记录两个维度（白话解释和做菜比喻见 [`company-os-overview.md`](company-os-overview.md) §6）：

```text
feature_status:
  implemented | partial | target | deferred | not_supported

proof_level:
  source | local_behavior | durable | live | physical
```

含义：

- `implemented`：代码路径存在，但仍需结合证明等级判断可信范围；
- `partial`：存在局部能力或已知绕过；
- `target`：规范目标，尚未形成可用实现；
- `deferred`：明确延后；
- `not_supported`：当前必须拒绝或显示不可用；
- `source`：只能从源码或类型看出意图；
- `local_behavior`：固定本地入口和 fixture 可复现；
- `durable`：重启、崩溃、重放后仍可证明；
- `live`：真实外部 Provider 或服务证据；
- `physical`：真实设备或物理效果证据。

以下推断永远不成立：

```text
有类型 ≠ 已强制
单元测试通过 ≠ local_behavior
local_behavior ≠ durable
Receipt 存在 ≠ 现实世界结果正确
reference 有实现 ≠ Kiana 已实现
```

## 5. 变更规则

每次 CompanyOS 文档或代码变更都必须回答：

```text
影响哪些 canonical 对象？
影响哪些 crate 和入口？
改变哪些状态、命令、事件或 Receipt？
是否改变权限、数据范围、成本或副作用？
如何迁移和回滚？
用什么精确测试和证据证明？
当前 feature_status / proof_level 是什么？
```

文档编辑规则：

1. 领域字段和状态机只在对应 canonical contract 中定义；
2. 其他文档通过链接引用，不复制完整语义；
3. 当前事实只在 `CURRENT_STATUS.md` 和用户面文档声明；
4. Reference 结论必须标明"吸收、选择性吸收或排除"；
5. 新能力先写 deny、unknown、replay、cancel、恢复和数据边界，再写 happy path；
6. 任何 schema 破坏性变化都必须有 major version、upcaster 或迁移窗口；
7. 不因为代码行数、crate 数量或参考目录大小而提升产品状态；
8. 各规范文档顶部的《本文速览》是非规范导读，规范语义变化时应同步更新它，但它本身不构成合同。

## 6. 推荐阅读路径

### 新读者 / 新贡献者

```text
company-os-overview.md（白话总览）
  → CURRENT_STATUS.md（现在做到哪了）
  → USER.md（跑一遍试试）
  → company-os-design.md（总蓝图）
  → company-os-domain-contracts.md
  → company-os-platform-architecture.md
  → company-os-security-constitution.md
  → company-os-implementation-outline.md
```

### 要实现 Runtime/平台能力

```text
CURRENT_STATUS.md
  → company-os-platform-architecture.md
  → company-os-operations-governance.md
  → company-os-quality-ecosystem.md
  → company-os-spec-index.md
  → reference-agent-audit/00-unified-agent-flow.md
  → reference-agent-audit/99-kiana-mapping.md
```

### 要实现 Company 业务闭环

```text
company-os-design.md
  → company-os-domain-contracts.md
  → company-os-security-constitution.md
  → company-os-implementation-outline.md (Slice I)
```

### 要评估参考项目设计

```text
company-os-reference-matrix.md
  → reference-agent-audit/README.md
  → reference-agent-audit/00-unified-agent-flow.md
  → 对应的编号审计文件
```

## 7. 当前主线

全局实施阶段序列（P0–P6）的唯一 canonical 是 [`company-os-spec-index.md`](company-os-spec-index.md) §7；本节不维护第二套 P 编号，阶段含义和先后顺序以该节为准。

当前最小可验证主线不是"先实现所有功能"，而是先补齐事实源、canonical schema、runtime ledger、身份、恢复和评测的缺口；不应通过新增 UI、百级工具、自由消息总线或外部副作用来掩盖这些缺口。
