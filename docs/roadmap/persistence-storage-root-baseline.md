# PD-01 storage root, identity and lock baseline

> 快照日期：2026-09-17。本页记录 StorageRoot/StoreIdentity/owner scope/namespace/lock 合同；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`PD-01`](../roadmap.md#step-pd-01) |
| feature_status | `implemented`（domain storage contracts + daemon resolver/lease） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | DaemonHost → `resolve_storage_root` → StorageRoot/owner scope → StoreIdentity → StorageLease |
| this step does | stable root ID, absolute root validation, owner/instance/authority epoch scope, fixed logical namespaces, persisted store identity and create-new storage lock |
| this step does not | 不迁移所有 legacy `KIANA_HOME` adapter、实现 projection/artifact/backup/migration/retention stores、网络 FS backend、数据库或第二执行循环；PD-02+ 负责 |

## 2. Root and lock rules

Domain `StorageRoot` 只接受 absolute lexical path（无 `..`/prefix）、LocalFilesystem backend、完整 namespace map（meta/facts/projections/artifacts/memory/indexes/checkpoints/backups/migrations/quarantine/locks）和 owner scope digest；root ID 按 canonical path 稳定派生，重启解析同一路径不会漂移。`StorageOwnerScope` 绑定 owner、instance、optional project、authority epoch；scope/ID/digest/unknown fields fail-closed。

Daemon resolver 优先使用 `KIANA_HOME`，否则 `$HOME/.kiana`，canonicalize 已存在 root/parent，拒绝相对 root、project 内 root、mount probe 识别的 nfs/nfs4/cifs/smbfs/sshfs。`StorageLease` 创建 namespace dirs，`meta/store-identity.json` 使用 create-new/0600/sync 持久化 StoreIdentity；随后 `locks/storage.lock` create-new/0600 写入 strict StorageLockRecord，已存在锁或 owner/instance mismatch 返回不同 Conflict，Drop/release 删除当前锁文件。

所有 surface 通过 DaemonHost 的 `storage_root`/`acquire_storage` 入口获得同一解析器；这些入口只处理存储生命周期，不接受模型工具或绕过 ControlPlane。现有 EventLog/approval/memory 等 legacy adapter 仍各自读取 `KIANA_HOME`，迁移留在后续 PD/ER。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `storage_root_namespaces_and_identity_are_stable_and_strict` | root ID/path namespace/identity/lock digest 稳定，unknown field 拒绝 |
| `storage_root_rejects_relative_network_and_owner_mismatch` | relative/network backend、owner/instance identity mismatch 拒绝 |
| `daemon_storage_resolver_and_lock_reject_scope_conflicts` | resolver root、identity persistence、single-writer lock conflict 与 owner mismatch |
| `daemon_storage_resolver_rejects_project_local_root` | 项目内 `.kiana` root fail-closed |
| `storage_root_is_resolved_once_and_lock_adapter_stays_outside_control_plane` | domain/daemon/host source guard 无 ControlPlane/Broker second path |

`.github/workflows/pd01-storage-root.yml` 在 GitHub runner 执行 domain/daemon storage fixtures、core source guard、fmt 和 domain/daemon/protocol/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 本地 resolver/lease 是 daemon adapter，create-new lock 不是跨机器 lease/fencing；stale process/crash lock、fsync-dir、network mount race 和 full recovery 仍需 PD-05/06/ER/DEP/SC。
- StoreIdentity 当前 format/schema epoch 从 1 创建，尚无 migration runner、backup/restore、owner/project durable upcast；authority epoch 改变会使 owner scope digest mismatch，按 fail-closed 处理。
- 部分 legacy adapters 仍直接读取 KIANA_HOME；PD-02/04+ 必须逐步迁移并保持 EventLog/ControlPlane 唯一事实与执行边界。
