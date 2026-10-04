# PD-01 storage root, identity and lock baseline

> 快照日期：2026-09-17。本页记录 StorageRoot/StoreIdentity/owner scope/namespace/lock 合同；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`PD-01`](../roadmap.md#step-pd-01) |
| feature_status | `partial`（domain storage contracts + daemon resolver/lease source; product entrypoint/restart identity acceptance remains incomplete） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | DaemonHost → `resolve_storage_root` → StorageRoot/owner scope → StoreIdentity → StorageLease |
| this step does | stable root ID, absolute root validation, owner/instance/authority epoch scope, fixed logical namespaces, persisted store identity and create-new storage lock |
| this step does not | 不迁移所有 legacy `KIANA_HOME` adapter、实现 projection/artifact/backup/migration/retention stores、网络 FS backend、数据库或第二执行循环；PD-02+ 负责 |

## 2. Root and lock rules

Domain `StorageRoot` 只接受 absolute lexical path（无 `..`/prefix）、LocalFilesystem backend、完整 namespace map（meta/facts/projections/artifacts/memory/indexes/checkpoints/backups/migrations/quarantine/locks）和 owner scope digest；root ID 按 canonical path 稳定派生，重启解析同一路径不会漂移。`StorageOwnerScope` 绑定 owner、instance、optional project、authority epoch；scope/ID/digest/unknown fields fail-closed。

Daemon resolver 优先使用 `KIANA_HOME`，否则 `$HOME/.kiana`，canonicalize 已存在 root/parent，拒绝相对 root、project 内 root、mount probe 识别的 nfs/nfs4/cifs/smbfs/sshfs。`StorageLease` 创建 namespace dirs，`meta/store-identity.json` 使用 create-new/0600/sync 持久化 StoreIdentity；随后 `locks/storage.lock` create-new/0600 写入 strict StorageLockRecord，已存在锁或 owner/instance mismatch 返回不同 Conflict，Drop/release 删除当前锁文件。

DaemonHost 暴露 `storage_root`/`acquire_storage` 生命周期入口，但 CLI/Web/Workbench 尚未通过它们共用同一稳定 root；这些入口本身不接受模型工具或绕过 ControlPlane。现有 EventLog/approval/memory legacy adapter 也仍各自读取 `KIANA_HOME`，迁移留在后续 PD/ER。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `storage_root_namespaces_and_identity_are_stable_and_strict` | root ID/path namespace/identity/lock digest 稳定，unknown field 拒绝 |
| `storage_root_rejects_relative_network_and_owner_mismatch` | relative/network backend、owner/instance identity mismatch 拒绝 |
| `storage_identity_and_lock_reject_zero_time_and_nested_scope_drift` | zero identity/lock timestamps and lock scope drift fail closed before a record can be accepted |
| `daemon_storage_resolver_and_lock_reject_scope_conflicts` | resolver root、identity persistence、single-writer lock conflict 与 owner mismatch |
| `daemon_storage_resolver_rejects_project_local_root` | 项目内 `.kiana` root fail-closed |
| `daemon_storage_lock_initialization_failure_releases_owned_file` | Linux 子进程限制 lock file 写入；初始化失败后 owned lock path 被清理，恢复写入限制后可重新获取 |
| `storage_root_is_resolved_once_and_lock_adapter_stays_outside_control_plane` | domain/daemon/host source guard 无 ControlPlane/Broker second path |

当前统一 `.github/workflows/ci.yml` 在 GitHub runner 执行 `pd01_storage` domain target (`kiana-domain-s4/4`)、`pd01_storage_guard` core target (`kiana-core-s5/6`) 和 daemon whole-crate fixtures；历史独立 `.github/workflows/pd01-storage-root.yml` 已合并删除。本地不运行测试。

## 4. 限制与交接

- 本地 resolver/lease 是 daemon adapter，create-new lock 不是跨机器 lease/fencing；stale process/crash lock、fsync-dir、network mount race 和 full recovery 仍需 PD-05/06/ER/DEP/SC。
- StoreIdentity 当前 format/schema epoch 从 1 创建，尚无 migration runner、backup/restore、owner/project durable upcast；authority epoch 改变会使 owner scope digest mismatch，按 fail-closed 处理。
- 部分 legacy adapters 仍直接读取 KIANA_HOME；PD-02/04+ 必须逐步迁移并保持 EventLog/ControlPlane 唯一事实与执行边界。
- `StorageOwnerScope` 将 instance ID 与 authority epoch 摘要绑定到 StoreIdentity；它们是否代表跨重启稳定的安装/store generation、以及 authority epoch 改变时的 migration/rebinding 尚待架构决定。在该语义和产品入口 callsites 收口前，不宣称 CLI/Web/Workbench 共享同一 root 或重启复用 identity。
- Failed lock initialization now attempts inode-checked cleanup through the pinned lock directory; this does not prove cleanup across crashes/power loss, and `remove_owned_file` retains its existing check-to-unlink race against a hostile same-UID replacement.

## 5. 2026-10-03 failed lock initialization cleanup

`StorageLease::acquire` now treats lock-record construction, canonical serialization, `write_all`,
and `sync_all` as one initialization phase. If any fails after the create-new lock succeeds, it
passes the held file descriptor to `LocalDir::remove_owned_file`; that helper checks the pinned
directory entry's type/link count and device/inode before unlinking. Cleanup failure is returned
alongside the original initialization error so the caller does not mistake a stranded lock for a
normal conflict.

The Linux-only `daemon_storage_lock_initialization_failure_releases_owned_file` fixture runs in a
child test process, uses `RLIMIT_FSIZE=0` to force lock record write failure, confirms the leaf was
removed, restores the limit, and reacquires. It is included by the existing `kiana-daemon` crate
shard; this source snapshot has no GitHub receipt yet. This does not establish power-loss cleanup,
cross-machine fencing, or eliminate the existing check-to-unlink race against a same-UID replacer.
PD-01 remains `partial/source` until the shared product root and restart identity contract are
resolved.

## 6. 2026-10-04 malformed lock classification

The daemon lock-conflict reader now separates malformed or structurally invalid lock data from a
valid record owned by another scope. JSON decode failure, an unknown lock schema, a nil lock id,
or failed timestamp/digest validation returns `storage_lock_corrupt`, which maps to the storage
corrupt class; a valid record with a different owner or instance remains the distinct
`storage_lock_owner_mismatch` conflict. A focused daemon fixture writes `{}` after a clean lease
release and asserts the corrupt classification, while the Core guard pins the source marker. No
recovery or execution path is added.

```text
source_snapshot: parent `d73aca7c` plus current PD-01 slice; kiana-daemon/src/storage.rs; kiana-daemon/tests/pd01_storage.rs; kiana-core/tests/pd01_storage_guard.rs
worktree_status: malformed/unknown-schema/nil-id/timestamp/digest-invalid lock records fail closed as storage_lock_corrupt; valid scope mismatch remains storage_lock_owner_mismatch; no manifest, lockfile or ControlPlane path changed
command_argv: git diff --check; staged diff check; no local test/build/check/clippy/smoke; GitHub Actions `pd01-storage-root.yml` after push
cwd·environment: repository root Linux/bash; GitHub Actions is the runtime test authority
fixture·cassette: `daemon_storage_lock_conflict_rejects_a_malformed_record_distinctly` plus existing scope/FIFO/identity/initialization fixtures and Core storage source guard
exit_code: format/diff checks 0; changed-source CI pending; no local runtime exit code
status_change: PD-01 remains roadmap row 108 `🔄`; feature_status=partial; proof_level=source
proof-level_change: none; no local_behavior, durable, live or physical promotion
limitations: malformed metadata classification does not prove stale-lock recovery, power-loss cleanup, cross-process fencing, shared CLI/Web/Workbench root reuse or restart identity semantics
reviewer: source review of lock conflict parsing/classification and deny-first fixture; no local runtime reviewer
```
