# CM-12 统一 sparse/dense/RRF/MMR 检索器基线

> 快照日期：2026-09-20。本页记录 unified retrieval source slice；夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CM-12`](context-memory.md#step-cm-12) |
| feature_status | `implemented`（domain algorithm/profile + query RetrievalPort adapter + CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| canonical path | server-derived scope/ACL filter → exact + BM25-CJK + optional dense → RRF-k60 → MMR → stable RetrievalResult |
| authority | ranking is read-only derived context evidence; it cannot authorize a read, write, model call or promotion |

## Contract and behavior

`RetrievalRequest` refuses an unfiltered request and binds a server-derived permission scope
digest. `rank_retrieval` removes disallowed items before document frequency, sparse/dense ranking
or MMR; all remaining items must carry the same scope digest. `RetrievalProfile` pins the
algorithm version, BM25 parameters, RRF-k=60 and MMR lambda. `RetrievalResult` records exact,
BM25, dense, sparse/dense ranks, RRF/MMR scores, degraded reasons, source digests and stable
tie-break output. `kiana-query::UnifiedRetrievalPort` exposes the same implementation to adapters.

## CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `acl_filter_happens_before_ranking` | a high-scoring denied item cannot affect returned ranking |
| `unified_ranking_is_stable_and_uses_versioned_rrf_mmr_profile` | same snapshot/request gives byte-equivalent pinned profile result |
| `unfiltered_request_is_rejected_before_any_score_is_computed` | missing ACL filter fails before scoring |
| `cli_and_memory_tool_have_identical_rankings` | direct and RetrievalPort adapter use identical ranking |
| `unified_retrieval_keeps_acl_first_and_one_algorithm_profile` | daemon/query/legacy adapter boundary is source-guarded |

## Proof ceiling and handoff

CM-12 proof ceiling is `source` plus remote CI wiring. Existing daemon legacy ranker and CLI
memory search remain compatibility adapters pending full call-site migration; no vector model
quality, external embedding, durable index or live retrieval claim is made. CM-13/14 handle repo
map explanations and provenance/health completion.
