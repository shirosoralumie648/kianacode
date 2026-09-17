# PD-03 storage errors, health and integrity baseline

> 快照日期：2026-09-17。本页记录 storage error/health/capability contracts；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`PD-03`](../roadmap.md#step-pd-03) |
| feature_status | `implemented`（typed storage taxonomy, health/capability/incident DTOs and port mapping） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | PortError → StorageErrorClass/StorageError → StorageHealth/IntegrityIncident projection |
| this step does | distinct Empty/Unavailable/Corrupt/Conflict/Unknown/ResultUnknown classes, retry/reconcile disposition, capability contract, health snapshot and quarantine incident |
| this step does not | 不改所有 adapter 错误调用点、实现 health probe/projector、迁移/backup/retention、跨进程 recovery 或第二执行循环；PD-04+ / ER 负责 |

## 2. Error and health rules

`StorageErrorClass` 保留六种语义，`StorageRetryDisposition` 由 class 服务器派生：Unavailable/Conflict 可在重新读取后重试，Corrupt/Unknown/ResultUnknown 必须 Reconcile，Empty 明确是空状态而非 Unknown。`PortError::storage_class` 对已知关键词做保守映射，未分类 Failed→Unknown；`into_storage_error` 生成 strict ID/digest，不会把失败转成成功。

`StorageCapabilities` 声明 durable/atomic/cursor/receipt/fsync/limits，durable 或 atomic 没有 fsync 直接拒绝。`StorageHealth` 绑定 StoreIdentity、source cursor、authority epoch、capabilities、limitations；`StorageIntegrityIncident` 绑定 store、Corrupt/Unknown/ResultUnknown、source cursor、强制 quarantine 和 resolution 时间。所有 DTO deny_unknown_fields、nil ID/digest/time/状态冲突 fail-closed。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `storage_errors_keep_empty_unavailable_corrupt_conflict_unknown_distinct` | 六类错误/重试策略与 PortError 映射保持区分，Unknown/Corrupt 不会成功 |
| `storage_health_capabilities_and_integrity_incident_are_strict` | capabilities/health/incident digest、fsync durability conflict、unknown field/quarantine guard |
| `port_errors_map_to_stable_storage_classes_without_success_fallback` | ports source mapping 稳定、未知失败保守归类 |
| `storage_health_and_error_taxonomy_are_typed_before_ports_or_surfaces` | domain/protocol/ports source ownership/no execution path |

`.github/workflows/pd03-storage-health.yml` 在 GitHub runner 执行 domain/ports fixtures、core source guard、fmt 和 domain/ports/protocol/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- Error mapping 仍是 PortError 文本边界，尚未全量替换 adapter 自定义错误；未知/损坏事实仍需 EventLog quarantine/recovery，不应自动 retry。
- Health 是 projection，不证明 disk/provider/network/peer reality；capabilities 是声明，不等同于 fsync/kill/reopen 的 durable 证据。
- PD-04 将下沉 projection/artifact/backup/migration/retention ports；PD-05+、ER/DEP/SC 负责 adapter conformance、flush/lock/recovery、错误映射到 CLI/HTTP/Receipt。
