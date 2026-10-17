# CM-12 统一 sparse/dense/RRF/MMR 检索器基线

> 快照日期：2026-09-20；2026-10-17 追加完成切片（达 daemon/CLI 产品适配器）。本页记录 unified retrieval source slice；夹具仅由 GitHub Actions 执行。

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

CM-12 proof ceiling is `source` plus remote CI wiring. No vector model quality, external embedding,
durable index or live retrieval claim is made. CM-13/14 handle repo map explanations and
provenance/health completion.

## 2026-10-17 completion slice（CI-only）

Daemon 与 CLI 的本地排名公式已移除，两个产品适配器共享 Domain `rank_retrieval()`：

- `kiana-daemon/src/memory_retrieval.rs` 不再自带 cosine/overlap/RRF/MMR 公式；`rank_records()` 把
  `harness_memory::search_records_scoped` 已完成 collection/role 授权与投影过滤的候选投影成
  `RetrievalItem`（`source_digest` 为正文 SHA-256；scope 摘要由 `kiana_domain::retrieval_scope_digest`
  从候选快照标签派生，只做快照绑定、不是授权），交给 `kiana_query::UnifiedRetrievalPort`
  （`RetrievalProfile::unified()`）排名，再把结果映射回既有 JSON hit 形状。dense 模型加载、
  manifest/hash fail-closed 与降级原因仍由本模块负责。hit 的 `retrieval_algorithm` 标签从
  `bm25-cjk+cosine+rrf60+mmr0.7.v1` 变为统一 profile 的 `exact+bm25-cjk+dense+rrf60+mmr.v1`。
- `kiana-commands/src/memory.rs` 的 `search_memory_records()` 不再按“词项包含计数 + 时间”排序，改为对
  同一批本地记录直接调用 `kiana_domain::rank_retrieval(&RetrievalProfile::unified(), ...)`
  （`kiana-commands` 不依赖 `kiana-query`，manifest/lockfile 不变），构造/排名失败返回错误而不是静默
  丢结果。
- 边界（有意保留）：`kiana-query/src/index.rs` 的 `context.search`/context pack 候选搜索保持行/路径级
  语义（`occurrences`、`score`、CM-00 钉扎与 CM-10/11 夹具依赖它），没有并入文档排名器；上下文选材与
  证据归 CM-13/CM-15。
- CM-00 快照按约定重钉：`kiana-daemon/src/memory_retrieval.rs` 的 SHA-256；另补钉
  `kiana-daemon/src/harness_memory.rs`（`0cc42418` 对 H17/CAP-19 daemon 源码套 rustfmt 后遗留的过期
  钉扎，非 CM-12 行为改动）。
- 守卫 marker 随实现更新：`kiana-daemon/tests/p1_j3_02_memory.rs`、`kiana-daemon/tests/p1_j3_04_hybrid_retrieval.rs`
  （RRF/MMR 公式锚点移到 `kiana-domain/src/unified_retrieval.rs`）、
  `kiana-core/tests/cap19_memory_scope_commit_guard.rs`；`kiana-core/tests/cm12_unified_retrieval_guard.rs`
  新增委托与“无第二套公式”正反锚点。

新增/更新的夹具（全部 CI-only）：

| Fixture | 断言 |
|---|---|
| `cross_scope_candidate_fails_closed_before_scoring`（domain） | 混合 scope 候选在打分前以 `retrieval_item_scope_mismatch` 失败关闭 |
| `dense_channel_joins_rrf_and_dimension_mismatch_is_degraded`（domain） | dense-only 命中进入结果并参与 RRF；全维度不匹配时 `dense_unavailable_or_dimension_mismatch` 且 sparse 继续 |
| `mmr_prefers_a_distinct_hit_over_a_near_duplicate`（domain） | MMR 把不同文本排在重复副本之前，而不是让副本跟在原件后 |
| `retrieval_scope_digest_is_order_insensitive_and_dedupes`（domain） | scope 摘要顺序无关、去重，标签集合变化即摘要变化 |
| `port_keeps_acl_first_before_any_scoring`（query） | port 层拒绝候选只增加计数、不改变允许候选分数；`acl_filtered=false` 请求在 adapter 层 `retrieval_request_invalid` |
| `cli_memory_search_matches_the_unified_retrieval_order`（kiana-commands 单测） | CLI 顺序与统一算法逐条一致（旧“计数 + 时间”排序会得出不同顺序） |
| `memory_tool_ranking_matches_the_unified_retrieval_port`（daemon 单测） | memory tool hit 与同一 `RetrievalPort` 实现逐条一致（顺序/RRF/MMR/降级），标签同源 |
| `unified_retrieval_keeps_acl_first_and_one_algorithm_profile`（core guard，更新） | daemon 经 `UnifiedRetrievalPort`、CLI 调 `rank_retrieval`，且两适配器不再内联旧 RRF/MMR 公式 |

CI 接线（既有分片，无新 test target）：`scripts/ci/test-shards.json` 的 `kiana-domain-s1/4` 认领
`cm12_unified_retrieval`，`kiana-core-s2/6` 认领 `cm12_unified_retrieval_guard`，`kiana-core-s1/6` 认领
`cap19_memory_scope_commit_guard`；`kiana-daemon`、`kiana-query`、`kiana-commands` 为整 crate 分片，
自动涵盖本次改动单测与夹具。本地未运行（CI-only），回执未等待；proof_level 维持 `source`。
