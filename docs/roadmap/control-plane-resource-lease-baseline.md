# CP-12 resource lease 与 fencing 基线

> 快照日期：2026-09-17。本页记录路径写集、资源 lease 和 fencing token 的 source/CI 边界；不把
> domain lease 当成已持有 OS 锁或 handler permit。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CP-12`](control-plane.md#step-cp-12) |
| feature_status | `implemented`（strict ResourceLease、canonical write-set、Cell capability fence token） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | ControlPlane current authority epoch + existing kernel-backed path-lock adapter |
| this step does | owner run/cell/session、resource digest、authority epoch、monotonic sequence、expiry、successor fencing、canonical path set、O_NOFOLLOW/LOCK_NB boundary |
| this step does not | 不声称 lease 自身持有 OS lock、不在 approval wait 自动释放 Cell 写集、不实现跨进程 durable lease projector、Broker permit/effect-time atomicity 或 external/live/physical proof |

## 1. Contract

`kiana-domain/src/resource_leases.rs` 新增 strict `ResourceLease`，绑定
`StorageLockId`、随机 `FenceTokenId`、canonical resource、owner run/cell/session、authority
epoch、sequence/parent digest、TTL、resource/lease digest。旧 lease 只能被新 token 的 successor
替代；过期、epoch drift、token mismatch、resource overlap 和 digest/unknown field 错误均
fail-closed。`canonical_resource_set` 对空集明确变为 exclusive `*`，不再静默过滤非法路径。

现有 `kiana-core::sessions` 继续使用单一 kernel-backed path-lock（Unix `flock`、`O_NOFOLLOW`、
non-blocking acquisition），但先通过 canonical write-set；`CapabilityLease` 增加 server-generated
fencing token，完成/结算必须原样带回，防止 stale worker 使用复制的 lease。Core 提供
`issue_resource_lease`/`validate_resource_lease`，只读取 authority epoch，不打开第二执行循环。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `resource_lease_binds_owner_scope_epoch_and_fencing_successor` | owner/resource/epoch/TTL、successor sequence 和新 token；旧 token 被拒 |
| `resource_write_set_is_canonical_and_never_silently_filters_invalid_paths` | 空集/wildcard/dedup canonical，absolute/parent escape 拒绝 |
| `resource_lease_rejects_scope_epoch_and_digest_tampering` | strict resource/digest tamper fail-closed |
| `cp12_resources_use_canonical_paths_and_fencing_without_a_second_execution_path` | source guard 固定 kernel lock、authority recheck、fencing boundary |

## 3. Proof ceiling and handoff

CP-12 proof ceiling 为 `source`：路径规范化、kernel lock adapter 和 typed fencing 入口已固定，但
lease/lock 事实仍未由 durable projector 与 Cell/Grant/Budget/Approval 原子合并；approval wait
释放物理锁、过期 worker 隔离、Patch/MCP effect-time、CP-13 permit、CP-15 cancellation、跨进程
crash recovery 和 external/live/physical proof 留待后续步骤。
