# CM-14 检索 provenance、freshness 与 health 基线

> 快照日期：2026-09-20。本页记录 retrieval evidence/health source slice；夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CM-14`](context-memory.md#step-cm-14) |
| feature_status | `implemented`（domain response/evidence/health contracts + CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| canonical path | RetrievalResult/candidate source → RetrievalEvidence(source ref/revision/snapshot/generation/rank) → RetrievalHealth → RetrievalResponse |
| authority | response is derived read evidence; it cannot authorize access, promote context authority or claim external truth |

## Contract and behavior

`RetrievalEvidence` requires source reference, revision, source snapshot digest, generation,
freshness, evidence status and rank components for every hit. `RetrievalHealth` distinguishes
Ready/Degraded/Denied/Unavailable and requires reasons for non-ready states. Retryability is
allowed only with explicit `retry_safe`; an embedding-unavailable path cannot silently become a
retryable success. Empty responses require an explicit reason, and `RetrievalResponse` rejects
hit generation/rank mismatch.

## CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `every_hit_has_source_and_generation` | every hit carries source/revision/snapshot/generation/rank evidence |
| `empty_and_degraded_results_require_reason_and_safe_retry` | Denied/empty and degraded/embedding paths are explained and safe-retry gated |
| `generation_mismatch_is_rejected` | response rejects a hit from a different source generation |
| `retrieval_results_keep_generation_provenance_and_safe_health_semantics` | source guard keeps candidate/evidence/freshness/health boundary |

## Proof ceiling and handoff

CM-14 proof ceiling is `source` plus remote CI wiring. Actual index freshness/health probes,
durable generation recovery, provider embedding quality and external/live availability remain
CM-15+/PD/provider work; no operational SLO claim is made.
