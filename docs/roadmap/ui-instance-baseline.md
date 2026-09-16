# UI-03 local instance identity/discovery/transport baseline

> 快照日期：2026-09-16。本文记录 UI-03 的本地 instance ID、ready record、single-instance lock 与 peer 校验；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`UI-03`](ui-entrypoints.md#step-ui-03) |
| source snapshot | `8fe8338`（UI-02 typed handshake 提交后的干净基线） |
| feature_status | `implemented`（daemon instance lease/discovery + protocol record source） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | workspace → `.kiana/instances` regular root → create-new lock + ready record → discover/validate protocol/workspace/epoch peer → existing ControlPlane transport |
| this step does | protocol 新增 `UiTransportKind`/`UiInstanceRecord`（instance/authority epoch、protocol/workspace/endpoint digest、PID、ready、record digest）；daemon `InstanceLease` create-new single lock、0600 record/lock、bounded parse/discovery、duplicate/stale/foreign workspace/epoch/protocol fail-closed；DaemonHost 暴露 acquire_instance，client 继续只传输 typed commands |
| this step does not | 不自动启动第二 daemon、不执行 shell/runner、不以 socket/URL/PID 作为认证、不把 ready/health/HTTP ACK 当执行事实；Unix socket/named pipe listener、peer auth、重启 epoch 持久递增和 UI-04 action journal 仍后续实现 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Protocol instance record | `kiana-protocol/src/ui_contracts.rs` | `7703e4e6052d34b488185065234cff111d6de0d223fc6e563ae1be3216d5497e` |
| Daemon lease/discovery | `kiana-daemon/src/instance.rs` | `7e4c9ad7aa50bbd622bfece6dd193919c87b0da328fcdaf5b9f57e9e71fbb92e` |
| DaemonHost integration | `kiana-daemon/src/lib.rs` | `24ce8bab6d72ed33808d0d3573b8c51591bf870285bab83d009a5a7556761a68` |
| Fixtures | `kiana-daemon/tests/ui03_instance.rs`, `.github/workflows/ui03-instance.yml` | `da8382243ed518754d8c42e9be1cd5f38293a803a995d412ec1e6b0044f52768`, `38a64d5cfb67caa4fc787e30be02af6056e484e5298a5a5a8a86826da30fc981` |

hash 只用于 UI-03 源码漂移复核，不构成真实 socket transport、跨进程认证、持久 authority epoch、命令执行或业务结果证明。

## 2. Instance record and lock

`InstanceLease::acquire` 将 workspace canonical identity 绑定到 `.kiana/instances/instance.lock` 和 UUID 文件名 ready record。lock 使用 `create_new`，已有 lock 返回 `ui_instance_already_running`，不删除/接管旧 PID 或 socket；record/lock 只写 regular non-symlink 文件并在 Unix 使用 0600。record 的 workspace/endpoint 只保存 digest，instance/epoch/PID/协议 schema/ready 和 record digest 受 bounded 校验。

`discover` 要求 regular lock、严格权限、恰好一个 JSON record，使用 domain bounded JSON（duplicate key/size guard）解析并校验 record digest、protocol schema、workspace digest、ready/epoch。多 record、缺 lock、foreign workspace、错误 protocol/epoch 都不会回退成“无实例”或自动新建。

DaemonHost 提供 `acquire_instance` 作为显式本地生命周期 API，仍将所有命令交给既有 ControlPlane；instance record 是发现元数据，不是 capability grant。释放 lease 先关闭 lock descriptor，再移除明确的 record/lock 路径；清理失败不改变已提交业务事实。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `instance_lock_record_discovery_and_peer_checks_are_fail_closed` | create/discover/peer 成功；重复 acquire、foreign workspace/epoch、释放后发现均拒绝 |
| `instance_record_uses_protocol_schema_and_does_not_expose_endpoint_path` | record 使用 protocol schema/digest，不把 endpoint 原文写入 record |
| `symlink_workspace_and_record_paths_are_rejected` | symlink workspace 不可作为实例根 |

`.github/workflows/ui03-instance.yml` 在 GitHub runner 执行 daemon instance fixtures、compile 和 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 4. 限制与交接

- 当前 lock/record 是 workspace-local 文件，尚无跨进程 heartbeat/PID start-time 证明、崩溃残留恢复、authority epoch durable counter 或真正 Unix socket/named pipe listener；旧锁不自动接管，需显式运维清理/后续恢复门。
- peer 校验只绑定 workspace/protocol/epoch/digest，不提供 OS credential/token/mTLS 等认证；loopback/socket 可达不等于 trusted peer，UI-04/SC 仍需 ControlPlane 授权。
- `UiInstanceRecord.ready` 由显式 lease 写入，尚未与 DaemonHost readiness/health projector 原子绑定；ready record 不表示 EventStore、Runner、Provider 或外部连接可用。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
