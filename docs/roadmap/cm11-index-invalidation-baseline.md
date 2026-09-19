# CM-11 增量更新、rename/delete 与缓存失效基线

> 快照日期：2026-09-20。本页记录 source-snapshot invalidation source slice；夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CM-11`](context-memory.md#step-cm-11) |
| feature_status | `implemented`（domain invalidation/cache-key contract + query adapter + CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| canonical path | previous/current WorkspaceSnapshot → content/identity/disposition diff → Added/Changed/Removed/Renamed plan → invalidated paths/tombstones → next IndexGeneration |
| authority | invalidation is derived index evidence; EventLog/ControlPlane and IndexGeneration remain authoritative boundaries |

## Contract and behavior

`IndexInvalidationPlan` compares effective content evidence and read disposition, not mtime alone.
It detects deterministic add/change/remove/rename rows, emits tombstones for removed/renamed old
paths and binds source/target snapshot digests and generation order. `IndexCacheKey` binds root,
worktree, branch, dirty manifest, parser, chunker and config digests. The query adapter only
delegates to these pure contracts; it does not mutate a cache or index itself.

## CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `changed_file_invalidates_only_affected_chunks` | one content change invalidates only its path |
| `rename_and_delete_emit_tombstones_and_do_not_use_mtime_alone` | same content under a new path is rename evidence and mtime-only drift is ignored |
| `cache_key_binds_all_source_and_algorithm_inputs` | branch/source/parser/chunker/config inputs change the cache key |
| `query_adapter_returns_typed_invalidation_plan` | query surface returns the same typed rename/tombstone plan |
| `invalidation_binds_identity_content_cache_inputs_and_tombstones` | source guard preserves identity/content/cache/tombstone boundary |

## Proof ceiling and handoff

CM-11 proof ceiling is `source` plus remote CI wiring. Existing persistent builders still need full
integration with this plan and CM-10 publication, durable tombstone/retention propagation and
cross-process recovery remain PD/ER work; no production index freshness claim is made.
