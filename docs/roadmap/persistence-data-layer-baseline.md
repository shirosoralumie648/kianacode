# PD-00 Persistence / Data Layer source baseline

> 快照日期：2026-09-16。本文是 `PD-00` 的 source-only reconciliation，不是统一 StorageRoot、
> durable ProjectionStore、ArtifactStore、Backup/Migration/Retention service 或跨进程数据层的完成声明。
> 本轮不在本地运行测试；`persistence_baseline` 只由 GitHub Actions 执行。
> 2026-10-16 重新钉扎：WIP merge 后按当前工作树更新第 2 节 SHA-256 与 source snapshot 说明；
> `.github/workflows/pd00-baseline.yml` 已并入统一 CI（`scripts/ci/test-shards.json` 调度），
> 该行保留为历史记录。重新钉扎只更新快照记录，不放宽任何断言。

## 1. 范围、事实源和证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`PD-00`](../roadmap.md#step-pd-00) |
| source snapshot | `5226b97`（EQ-00 已推送的干净基线；2026-10-16 按 WIP merge 后工作树重新钉扎第 2 节哈希） |
| proof ceiling | `source`；source guard/test-target 编译不提升 `local_behavior`、`durable`、`live` 或 `physical` |
| single fact source | `EventStore/RuntimeEvent` 与 atomic `TransitionBatch`/`CommandReceipt`；Projection、Receipt、Memory、Index、Cache、Notification 和 UI 都只能派生 |
| this step does | inventory、owner/scope/format/cursor 能力、Memory/JSONL/Approval/Artifact/Index 边界、迁移/备份/保留缺口、PD-01..35 handoff |
| this step does not | 不新增 StorageRoot/StoreIdentity、ProjectionStorePort、ArtifactStorePort、Backup/Migration/Retention service、第二 EventLog 或第二执行循环 |

## 2. Source hashes

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Journal contracts/limits | `kiana-domain/src/journal.rs` | `6966da5534b7bb9155b3282e5338aa5195f362aa6a08ab6564a7a8ddfe24324e` |
| Governance/retention contracts | `kiana-domain/src/governance.rs` | `e7a0c59ec507bf435c0da4cd8efb0e3cf000fdd857712a629b2c4560796f5333` |
| EventStore port | `kiana-ports/src/lib.rs` | `313d3d95987d9343fa0c821faa162645c4cb7221acb541970a75ebfae8797dec` |
| EventLog facade | `kiana-eventlog/src/lib.rs` | `fe98b8bffdf86cc49ce6630bdf3240b8d92b9590024ce3b2aea7edb132b090f4` |
| JSONL adapter | `kiana-eventlog/src/jsonl.rs` | `614ef68ba99f8df06dd56c90ca3864956ec1619bef2623bdfba111f7b4315efc` |
| Memory adapter | `kiana-eventlog/src/memory.rs` | `2af7733b0ba0333c9e13dcd71d63224d0d0fe2a3a72943adff60463738c05774` |
| ApprovalStore | `kiana-daemon/src/approval_store.rs` | `c6267bb007b5ede4663f5ae36c16797f9429ca3a167121024cf01ef5bc84230a` |
| Memory adapter | `kiana-daemon/src/harness_memory.rs` | `278b50288a6cf6e1e65b94ebff7171eea2508f0d077828ae10a556905d9d7ee0` |
| Artifact boundary | `kiana-core/src/artifacts.rs` | `01938b6dd8e0d396482a3b15cfd22c61d2921916a292d51b2295b94d5658ef66` |
| Receipt/recovery projection | `kiana-core/src/receipts.rs`, `kiana-core/src/recovery.rs` | `b5ebf37858e212616523b9d771489c60d1bb087cac74604b2330caad0122fd67`, `5169aecfa239d023af97ea5ddccf0019204dbfdf01d5a14e8e3c3972c9ec1721` |
| Query index/cache | `kiana-query/src/index.rs`, `kiana-query/src/repo_map.rs` | `0515f9a2cce54f3d99edac018f3dc079bfe6d907e0523a365cd42cd44647d56c`, `25e3fe3130ca30ab5e73567c4f0903a24210af1f0a774b1bfb597ff59260fa14` |
| PD-00 source guard/workflow | `kiana-core/tests/persistence_baseline.rs`（2026-10-16 重新钉扎），`.github/workflows/pd00-baseline.yml`（历史记录，已并入统一 CI 调度） | `d1ad17b57bd900e0d508a68795fb21647f05764cb194560201e593e1bdd303ae`, `d696e54207d89ad10e72f88047e5428f5d3d9a3922a06d1d0692b8231ecc3b46`（历史） |

Pre-existing rows are anchors; later PD steps touching these files refresh the hash in the same commit.

## 3. Current data-layer inventory

| 数据对象/层 | 当前 owner 与形态 | 当前读取/失败边界 | proof / gap |
|---|---|---|---|
| Runtime/Company facts | `EventStorePort`；Memory/JSONL `RuntimeEvent`，ControlPlane `append_event`/`commit_transition` | `read_all`/stream/request/cursor；unknown commit、冲突、坏 frame 不得当空 | `source`；没有统一 StorageRoot/StoreIdentity/owner namespace |
| CommandReceipt | atomic transition frame 的 command id/digest/cursor/version | `read_command` 确认原 receipt；Replayed 只能使用原 receipt | `source`/局部 durable；无统一 ReceiptStore/checkpoint |
| JSONL facts | `JsonlEventLog` v2 header/frame、CAS、idempotency、flock、fsync、torn-tail repair、容量 512 MiB/1M events | middle corruption/unknown required record/legacy-after-upgrade/file replacement fail-closed；最后半行有条件修复 | `source`；单机 adapter 行为，尚无 cross-process/power-loss/backup proof |
| Memory facts | `MemoryEventLog` 同一事务/CAS/dedup 规则，capabilities explicitly `durable_commits=false` | 进程内 read/stream/cursor；重启丢失 | `source`；不能把 Memory 标成 durable |
| Approval facts | `JournalApprovalStore`/`MemoryApprovalStore` 查询、TTL、proof、single consume、JSONL journal 局部持久化 | owner/session/hash/nonce/expiry/integrity mismatch 拒绝；PendingInvocation/Runner continuation 仍 process-local | `source`；审批事实尚未完全收敛单一 EventStore，缺统一 projection/root/backup |
| Artifact bytes | `kiana-core::artifacts` 受限路径、descriptor/identity/hash、atomic temporary/rename 原语 | symlink/hardlink/path/identity/version/TOCTOU checks；hash/rename/fsync 失败不声称保存 | `source`；无统一 ArtifactStore manifest/reference count/retention/backup |
| Memory records | daemon 分层 JSONL、scope/grant、candidate/draft、redaction/review/revoke | candidate 默认不检索；scope/ACL/revocation 先于 ranking | `source`/process-local；mutation event、generation、backup、retention、cross-process rebuild 未收口 |
| Context/Repo index | `kiana-query` index/repo map、artifact manifest、generation/content hash、可选 vector/lexical cache | index 是重建加速层；stale/corrupt 不可损坏 facts，scope 仍由 server 派生 | `source`；无 ProjectionStore、generation checkpoint、owner/data epoch/backup contract |
| Run/Company/Observability projections | core projections from EventLog；Receipt/Audit/Health/Metric/Trace/Company snapshots | source cursor/version/epoch/limitations；read failure ≠ empty | `source`；无统一 projector registry/checkpoint store |
| Backup/migration/retention | 文档设计已定义 namespaces/manifest/epoch/watermark/restore，但当前未发现统一 runtime adapter | unknown major/owner/epoch/hash mismatch 应拒绝；暂无操作命令 | `target`；不能用 JSONL reopen、CI、cache 或文件存在宣称备份/迁移/保留完成 |

## 4. Logical layout (design, not current path claim)

未来 StorageRoot 的逻辑 namespace 规划为：

```text
meta/ facts/ projections/ artifacts/ memory/ indexes/ checkpoints/
backups/ migrations/ quarantine/ locks/
```

这是 PD 设计目标，不是当前目录事实。每个 namespace 需要 `store_id`、format/schema epoch、owner
scope、source/data cursor、generation、authority/data epoch、capabilities 和 limits；路径必须通过
现有 `KIANA_HOME`/project/trust resolver，禁止在入口硬编码 `~/.kiana` 或把 project-relative cache
当作事实根。

## 5. Migration and safety guard

1. 事实只能有一个 writer/authority：先协商 adapter capabilities，再在同一 EventStore/transition
   合同内提交；SQLite/WAL、index、Memory、Approval 或 cache 不得与 JSONL 双写为第二账本。
2. 版本拆成 schema/store-format/projection/source-cursor/config/authority/data/generation；未知 major、
   owner mismatch、epoch rollback、checksum drift、torn middle、无 backup 的 destructive migration
   和 downgrade write 全部 fail-closed。
3. Migration 先 read-only preflight（source cursor、artifact/WAL/lock、limits、dependency/retention），
   只有显式 approved operation 才能写 `MigrationRecord`；旧记录不能静默 upcast 成新事实。
4. Projection/index/cache/Memory/Notification 都可从 source 重建；projector 先验证 schema/scope/epoch，
   后原子更新 read model，再推进 cursor。读取失败、lag、Unknown、revocation 或 hash mismatch 必须
   visible limitation/quarantine，不能回退成 empty/healthy。
5. Backup 必须绑定 source/projection cursors、generations、epochs、file/chunk/artifact hashes、config
   revision 和 key ref；JSONL 与 WAL sidecars 一致保存。Restore 默认 paused，重新做 trust/authority/
   policy/data/lease 检查，不能自动 dispatch。
6. Retention/deletion 只追加 watermark/tombstone/data epoch；不能只删 UI/cache，也不能删除仍被
   Receipt/Audit/Incident/Delivery 依赖的 metadata。审计 metadata 与 payload bytes/reference 分开保留。

## 6. Fixture catalog and handoff

| Fixture | Purpose | Owner step |
|---|---|---|
| `persistence_baseline` | source-only fact/projection/cache/adapter boundary guard | PD-00 |
| `storage_root_owner_scope_rejected` | root/namespace/owner/project trust mismatch | PD-01 |
| `unknown_store_schema_fails_closed` | schema/store-format/upcaster unknown major | PD-02 |
| `adapter_capability_mismatch_is_not_empty` | unsupported/durable/cursor/flush distinction | PD-03/05 |
| `eventstore_conformance_memory_jsonl` | CAS/dedup/cursor/order common contract | PD-05/06 |
| `jsonl_reopen_and_corrupt_middle_quarantine` | frame/checksum/torn-tail/identity recovery | PD-07/08 |
| `projection_cursor_atomic_with_state` | lag/gap/checkpoint/rebuild | PD-09 |
| `artifact_hash_reference_retention` | immutable bytes/manifest/TOCTOU/data epoch | PD-14..18/24 |
| `memory_mutation_rebuild_and_revoke` | candidate/event/generation/index propagation | PD-19..21/25 |
| `backup_restore_requires_reauthorization` | cursor/epoch/hash/restore pause | PD-22/23 |
| `migration_checksum_and_downgrade_read_only` | ordered migration/upcast/rollback | PD-26..29 |
| `retention_tombstone_preserves_audit_metadata` | payload vs audit retention/deletion | PD-30..33 |
| `persistence_cross_adapter_uat` | CLI/Web/Workbench shared StorageRoot/Receipt | PD-34/35 |

PD-00 closes only the source inventory and migration guard. It does not claim unified durable data-layer
behavior; all behavior and adapter conformance fixtures run in GitHub Actions only.
