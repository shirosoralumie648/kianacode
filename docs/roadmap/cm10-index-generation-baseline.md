# CM-10 ContextIndex generation 与原子切换基线

> 快照日期：2026-09-20。本页记录 shared generation/manifest source slice；夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CM-10`](context-memory.md#step-cm-10) |
| feature_status | `implemented`（domain generation state + query atomic manifest publisher + CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| canonical path | source manifest digest → one building generation containing repo-map/exact/BM25/dense component digests → validate/commit → synced atomic Ready manifest → readers use one generation |
| authority | generation manifest is a derived index publication boundary; EventLog/ControlPlane remain authority and component files cannot authorize actions |

## Contract and behavior

`IndexGenerationState` admits one active build, binds all four component digests to one source
manifest, publishes only a `Ready` manifest and retains the last Ready generation when a build
fails. A reader cannot obtain components individually from the state. `write_index_manifest_atomic`
serializes a validated Ready manifest to a create-new temporary file, syncs it, renames it into
place and syncs the parent directory where supported; readers validate schema/status/digests again.

The existing broad query index builders remain compatibility adapters; this step supplies the
shared generation contract and atomic publication boundary for their migration, without creating
a second retrieval or execution authority.

## CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `readers_never_mix_index_generations` | new Building source cannot replace the prior Ready generation before commit |
| `failed_rebuild_keeps_last_ready_generation` | failed rebuild records reason while reader stays on old Ready manifest |
| `incomplete_component_manifest_is_rejected` | repo-map/exact/BM25/dense component set is all-or-nothing |
| `ready_manifest_publishes_by_temp_sync_and_atomic_rename` | query publisher writes and rereads one validated Ready manifest |
| `building_manifest_cannot_be_published_as_ready` | incomplete build cannot become visible |
| `index_generation_has_one_manifest_and_atomic_publication_boundary` | source guard keeps one manifest, sync/rename and no-second-loop markers |

## Proof ceiling and handoff

CM-10 proof ceiling is `source` plus remote CI wiring. Full index builder migration, crash/power
loss recovery, cross-process locks and rename/delete invalidation remain CM-11/PD work; no durable
index availability or production retrieval quality claim is made.
