# PD-00 Persistence / Data Layer source baseline

> 快照日期：2026-09-16。本文是 `PD-00` 的 source-only reconciliation，不是统一 StorageRoot、
> durable ProjectionStore、ArtifactStore、Backup/Migration/Retention service 或跨进程数据层的完成声明。
> 本轮不在本地运行测试；`persistence_baseline` 只由 GitHub Actions 执行。

## 1. 范围、事实源和证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`PD-00`](../roadmap.md#step-pd-00) |
| source snapshot | `5226b97`（EQ-00 已推送的干净基线） |
| proof ceiling | `source`；source guard/test-target 编译不提升 `local_behavior`、`durable`、`live` 或 `physical` |
| single fact source | `EventStore/RuntimeEvent` 与 atomic `TransitionBatch`/`CommandReceipt`；Projection、Receipt、Memory、Index、Cache、Notification 和 UI 都只能派生 |
| this step does | inventory、owner/scope/format/cursor 能力、Memory/JSONL/Approval/Artifact/Index 边界、迁移/备份/保留缺口、PD-01..35 handoff |
| this step does not | 不新增 StorageRoot/StoreIdentity、ProjectionStorePort、ArtifactStorePort、Backup/Migration/Retention service、第二 EventLog 或第二执行循环 |

## 2. Source hashes

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Journal contracts/limits | `kiana-domain/src/journal.rs` | `4dbd656d03ec12e7821ffac254307227419423cbaf74ccb99a2b819c5eeedd48` |
| Governance/retention contracts | `kiana-domain/src/governance.rs` | `8d64488c287fe7fe8e2a67f9db2cafa685c50d0012600a6ac23fe4d372e1a777` |
| EventStore port | `kiana-ports/src/lib.rs` | `a539b96c0813c0f08c043dd89b4f2bcbe9fdb4d6d9f93923443821b54679e6d0` |
| EventLog facade | `kiana-eventlog/src/lib.rs` | `7af0aba67a44e4575ffd7920ab07866a28c4dbbfcf0c7a3a96221986dfc190bb` |
| JSONL adapter | `kiana-eventlog/src/jsonl.rs` | `f549e5de5bc4e197f268b865327bd1d0d7a4c10208daa3e7c13ceb68188e5c5f` |
| Memory adapter | `kiana-eventlog/src/memory.rs` | `bcb98daa0a9548384a9dafdd9d5af2aefacca3d01d699056ba86477d3276e9e9` |
| ApprovalStore | `kiana-daemon/src/approval_store.rs` | `e013b20c983f4555d7256980ed96c3785136c2fc148568a3a9a7ee34e6695929` |
| Memory adapter | `kiana-daemon/src/harness_memory.rs` | `6379bfb054a745f07f9977cf63122107f97e3753b5e15bcfc90217d8518b953c` |
| Artifact boundary | `kiana-core/src/artifacts.rs` | `dbc0776cdfebff5d10ff65238cf09ca3c41fa4ed350dc769b3f6bc4eaceaf1d0` |
| Receipt/recovery projection | `kiana-core/src/receipts.rs`, `kiana-core/src/recovery.rs` | `dda33c346b1ffd94389093e2856f99433ea083a3c61b35cf562484f9f4bc7e1e`, `f964c69bb32e940782ef52b399afd9ad6902778b4900f6b7164cb79f3dcc7d26` |
| Query index/cache | `kiana-query/src/index.rs`, `kiana-query/src/repo_map.rs` | `a9cfc4767f24a7f790686be8068554aac7ee84c6c8d58ee6c71e78b75afc3716`, `ead92a58e5de2b391779666007b4972bc79ef0e16226e8937338310ae9e39100` |
| PD-00 source guard/workflow | `kiana-core/tests/persistence_baseline.rs`, `.github/workflows/pd00-baseline.yml` | `d050e42f09da86596997450d854aa4684f9cd0b1f18c348b9ec2934188ab03d6`, `d696e54207d89ad10e72f88047e5428f5d3d9a3922a06d1d0692b8231ecc3b46` |

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
