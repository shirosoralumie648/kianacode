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

Daemon resolver 优先使用 `KIANA_HOME`，否则 `$HOME/.kiana`，canonicalize 已存在 root/parent，拒绝相对 root、已存在但不是目录的 root、project 内 root、mount probe 识别的 nfs/nfs4/cifs/smbfs/sshfs。`StorageLease` 创建 namespace dirs，`meta/store-identity.json` 使用 create-new/0600/sync 持久化 StoreIdentity；随后 `locks/storage.lock` create-new/0600 写入 strict StorageLockRecord，已存在锁或 owner/instance mismatch 返回不同 Conflict，Drop/release 删除当前锁文件。

DaemonHost 暴露 `storage_root`/`acquire_storage` 生命周期入口，但 CLI/Web/Workbench 尚未通过它们共用同一稳定 root；这些入口本身不接受模型工具或绕过 ControlPlane。现有 EventLog/approval/memory legacy adapter 也仍各自读取 `KIANA_HOME`，迁移留在后续 PD/ER。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `storage_root_namespaces_and_identity_are_stable_and_strict` | root ID/path namespace/identity/lock digest 稳定，unknown field 拒绝 |
| `storage_root_rejects_relative_network_and_owner_mismatch` | relative/network backend、owner/instance identity mismatch 拒绝 |
| `storage_identity_and_lock_reject_zero_time_and_nested_scope_drift` | zero identity/lock timestamps and lock scope drift fail closed before a record can be accepted |
| `daemon_storage_resolver_and_lock_reject_scope_conflicts` | resolver root、identity persistence、single-writer lock conflict 与 owner mismatch |
| `daemon_storage_resolver_rejects_project_local_root` | 项目内 `.kiana` root fail-closed |
| `daemon_storage_resolver_rejects_existing_non_directory_root` | `KIANA_HOME` 指向已存在普通文件时返回 `storage_root_not_directory`，不构造 StorageRoot |
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

## 7. Storage guard marker ownership correction (2026-10-04)

Focused PD-01 run `37172453421` reached the source guard and failed because the Core guard
searched the daemon-only `storage_lock_corrupt` marker in the domain source. The guard now checks
domain-owned schema/validation markers against `kiana-domain/src/storage.rs` and daemon-owned
conflict classification markers against `kiana-daemon/src/storage.rs`. The production lock
classification is unchanged.

```text
source_snapshot: failing head `890d9585`; correction `8d93200c`; kiana-core/tests/pd01_storage_guard.rs
worktree_status: source guard marker ownership matches the implementation boundary; no production storage, ControlPlane or execution path changed
command_argv: remote `gh run view 37172453421 --log-failed`; targeted rustfmt; `git diff --check`; push `8d93200c`; `gh workflow run pd01-storage-root.yml --ref master`
cwd·environment: repository root Linux/bash; GitHub Actions is the runtime test authority
fixture·cassette: run `37172453421`, job `storage-root`, `storage_root_is_resolved_once_and_lock_adapter_stays_outside_control_plane` failed only on `storage_lock_corrupt` marker ownership; replacement run `37173030509` pending
exit_code: remote source-guard failure observed; targeted format/diff checks 0; replacement CI pending; no local runtime exit code
status_change: PD-01 roadmap row 108 remains `🔄`; feature_status=partial; proof_level=source
proof-level_change: none; guard correction does not promote runtime proof
limitations: stale lock recovery, power-loss cleanup, cross-process fencing, shared product-entry root resolution and restart identity remain open
reviewer: source review of exact remote guard failure and daemon/domain marker ownership; no local runtime reviewer
```

## 8. Current-head focused acceptance receipt (2026-10-04)

Run `37173137916` at head `aea0d70c` completed successfully. GitHub Actions passed formatting,
the domain storage fixtures, daemon storage fixtures, the ownership-aligned Core source guard,
and all three target-scoped compile steps. This confirms the malformed-lock classification and
its source guard on the current head only; it does not close the broader StorageRoot contract.

```text
source_snapshot: `aea0d70c543995ccf322bff4568d6b5e37338c06`; .github/workflows/pd01-storage-root.yml; PD-01 domain/daemon/Core sources and fixtures
worktree_status: current-head focused workflow success; no shared manifest, lockfile, ControlPlane path or second execution loop changed
command_argv: `gh run view 37173137916 --json headSha,status,conclusion,jobs,url`; no local test/build/check/clippy/smoke
cwd·environment: repository root; GitHub Actions Ubuntu runner is the runtime test authority
fixture·cassette: job `storage-root`: formatting, domain fixtures, daemon fixtures, source guard, and domain/daemon/Core target compiles all success
exit_code: all focused remote steps success; no local runtime exit code
status_change: PD-01 roadmap row 108 remains `🔄`; feature_status=partial; proof_level=source
proof-level_change: focused current-head evidence refreshed; no local_behavior, durable, live or physical promotion
limitations: shared CLI/Web/Workbench root reuse, restart identity, stale-lock recovery, power-loss cleanup and cross-process fencing remain open
reviewer: exact GitHub job/step receipt and source/fixture review; no local runtime reviewer
```

## 9. Current-head focused local receipt (2026-10-05)

The current worktree ran the domain, daemon and Core PD-01 selectors with one Cargo/test worker.
Domain 3/3, daemon 9/9 and the Core source guard 1/1 passed, including the Linux file-size
failure cleanup child fixture. The selected implementation has local behavior evidence, while the
card remains `partial/source` because product-entry root reuse, restart identity, stale/crash and
power-loss recovery, fsync-directory guarantees and cross-process fencing remain unresolved.

```text
source_snapshot: current dirty worktree; kiana-domain/src/storage.rs; kiana-daemon/src/storage.rs; kiana-daemon/src/lib.rs; kiana-core/tests/pd01_storage_guard.rs
worktree_status: PD-01 remains roadmap row 108 `🔄`; no shared CLI/Web/Workbench routing, manifest or lockfile change
command_argv: bounded serial `cargo test -p kiana-domain --test pd01_storage --locked --offline -- --test-threads=1`; `cargo test -p kiana-daemon --test pd01_storage --locked --offline -- --test-threads=1`; `cargo test -p kiana-core --test pd01_storage_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`, 32 MiB test stack, stripped non-incremental profile; 2.39s elapsed, 1.59s user, 0.77s system, max RSS 569548 KiB (~556.2 MiB), swaps 0
fixture·cassette: domain 3/3; daemon 9/9 including RLIMIT_FSIZE lock initialization cleanup; Core guard 1/1; logs `/tmp/kiana-roadmap-20261005-evidence/pd01-focused.log` and resources `/tmp/kiana-roadmap-20261005-evidence/pd01-focused.resources`
exit_code: all three targets exited 0
status_change: selected PD-01 storage/root contracts recorded as `local_behavior`; whole card remains `feature_status=partial`, `proof_level=source`, roadmap row 108 `🔄`
proof-level_change: focused selectors `source` → `local_behavior`; no whole-card promotion
limitations: CLI/Web/Workbench shared root routing, restart identity/authority rebinding, stale/crash/power-loss recovery, fsync-dir, cross-process fencing, runtime network matrix, same-UID check-to-unlink race and legacy adapter migration remain open
reviewer: integration agent with read-only PD-01 audit; focused local storage reviewer
```

## 2026-10-06 stable root-ID binding receipt

`StorageRoot::validate` now recomputes the root ID from the validated absolute
`canonical_path`. A serialized record with a different valid root ID is rejected even when its
root digest has been recomputed to match the altered record. This closes the gap between the
constructor's stable-ID rule and validation of persisted/deserialized roots.

```text
source_snapshot: HEAD c9b8360fd87a5d3809da84cf73b8ba27247b85b6 plus current dirty worktree; kiana-domain/src/storage.rs and kiana-domain/tests/pd01_storage.rs
worktree_status: PD-01 remains roadmap row 108 🔄 / feature_status=partial; no shared product-entry root routing, manifest, lockfile or ControlPlane path change
command_argv: RED `cargo test -p kiana-domain --test pd01_storage storage_root_rejects_id_not_derived_from_path_even_with_recomputed_digest --locked --offline -- --exact --test-threads=1`; GREEN `cargo test -p kiana-domain --test pd01_storage --locked --offline -- --test-threads=1`; downstream `cargo test -p kiana-daemon --test pd01_storage --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; each Cargo invocation used one build job, one test thread, 32 MiB test stack, stripped/non-incremental test profile; combined targeted peak RSS 4,095,400 KiB (~4.0 GiB), each measured process reported 0 swaps; logs/resources under `/tmp/kiana-roadmap-20261006-pd01-{red,green,daemon}.{log,resources}`
fixture·cassette: RED reproduced forged non-nil root ID with recomputed digest returning `Ok(())`; GREEN domain target 4/4 including the new denial case; daemon storage target 9/9 including lock conflict and failed initialization cleanup
exit_code: RED 101 as expected; GREEN domain and daemon targets 0
status_change: selected root identity binding behavior recorded as `local_behavior`; whole PD-01 remains `feature_status=partial`, `proof_level=source`, row 108 🔄
proof-level_change: selected domain and daemon behavior `source` → `local_behavior`; no whole-card durable/live/physical promotion
limitations: Core `pd01_storage_guard` not rerun after this source change; CLI/Web/Workbench shared root, restart identity, migration/upcast, stale/crash/power-loss cleanup, directory fsync and cross-process fencing remain open
reviewer: root integration agent; read-only PD-01 audit; focused domain/daemon test review
```

## 10. Current-head lock integrity precedence receipt (2026-10-07)

The daemon conflict reader previously compared `store_id`, owner and instance scope before
checking the lock record's timestamp and digest. A damaged record that also belonged to a foreign
scope could therefore be reported as `storage_lock_owner_mismatch`. `StorageLockRecord` now exposes
`validate_integrity`, and both domain validation and the daemon conflict path perform this
self-contained timestamp/digest check before scope comparison. A valid foreign record remains an
owner mismatch; a foreign record with zero acquisition time or an invalid/mismatched digest is
classified as `storage_lock_corrupt`.

```text
source_snapshot: current dirty worktree; kiana-domain/src/storage.rs; kiana-daemon/src/storage.rs; kiana-daemon/tests/pd01_storage.rs; kiana-core/tests/pd01_storage_guard.rs
worktree_status: PD-01 row 108 remains 🔄 / feature_status=partial; no shared product-entry root, ControlPlane path, manifest or lockfile change
command_argv: RED `cargo test -p kiana-daemon --test pd01_storage daemon_storage_lock_conflict_rejects_foreign_scope_with_invalid_digest_as_corrupt --locked --offline -- --exact --test-threads=1`; GREEN daemon `cargo test -p kiana-daemon --test pd01_storage --locked --offline -- --test-threads=1`; domain `cargo test -p kiana-domain --test pd01_storage --locked --offline -- --test-threads=1`; Core `cargo test -p kiana-core --test pd01_storage_guard --locked --offline -- --test-threads=1`; final `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; every completed Cargo command used one build job/test thread, 32 MiB test stack, stripped/non-incremental profile; RED 1m46.99s/87% CPU/max RSS 4,076,316 KiB, expected exit 101; daemon GREEN 1m48.57s/86% CPU/max RSS 4,041,152 KiB; domain GREEN 59.29s/89% CPU/max RSS 4,010,220 KiB; final Core guard GREEN 0.59s/57% CPU/max RSS 136,780 KiB after the marker update; all completed commands reported 0 swaps; logs/resources `/tmp/kiana-roadmap-20261007-pd01/{red-digest,daemon-final2,domain-final2,core-guard-final}.{log,resources}`
fixture·cassette: new daemon fixtures cover foreign owner + invalid digest and foreign owner + zero time with a recomputed digest; initial RED returned `Conflict("storage_lock_owner_mismatch")` for the invalid-digest case; final daemon 11/11, domain 4/4, Core source guard 1/1; valid foreign scope behavior remains covered by existing resolver conflict fixture
exit_code: RED 101 expected; daemon/domain/Core/fmt/diff 0; one accidental concurrent Core retry was cancelled while waiting on the Cargo build lock (exit 130, no test executed) and is excluded from the green receipt
status_change: selected lock-corruption classification moves source → local_behavior; whole PD-01 remains `feature_status=partial`, `proof_level=source`, roadmap row 108 🔄
proof-level_change: deny-first integrity-before-scope behavior is locally verified; no durable/live/physical promotion
limitations: CLI/Web/Workbench shared root routing, stable restart identity and authority rebinding, stale/crash/power-loss recovery, directory fsync, cross-process fencing, network filesystem matrix, same-UID replacement race and legacy adapter migration remain open; this slice does not close PD-01
reviewer: root integration agent with PD-01 read-only audit; no human release reviewer
```

## 11. Existing non-directory root fence (2026-10-08)

The daemon resolver now rejects a configured storage root that already exists but is not a
directory. Previously `canonicalize_nonexistent` returned the canonical path for a regular file,
after which backend detection and `StorageRoot::new` accepted it as a usable root. The new
`storage_root_not_directory` denial occurs before backend detection and before any StorageRoot is
constructed; a missing leaf remains allowed for later lease initialization.

```text
source_snapshot: current dirty worktree; kiana-daemon/src/storage.rs; kiana-daemon/tests/pd01_storage.rs; kiana-core/tests/pd01_storage_guard.rs
worktree_status: resolver rejects existing regular-file KIANA_HOME roots with storage_root_not_directory; no KIANA_HOME=/ architecture change, manifest/lockfile change, or second execution path
command_argv: RED `/usr/bin/time -v env CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 RUST_MIN_STACK=33554432 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_STRIP=symbols CARGO_INCREMENTAL=0 cargo test -p kiana-daemon --test pd01_storage daemon_storage_resolver_rejects_existing_non_directory_root --locked --offline -- --exact --test-threads=1`; GREEN daemon `/usr/bin/time -v env CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 RUST_MIN_STACK=33554432 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_STRIP=symbols CARGO_INCREMENTAL=0 cargo test -p kiana-daemon --test pd01_storage --locked --offline -- --test-threads=1`; domain and Core guard use the same bounded environment and serial test threads; final format/diff checks remain required by integration
cwd·environment: repository root Linux/bash; one Cargo build job/test thread, 32 MiB test stack, stripped/non-incremental test profile; RED exit 101 / 16.68s / 83% CPU / max RSS 951396 KiB / 0 swaps; daemon GREEN 12/12 / 10.38s / 88% CPU / max RSS 952500 KiB / 0 swaps; domain GREEN 4/4 / 1.87s / 38% CPU / max RSS 251460 KiB / 0 swaps; Core guard GREEN 1/1 / 0.57s / 60% CPU / max RSS 138608 KiB / 0 swaps
fixture·cassette: `daemon_storage_resolver_rejects_existing_non_directory_root` writes a regular file as KIANA_HOME and expects `PortError::Failed("storage_root_not_directory")`; daemon full target 12/12, domain target 4/4, Core source guard 1/1
exit_code: RED 101 expected; daemon/domain/Core focused GREEN 0; format/diff follow-up handled by integration agent
status_change: selected resolver type boundary promoted source → local_behavior; whole PD-01 remains `feature_status=partial`, `proof_level=source`, roadmap row 108 `🔄`
proof-level_change: local_behavior for existing non-directory denial only; no durable/live/physical promotion
limitations: missing-root creation, symlink/race handling, shared CLI/Web/Workbench root routing, restart identity, stale/crash/power-loss recovery, directory fsync, cross-process fencing, network filesystem matrix and legacy adapter migration remain open; the exact KIANA_HOME=/ case is covered only by the later filesystem-root deny fence in section 12, not by this historical regular-file slice
reviewer: root integration agent with PD-01 read-only audit; focused local storage reviewer
```

## 12. Filesystem-root deny fence (2026-10-13)

The daemon resolver now rejects the exact filesystem root after canonicalization and before
backend detection or `StorageRoot` construction. This prevents a configured `KIANA_HOME=/` from
being handed to `StorageLease::acquire`, which would otherwise try to create the storage namespace
under the operating-system root. The fixture calls only the resolver and never acquires a lease on
`/`; non-root missing leaves keep their existing creation path.

```text
source_snapshot: current dirty worktree; kiana-daemon/src/storage.rs; kiana-daemon/tests/pd01_storage.rs; kiana-core/tests/pd01_storage_guard.rs
worktree_status: PD-01 row 108 remains 🔄 / feature_status=partial / proof_level=source; resolver-only deny-first correction, no manifest/lockfile, shared entrypoint or ControlPlane path change
command_argv: bounded serial RED exact `cargo test -p kiana-daemon --test pd01_storage daemon_storage_resolver_rejects_filesystem_root --locked --offline -- --exact --test-threads=1`; GREEN same selector; full daemon `cargo test -p kiana-daemon --test pd01_storage --locked --offline -- --test-threads=1`; Core `cargo test -p kiana-core --test pd01_storage_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo/compiler worker and one Rust test thread; 32 MiB stack; stripped/non-incremental profile; locked/offline dependencies
fixture·cassette: temporary project with `KIANA_HOME=/`; old resolver accepted `/` (RED assertion failure), corrected resolver returns `storage_root_filesystem_root_denied` before backend/lease setup; daemon target 13/13, Core guard 1/1
exit_code: RED 101 / max RSS 4095944 KiB / CPU 90% / swap 0; exact GREEN 0 / max RSS 4105356 KiB / CPU 88% / swap 0; daemon target 0 / max RSS 4092004 KiB / CPU 90% / swap 0; Core guard 0 / max RSS 4087256 KiB / CPU 90% / swap 0; all swaps 0
status_change: exact filesystem-root input is fail-closed before any storage namespace write
proof-level_change: selected resolver denial source → local_behavior; whole PD-01 remains partial/source
limitations: Windows volume-root coverage, shared CLI/Web/Workbench root routing, stable restart identity, legacy adapter migration, stale/crash/power-loss recovery, directory fsync, cross-process fencing and network filesystem race proof remain open
reviewer: root integration agent with PD-01 read-only audit; focused storage resolver reviewer
```

## 13. Shared surface resolver and restart identity (2026-10-17, CI-only)

`kiana-daemon` now owns one surface resolver in addition to the raw `resolve_storage_root`
primitive. `StorageSurface` names the three product surfaces (`Cli`, `Web`, `Workbench`),
`resolve_surface_storage_root` / `acquire_surface_storage` call the single resolver with the frozen
local scope, and `StorageSurfaceBinding` records the derived root as evidence: root id, canonical
path, owner scope digest, instance id and authority epoch, with a canonical `binding_digest`.
`StorageSurfaceBinding::validate` rejects any instance or authority epoch other than the pinned
`LOCAL_STORAGE_INSTANCE_ID` / `LOCAL_STORAGE_AUTHORITY_EPOCH`, so a binding cannot silently describe
a foreign store.

Why the frozen scope matters: `StorageOwnerScope` binds owner, instance and authority epoch into
the persisted `StoreIdentity`. A per-run or per-process instance id would force a new owner scope on
every launch and make `StoreIdentity::validate` reject the stored identity as a scope mismatch. The
local product surfaces therefore resolve with one restart-stable installation instance and one
frozen epoch; a genuine authority-epoch advance (evidence revocation, rebinding) stays a future
migration concern (PD-12+/SC-08) rather than a per-invocation choice.

The three surfaces now call that resolver:

- CLI: `architecture status` reports `storage_surface` / `storage_root` / `storage_root_id` /
  `storage_binding_digest`; `kiana run` resolves the shared root before it starts (fail closed).
- Web: `run_web` resolves the root before serving and prints `storage root: <path> (<root_id>)`,
  and the desktop-ready record carries the same ids.
- Workbench: `run_workbench` resolves the root before its first turn and prints it in the banner.

Resolution is metadata-only: it never acquires the writer lease, authorizes a command or opens a
second execution path. Acquiring the single-writer lease remains an explicit, separate call.

```text
source_snapshot: current dirty worktree; kiana-daemon/src/storage.rs; kiana-daemon/src/lib.rs; kiana-daemon/tests/pd01_storage.rs; kiana-core/tests/pd01_storage_guard.rs; kiana-entrypoints/src/{cli,web,workbench}.rs; .github/workflows/pd01-storage-root.yml
worktree_status: PD-01 row 108 🔄 → ✅ (CI pending) / feature_status=implemented / proof_level=source; no ControlPlane/CapabilityBroker path, no manifest or lockfile change
command_argv: CI-only; no local cargo/build/test/fmt/clippy/smoke; static review plus `python3 scripts/roadmap-goal.py --json`, `bash scripts/ci/validate-doc-references.sh` (732 docs pass); `bash scripts/ci/validate-workflows.sh` not run (environment lacks ruby), workflow YAML reviewed by eye only
cwd·environment: worktree /workspace/kianacode-worktrees/PD-01, branch step/pd-01-20261017-1, Linux/bash; GitHub Actions ubuntu-latest is the runtime test authority (receipt not awaited)
fixture·cassette: daemon `pd01_storage` target +3 — `daemon_storage_surface_roots_are_identical_across_surfaces` (cli/web/workbench resolve one root id/canonical path/owner scope), `daemon_storage_surface_binding_rejects_scope_and_digest_tampering` (`storage_surface_binding_scope_invalid` / `storage_surface_binding_digest_mismatch`), `daemon_storage_identity_survives_a_new_process` (child process creates the store, parent reacquires the same `store_id`); Core guard `pd01_storage_guard` adds `product_surfaces_share_one_storage_resolver_without_a_second_root`; `pd01-storage-root.yml` adds a `cargo check -p kiana-entrypoints --locked` surface compile step
exit_code: local runs not executed; CI receipt not awaited
status_change: PD-01 card exit conditions covered at source level; row 108 ✅ (CI pending); feature_status=implemented
proof-level_change: source only; no local_behavior/durable/live/physical promotion
limitations: the shared root is proven for the local product instance only; authority-epoch advance/rebinding, stale/crash/power-loss lock recovery, directory fsync, cross-machine fencing, network filesystem race proof, Windows volume-root coverage, legacy `KIANA_HOME` adapter migration and full cross-surface UAT (PD-34/35) remain open; `validate-workflows.sh` could not run (ruby absent) so workflow YAML is not machine-validated; CI receipt for this slice is not yet read
reviewer: root integration agent with PD-01 read-only audit
```
