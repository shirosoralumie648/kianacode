# P1-D-01 single WorkPacket ready predicate baseline

> 快照日期：2026-09-16。本页记录 WorkPacket readiness 的单一 domain 谓词及 legacy project-board 委托；本地不运行测试，运行时/guard 夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P1-D-01`](../roadmap.md#step-p1-d-01) |
| feature_status | `implemented`（domain predicate + core/legacy adapter source） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | CompanyState/ControlPlane/legacy task adapter → `kiana_domain::ready_packets(graph, now)` |
| this step does | 统一状态、依赖、deadline 和 claim lease 的 readiness 计算；legacy `kiana-tasks` 仅委托 domain，不复制逻辑 |
| this step does not | 不实现 dependency cycle repair、lease reclaim、durable queue 或 scheduler dispatch；P1-D-02/03 与 AUT/PD 后续步骤负责 |

## 2. Readiness contract

`kiana_domain::ready_packets` 先验证 packet identity、依赖存在性/重复和 DAG，然后按确定性拓扑计算：Succeeded/Reviewed/Closed 不再 ready；Failed/Cancelled、依赖 blocked/incomplete、deadline expired、active claim 和非 Approved/Assigned 状态分别产生稳定 blocker reason；过期 claim 只进入 `expired_claims`，真正 reclaim 由后续命令完成。结果 ready/expired 均排序，避免 map/insertion order 造成漂移。

`kiana-core` 的 Company snapshot、CompanyState command paths 和 `kiana-tasks::ready_packets` compatibility wrapper 都消费这一实现。旧 `ProjectBoardStatus` 仍服务于 legacy task-card completion-gate 展示，但不再声称是 WorkPacket readiness authority。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `single_ready_predicate_agrees_across_three_callers` | domain canonical、legacy adapter 和 core/Company source consumers 对依赖/claim/deadline 得到同一 deterministic result |
| `product_callers_share_domain_ready_packets_predicate` | core/domain/legacy source 均指向唯一 `kiana_domain::ready_packets`，未新增第二 ready 算法 |

`.github/workflows/p1-d01-ready-predicate.yml` 在 GitHub runner 执行 readiness fixture、core source guard、fmt 和 domain/tasks/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 当前 readiness 只读计算，不改变 packet 状态、不回收 claim、不写 EventLog；P1-D-02/03 负责 DAG admission 与 claim/lease lifecycle。
- `kiana-tasks` 是兼容 crate，wrapper 仅转发 domain predicate；旧 board 的 completion gate 不是 Company WorkPacket 的事实源。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
