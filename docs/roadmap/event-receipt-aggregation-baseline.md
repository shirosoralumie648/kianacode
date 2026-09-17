# ER-12 cost/files/model turns/evidence aggregation 基线

> 快照日期：2026-09-17。本页记录 Receipt aggregation 的 source/CI 边界；聚合只解释
> committed facts，不能把估算、未提交输出或脱敏 artifact 当作现实成本/文件成功。

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-12`](event-receipt-recovery.md#step-er-12) |
| feature_status | `implemented`（strict ReceiptAggregation + receipt integration） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | EventLog committed model/effect/memory/artifact facts；aggregation 是只读 Receipt 子投影 |
| this step does | model turns、known/unknown usage tokens、committed execution count、normalized files_changed、memory hits、hashed evidence/provider refs、verification state、cost-estimate separation |
| this step does not | 不结算预算、不采信 provider estimate、不暴露 raw provider/artifact/output、不把 uncommitted/partial patch 标成 completed、不提供 external/live/physical proof |

## 1. Contract

`kiana-domain::ReceiptAggregation` 绑定 source cursor/event IDs、model turns、committed
executions、input/output tokens、usage_unknown、optional cost_micros/cost_estimated、normalized
relative files、memory hits、hashed evidence/provider refs、verification 和 digest。估算标记
必须有显式 cost value，默认 aggregation 不产生 cost；绝对/非法路径、unknown refs、digest/
version/unknown field 均 fail-closed。

`aggregate_receipt_facts` 只读取 run-scoped persisted events，event ID 去重；仅 `attempted`
model turns、committed execution facts、capability completed changed list、memory search hits
参与聚合。usage 缺字段或 overflow 标记 Partial，effect_known=false 进入 Unknown，未提交
marker/非法文件路径不静默丢弃。evidence/provider refs 只保存 digest，兼容 `receipt_from_events`
附带 aggregation；它不写 EventStore、不调用 Broker、不结算 Budget。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `aggregation_counts_only_committed_facts_and_keeps_refs_hashed` | tokens/model turns/files/memory 只来自 committed facts，paths normalized，raw refs 不外泄 |
| `aggregation_marks_unknown_usage_and_effect_without_settling_estimated_cost` | usage missing/effect unknown/committed false 显式 Partial/Unknown，cost remains None |
| `aggregation_rejects_empty_or_foreign_source` | empty/foreign run source 不伪造零成本/成功 |
| `aggregation_is_strict_and_keeps_estimate_separate` | DTO unknown field、estimate-without-value、绝对 path/version/digest tamper fail-closed |

## 3. Proof ceiling and handoff

ER-12 proof ceiling 为 `source`：只读 aggregation 与 compatibility receipt integration 已由
CI-only fixtures 固化；未运行本地测试。Provider rate card/billing、ArtifactStore provenance/
retention/revoke、changeset durability、统一 result delivery 与 external/live/physical proof
留待 ER-13+、PD/DEP/INT/BQ。
