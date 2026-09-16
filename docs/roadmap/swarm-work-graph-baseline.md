# SW-02 typed Swarm WorkGraph baseline

> 快照日期：2026-09-16。本页记录显式 Partition/WorkGraph 规划合同；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`SW-02`](../roadmap.md#step-sw-02) |
| feature_status | `implemented`（typed Partition/WorkGraph validator + deterministic projection） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | typed WorkGraph → shared `packet_graph::validate_dependency_graph` → existing ControlPlane Swarm/EventLog path |
| this step does | Partition input/data/path/output/fingerprint 合同、依赖 cycle/missing/duplicate、scope/path overlap、count/depth/concurrency/spawn-rate/TTL/budget 上限、`first_success` 拒绝、稳定 ready/blocked/failed projection |
| this step does not | 不实现 queue/claim/scheduler、child materialization、attempt/dispatch durable facts、merge/replay/recovery 或 effect-time fencing；SW-03+ 负责 |

## 2. Validator rules

`Partition` 使用 domain-owned `PartitionId`/`SwarmPlanId`，规范化并锁定 input refs、data scope、owned paths、output contract/version、预算和 `WorkFingerprint`。空输入、重复输入、nil ID、digest/fingerprint drift、未知字段和错误 schema fail-closed。

`SwarmWorkGraph` 在新 SwarmPlan 的 `work_graph` 可选字段上接入：存在 typed graph 时，`SwarmState::Create` 在任何状态写入前调用 `validate`，并要求 graph swarm ID 与 packet plan 对齐；旧字符串 packet plan 在迁移期继续可读。路径使用 shared lexical containment/conflict helper，data scope 采用确定性前缀/通配冲突规则。

依赖边先转换为字符串键，再复用 `packet_graph::validate_dependency_graph`，因此 cycle 规范化、missing ref 和 duplicate edge 与 WorkPacket 图保持同一实现。投影只读地按同一拓扑输出排序后的 ready/blocked/failed；它不会创建 claim 或执行 Broker。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `swarm_plan_rejects_partition_overlap_and_unbound_input` | 重叠 owned path、空 input refs 均在构造/图校验前拒绝 |
| `work_graph_rejects_cycle_missing_duplicate_and_first_success` | cycle/missing dependency、重复 fingerprint 与 `first_success` fail-closed |
| `work_graph_rejects_limits_and_preserves_stable_projection` | count/depth/concurrency/spawn-rate/TTL/budget 上限拒绝，投影稳定排序且 unknown fields 拒绝 |
| `swarm_work_graph_uses_shared_packet_graph_and_keeps_execution_in_control_plane` | domain 复用 shared packet graph；Swarm 仍通过现有 ControlPlane/EventLog 唯一路径 |

`.github/workflows/sw02-work-graph.yml` 在 GitHub runner 执行上述 domain fixtures、core source guard、fmt 和 domain/protocol/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- `SwarmPlan.work_graph` 目前是迁移期 optional；没有 graph 的旧计划仍按旧 packet checks 运行，不能声称所有 Swarm 已使用 typed graph。
- `PartitionProjection` 是 read-only planning view，ready 不产生执行权；DispatchIntent/QueueEntry、capacity/fair ordering、claim lease 和 retry 由 SW-05/06 负责。
- WorkFingerprint 是现有 domain 的确定性 FNV 兼容指纹，不是密码学签名；真实跨进程唯一性、持久 CAS 和 worker 效果仍需 EventLog/PD/SC 证据。
