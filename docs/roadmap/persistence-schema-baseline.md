# PD-02 storage schema and canonical upcast baseline

> 快照日期：2026-09-17。本页记录 schema registry/canonical/upcast 边界；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`PD-02`](../roadmap.md#step-pd-02) |
| feature_status | `implemented`（registry uniqueness, canonical bytes/digest and named upcast） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | domain `SCHEMA_CONTRACTS` → `StorageSchemaRegistry` → canonical bytes → named migration/upcast → destination validation |
| this step does | duplicate schema/owner/unknown-field registry checks, conservative JSON number/depth policy, stable bytes/digest and `memory-record.v1→v2` explicit upcast |
| this step does not | 不迁移所有 legacy payload、实现 migration runner/backup/SQLite、改变 EventLog write path、自动丢弃 unknown facts 或新增执行循环；PD-03+ / ER 负责 |

## 2. Registry/upcast rules

`StorageSchemaRegistry::current/validate` 以 static `SCHEMA_CONTRACTS` 生成 digest，拒绝 duplicate names、zero major 和 domain allow-unknown。`canonical_storage_bytes` 递归限制 depth、NUL key、非 finite/`-0`/noncanonical exponent number，并复用 canonical journal key ordering；`canonical_storage_digest` 对稳定 bytes 计算 `sha256:`。

`upcast_storage_value` 必须先确认 expected schema 已登记；exact schema 原样返回，未知 major、unknown expected schema、无命名 migration 或 non-migratable fields fail-closed。唯一实现的 migration 是 `MemoryRecord::legacy_import`：v1 row 只在 allow-listed fields 下转为 v2 candidate/draft/unknown provenance，并再次执行 destination lifecycle validation；upcast 不执行文件/网络/provider，也不成为 authority。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `storage_schema_registry_is_unique_and_canonical` | registry unique/current digest、object-key canonical bytes/digest、number/unknown-field reject |
| `storage_upcaster_requires_named_migration_and_rejects_unknown_major_or_field` | memory v1→v2 named upcast、unknown major/field/expected schema reject |
| `storage_schema_registry_is_canonical_and_upcast_is_explicit` | source guard 锁定 no Tokio/Broker/implicit migration |

`.github/workflows/pd02-schema.yml` 在 GitHub runner 执行 domain schema/upcast fixtures、core source guard、fmt 和 domain/protocol/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- Registry 仍是 process static snapshot，不是 durable migration journal；schema catalog 变化需后续 migration/rollback 证据。
- Canonical number policy 保守但不等同于所有语言的 RFC canonical JSON；大型/非 JSON provider bytes、SQLite encoding、fsync/CAS 和 cross-process replay 留待 PD-05+ / ER / DEP。
- Legacy memory migration 明确降为不可搜索 candidate/draft；其他历史 payload 不会被猜测转换，必须有 named upcaster 或隔离 quarantine。
