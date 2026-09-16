# P1-D-02 dependency graph validation baseline

> 快照日期：2026-09-16。本页记录 WorkPacket 显式依赖、缺失/重复边和确定性环检测，并把候选 packet 的图校验接到批准前；本地不运行测试，运行时/guard 夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P1-D-02`](../roadmap.md#step-p1-d-02) |
| feature_status | `implemented`（domain DAG + Company approval source） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | WorkPacket.dependencies → `validate_dependency_dag` → CompanyState packet approval/plan/claim admission |
| this step does | 显式依赖边、缺失/重复检测、规范化环输出；ApprovePacket 在写入前把候选合并到项目图并 fail-closed |
| this step does not | 不实现依赖自动修复、队列 lease reclaim 或 scheduler dispatch；P1-D-03/AUT/PD 后续步骤负责 |

## 2. Dependency contract

`validate_dependency_dag` 要求 map key 与 packet ID 一致，拒绝空/错误 identity、重复依赖、缺失依赖和超大图；DFS 使用 BTreeSet/BTreeMap 稳定遍历，环从字典序最小成员开始并在末尾重复起点，错误包含稳定 code/packet_id/cycle。CompanyState 的 `ApprovePacket` 先检查跨项目依赖，再把候选 packet 放进 project graph 验证，只有通过后才追加事实；PlanProject、Claim 和 dispatch 继续复用同一 domain helper。

依赖图验证只判断结构和前置状态，不授予执行能力；ready、claim、budget、policy、approval 和 Broker admission 仍是独立边界。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `dependency_cycle_is_rejected_deterministically` | 相同环两次输出完全一致，cycle 从最小成员开始并闭合 |
| `dependency_missing_and_duplicate_edges_fail_closed` | 缺失/重复依赖在任何状态推进前拒绝 |
| `packet_approval_and_dispatch_use_the_domain_dependency_graph` | Company approval/plan/claim/dispatch source 均调用 domain DAG，候选 packet 不绕过图校验 |

`.github/workflows/p1-d02-dependency-graph.yml` 在 GitHub runner 执行 domain graph fixtures、core source guard、fmt 和 domain/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- DAG 校验不会自动把 blocked 父 packet 改写为子 packet 状态，也不会回收 claim；这些是 readiness/lease lifecycle 语义。
- 当前 CompanyState/EventStore 仍由后续 ER/PD 收口 durable migration/replay；图错误的稳定 reason 只在本地 domain/core source 层验证。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
