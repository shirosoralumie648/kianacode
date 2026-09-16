# SW-01 typed Swarm lineage baseline

> 快照日期：2026-09-16。本页记录 SwarmPlan/Partition/ChildCell/Attempt/DispatchIntent/QueueEntry/MergeDecision 的稳定 ID 与 lineage；本地不运行测试，运行时/guard 夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`SW-01`](../roadmap.md#step-sw-01) |
| feature_status | `implemented`（typed IDs/lineage + protocol/ports source contract） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | typed SwarmLineage → protocol/ports → existing ControlPlane Swarm/Company EventLog CAS |
| this step does | 七类稳定 Swarm IDs、strict/digest/authority epoch/revision lineage、cross-swarm/self-parent rejection、protocol/ports export |
| this step does not | 不替换旧 SwarmPlan 字符串字段，不实现 Partition/WorkGraph validator、durable DispatchIntent/QueueEntry、scheduler 或 child execution；SW-02+ 负责 |

## 2. Lineage rules

`SwarmLineage` 绑定 swarm plan、partition、child cell、attempt、dispatch intent、queue entry、merge decision、可选 workflow/parent/root run、correlation/causation、authority_epoch 和 revision。所有 IDs 都是 domain-owned UUID types，canonical serialization/digest 防止重放时跨 swarm 混淆；`validate_for_swarm` 拒绝把 lineage 用到另一个 swarm，self-parent/zero epoch/revision/unknown fields fail-closed。

旧 `SwarmPlan.swarm_id`、`packet_ids` 和 `SwarmChild.packet_id` 继续兼容读取；它们不能单独证明 typed lineage、attempt identity 或 dispatch authority。现有 core `commit_swarm`/`handle_company_command` 仍是唯一执行脊柱，lineage port 只保存/读取事实，不执行 Broker。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `swarm_ids_and_lineage_are_stable_and_reject_cross_swarm_partition` | typed IDs round-trip、digest/unknown-field guard 和 cross-swarm mismatch 拒绝 |
| `lineage_rejects_self_parent_and_zero_epoch` | self-parent/无有效 authority epoch 的 lineage 不可构造 |
| `swarm_paths_expose_typed_lineage_without_a_second_execution_loop` | domain/protocol/ports/core source 保持同一 Swarm→Company/Broker 路径 |

`.github/workflows/sw01-lineage.yml` 在 GitHub runner 执行 domain lineage fixtures、core source guard、fmt 和 domain/ports/protocol/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 当前 typed lineage 尚未写入每个现有 SwarmEvent/ChildAttempt；SW-02/03/06/07 负责把它绑定到 Partition/Attempt/DispatchIntent durable facts。
- ID/digest 存在不代表跨进程唯一性、真实 worker 存活或外部效果；authority/Grant/Budget/Approval/lease 必须独立校验。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
