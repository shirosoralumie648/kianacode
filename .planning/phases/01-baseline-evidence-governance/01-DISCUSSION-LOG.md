# Phase 1: 现状基线与证据治理 - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in `01-CONTEXT.md`; this log preserves the alternatives considered.

**Date:** 2026-07-15
**Phase:** 1-现状基线与证据治理
**Areas discussed:** Public-parity 账本粒度、38-repo 治理结构、证据等级与状态机、权威数据与输出形式

---

## Selection Flow

- 用户在 gray-area 选择阶段回复 `1234`，选择讨论全部四个区域。
- Public-parity 第一问中，用户明确选择推荐方案 `1`：旅程与可验证子能力的两级结构。
- 第二问后，用户要求“都按照你推荐的来吧”，授权剩余问题统一采用推荐项。
- 在完整推荐设计汇总后，用户回复 `1`，批准生成 Phase 1 context。

## Public-Parity Ledger

### Canonical record unit

| Option | Description | Selected |
|--------|-------------|----------|
| Journey + testable capabilities | 用户旅程为顶层，command/flag/IDE/MCP 等具体行为为独立子记录 | Yes |
| Journey only | 可读性高，但会隐藏旅程内部的部分覆盖 | No |
| Feature/command only | 证据精细，但无法证明端到端用户旅程 | No |

**User's choice:** Journey + testable capabilities.

### Source authority

| Option | Description | Selected |
|--------|-------------|----------|
| Official public source first | 官方 docs/release notes/help/schema 定义基线，clean-room 实测补充和纠错 | Yes |
| Reproducible observation first | 公开客户端实测优先，官方文档仅作说明 | No |
| Require both | 官方来源与实测同时成立才进入正式基线 | No |

**User's choice:** Recommended option applied by blanket approval.

### Freeze and refresh

| Option | Description | Selected |
|--------|-------------|----------|
| Immutable snapshots + diff | 按日期和产品版本冻结，刷新产生新 snapshot 与差异 | Yes |
| Mutable latest ledger | 始终覆盖同一份当前账本 | No |
| Milestone-only archive | 平时覆盖，里程碑时手工归档 | No |

**User's choice:** Recommended option applied by blanket approval.

### Journey aggregation

| Option | Description | Selected |
|--------|-------------|----------|
| All required children pass | required 子能力全部达到 proof gate 后旅程才完成 | Yes |
| Percentage threshold | 达到一定完成比例即可通过 | No |
| Manual journey status | 由维护者手工给出旅程总状态 | No |

**User's choice:** Recommended option applied by blanket approval.

---

## 38-Repo Governance

### Governance structure

| Option | Description | Selected |
|--------|-------------|----------|
| Repository registry + capability decisions | 仓库身份与能力级 Adopt/Adapt/Reject 分层记录 | Yes |
| One verdict per repository | 每仓库只有一行和一个主判决 | No |
| Capability-only aggregation | 只保留能力，不保存完整仓库登记表 | No |

### Mixed decisions inside one repository

| Option | Description | Selected |
|--------|-------------|----------|
| Independent capability decisions | 同一仓库可同时产生 Adopt、Adapt 和 Reject | Yes |
| Repository-level primary decision | 用一个主决策代表整个仓库 | No |
| Record adopted capabilities only | 丢弃未采用和被拒绝能力的记录 | No |

### Required governance data

| Option | Description | Selected |
|--------|-------------|----------|
| Full governed record | source revision、license、rationale、owner、target、risk、test、evidence 完整且 Reject 必须有理由 | Yes |
| Minimal record | 只保存 source、decision 和 owner | No |
| Free-form audit | 允许各仓库使用不一致的文本结构 | No |

### Upstream change handling

| Option | Description | Selected |
|--------|-------------|----------|
| Event-triggered stale + history | revision/license/source 变化使记录 stale，复审产生新 revision | Yes |
| Periodic full rescan only | 只按固定周期重扫全部仓库 | No |
| Manual discovery | 维护者发现变化后再更新 | No |

**User's choice:** All recommended options applied by blanket approval.

---

## Proof Levels and State

### Model dimensions

| Option | Description | Selected |
|--------|-------------|----------|
| Separate state, proof, freshness | coverage state、proof level、freshness 独立建模 | Yes |
| Single status enum | 一个枚举同时承载实现与证据含义 | No |
| Free-form conclusion | 由报告文本解释状态 | No |

### Proof ladder

| Option | Description | Selected |
|--------|-------------|----------|
| Six ordered levels | `none → source → local_contract → local_behavior → target_environment → user_value` | Yes |
| Local/external only | 仅区分本地和外部证据 | No |
| Per-capability custom levels | 每类能力自行定义证据等级 | No |

### Regression and invalidation

| Option | Description | Selected |
|--------|-------------|----------|
| Downgrade effective status, retain history | 失败复测或环境变化降低当前状态，旧 evidence 仍不可变 | Yes |
| Verified forever | 一旦验证即永久保持 | No |
| Delete invalid evidence | 删除旧记录后重新计算 | No |

### Completion accounting

| Option | Description | Selected |
|--------|-------------|----------|
| Required proof gate | current + verified + required proof level 才进入完成统计 | Yes |
| Implemented counts as done | 代码存在即可完成 | No |
| Weighted score | 不同证据按权重汇总为完成分数 | No |

**User's choice:** All recommended options applied by blanket approval.

---

## Canonical Data and Outputs

### Source of truth

| Option | Description | Selected |
|--------|-------------|----------|
| Schema-first JSON + generated Markdown | JSON 是唯一事实源，Markdown 为确定性人读视图 | Yes |
| Hand-maintained Markdown | 继续以人手编辑的表格和长篇文本为权威 | No |
| Database-only authority | 运行时数据库是唯一事实源 | No |

### Artifact decomposition

| Option | Description | Selected |
|--------|-------------|----------|
| Split by governance object | baseline、repository registry、decisions、evidence index 分层 | Yes |
| One large JSON | 所有记录保存在单个大型文件 | No |
| One file per capability | 每个细粒度能力单独一个文件 | No |

### Drift handling

| Option | Description | Selected |
|--------|-------------|----------|
| Deterministic generation + blocking drift | schema、生成 diff 或手改 generated view 均阻塞 | Yes |
| Markdown may lead temporarily | 允许人读视图暂时领先 canonical JSON | No |
| Release-only check | 只在正式发布时检查一致性 | No |

### Existing artifact migration

| Option | Description | Selected |
|--------|-------------|----------|
| Migrate into canonical records | 既有 matrix/audit 成为输入或生成视图，统一由 audit/report 消费 | Yes |
| Maintain old and new systems | 多套状态长期并行 | No |
| Add another Phase 1 report | 不迁移既有数据，仅新增一份报告 | No |

**User's choice:** All recommended options applied by blanket approval.

---

## Agent Discretion

- Canonical JSON 的精确路径、分片大小和 ID 前缀。
- Validator/generator 采用现有 Rust command、Bash/Python script 或组合实现。
- Generated Markdown 的视觉分组和导航细节。
- Evidence artifact 的物理存储布局，但必须保持受控路径、identity 和 hash 约束。

## Deferred Ideas

- 账本发现的具体能力缺口交由对应后续 phase 实现。
- 持续后台监控所有专有产品和 reference 上游变化，在显式 refresh/freeze 流程稳定后再评估。
- 目标平台、云服务、企业客户和最终用户验收由对应 phase 获取，Phase 1 不制造替代证据。
