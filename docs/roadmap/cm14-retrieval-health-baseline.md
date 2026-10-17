# CM-14 检索 provenance、freshness 与 health 基线

> 快照日期：2026-09-20；2026-10-17（CM-14 切片）更新，本页记录 retrieval evidence/health source
> slice 与统一检索合同接线；夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CM-14`](context-memory.md#step-cm-14) |
| feature_status | `implemented`（domain response/evidence/health contracts + unified ranker wiring + daemon/CLI 输出 + CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| canonical path | RetrievalItemProvenance(source ref/revision/generation/freshness/evidence) → rank_retrieval（每个命中带 RetrievalEvidence + rank components）→ RetrievalResult(source_generation/health/empty_reason) → RetrievalHealth → RetrievalResponse；daemon memory tool 输出 `retrieval_evidence`/`retrieval_health`，CLI memory search 输出 `retrieval_evidence`/`retrieval` 块 |
| authority | response is derived read evidence; it cannot authorize access, promote context authority or claim external truth |

## Contract and behavior

`RetrievalEvidence` requires source reference, revision, source snapshot digest, generation,
freshness, evidence status and rank components for every hit. `RetrievalHealth` distinguishes
Ready/Degraded/Denied/Unavailable and requires reasons for non-ready states. Retryability is
allowed only with explicit `retry_safe`（`RetrievalRetryPolicy::Deterministic` 不可重试，
`Safe` 才同时置位）；an embedding-unavailable path cannot silently become a retryable success.
Empty responses require an explicit reason（`no_candidates_in_scope` / `no_permitted_candidates` /
`result_limit_zero` / `no_query_match`），and `RetrievalResponse` rejects hit generation/rank mismatch.

2026-10-17（CM-14 切片）：统一检索合同（`kiana-domain/src/unified_retrieval.rs`）直接消费这些对象——
候选进入排名前必须携带 `RetrievalItemProvenance`（generation 非零，否则 fail-closed），
每个 `RetrievalHit` 输出 `RetrievalEvidence`，`RetrievalResult` 输出 `source_generation`、
`RetrievalHealth` 与 `empty_reason`，并校验命中证据与 hit/结果逐字段一致（candidate_id/rank/
snapshot digest/generation 上界）。两个产品适配器只读派生、不得自称 `Verified`：daemon
`memory_retrieval.rs` 用 `RetrievalSnapshotContext`（data epoch + 同一次检索共用时间基准）派生
来源证据、把 embedding 不可用等确定性降级写成 Degraded + 原因（不可重试），`harness_memory.rs`
在 memory.search 输出加入 `retrieval_health`；CLI `kiana-commands/src/memory.rs` 用
`RETRIEVAL_UNVERSIONED_GENERATION` + 内容摘要 revision 派生证据，并在 `kiana.memory-search.v1`
输出 `retrieval` 块。命中证据是只读标签：不重新授权、不探测来源、不做新鲜度探测。

## CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `every_hit_has_source_and_generation` | 统一排名器每个命中携带 source/revision/snapshot/generation/rank 证据；证据错位或篡改被拒绝 |
| `embedding_failure_is_visible_and_retryable_only_when_safe` | embedding/dense 通道失败以 Degraded + 原因可见、不可重试；仅显式 `Safe` 策略可安全重试；Denied 不可重试 |
| `empty_results_require_reason_and_never_silently_deny` | 空结果必须带原因（无候选/无匹配），去掉原因即被拒绝 |
| `empty_and_degraded_results_require_reason_and_safe_retry` | Denied/empty 与 degraded/embedding 路径都有解释且受安全重试约束 |
| `generation_mismatch_is_rejected` | response 拒绝来自不同 source generation 的命中 |
| `retrieval_results_keep_generation_provenance_and_safe_health_semantics` | source guard 保持 candidate/evidence/freshness/health 边界 |
| `product_adapters_surface_retrieval_evidence_and_health`（daemon/CLI） | daemon memory tool 与 CLI memory search 输出证据/health；适配器不声称 `Verified`、不内联第二套排名公式 |
| daemon 模块内 `embedding_failure_is_visible_and_retryable_only_when_safe` | 未配置 embedding manifest 时 health=Degraded(`embedding_not_configured`)、稀疏命中仍携带证据 |
| CLI 模块内 `cli_memory_search_explains_empty_results_and_keeps_hit_evidence` | 空结果原因与命中来源证据（`local-memory:*`/内容摘要 revision/非零 generation）可见 |

## Proof ceiling and handoff

CM-14 proof ceiling is `source` plus remote CI wiring. Actual index freshness/health probes,
durable generation recovery, provider embedding quality and external/live availability remain
CM-15+/PD/provider work; no operational SLO claim is made. 本切片未改变模型可见工具面、
执行脊柱或授权路径；daemon/CLI 证据均为调用方声明的只读标签。
