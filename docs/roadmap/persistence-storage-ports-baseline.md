# PD-04 storage lifecycle ports baseline

> 快照日期：2026-09-17。本页记录 projection/artifact/backup/migration/retention port 分层；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`PD-04`](../roadmap.md#step-pd-04) |
| feature_status | `implemented`（typed storage lifecycle ports with fail-closed defaults） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | domain Storage/Health contracts → `kiana-ports` lifecycle traits → future DaemonHost adapters |
| this step does | `ProjectionStorePort` cursor/checkpoint/read, `ArtifactStorePort` stage/commit/read/verify, `BackupStorePort` snapshot/verify/restore, `MigrationRunnerPort` preflight/apply and `RetentionStorePort` plan/tombstone/purge |
| this step does not | 不实现具体 store/SQLite/JSONL adapter、backup bytes、migration journal、retention policy、health projector 或第二执行循环；PD-05+ / ER/DEP 负责 |

## 2. Port rules

Projection methods require a named projection, committed source cursor and expected checkpoint revision; a missing capability must be `projection_store_unsupported`, never an empty success. Artifact methods carry typed `ArtifactVersion`/`ArtifactRef`, explicit expected revision and verify boundary; they cannot implicitly replace bytes. Backup and restore use opaque manifests plus store/cursor pins; restore remains an explicit authorized operation.

Migration preflight/apply receives the current `StorageSchemaRegistry`/registry digest and format versions, so a port cannot invent or skip an upcaster. Retention exposes planning, append-only tombstone and expected tombstone revision before purge; deletion is not inferred from an empty query. Traits only depend on domain objects, IDs, EventCursor and JSON, and default implementations return structured `PortError::Unavailable`.

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `storage_ports_are_typed_and_default_to_explicit_unsupported` | compile-only fake implements all five ports/Clock; defaults are explicit unsupported and no daemon/provider/path/network type |
| `storage_lifecycle_ports_have_no_implicit_fallback_or_execution_authority` | source guard locks cursor/revision/tombstone boundaries and no tokio/Broker loop |

`.github/workflows/pd04-storage-ports.yml` 在 GitHub runner 执行 ports fixture、core source guard、fmt 和 domain/ports/protocol/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- Traits alone do not prove any durable adapter, CAS, fsync, owner/scope/epoch verification, cross-process lock, crash recovery or health/read parity.
- `ArtifactStorePort`/`BackupStorePort` intentionally expose bytes/manifests for later controlled adapters; concrete implementations must enforce content hash, redaction/privacy and root scope before storage or restore.
- PD-05/06/07+ will add EventStore conformance, JSONL frame/lock/recovery, index/projector/cursor, migration/backup/retention implementations; no caller may treat default Unsupported as empty or success.
