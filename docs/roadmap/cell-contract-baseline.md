# P1-C-01 organization and Cell contract baseline

> 快照日期：2026-09-16。本页记录 AgentTemplate、CellSpec、SpawnPlan、BudgetLease、CapabilityGrant、SupervisionLease 六类合同及子授权收窄规则；本地不运行测试，运行时/guard 夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P1-C-01`](../roadmap.md#step-p1-c-01) |
| feature_status | `implemented`（domain contract + core admission source） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | domain contract → ControlPlane CellRegistryPort → MemoryCellRegistry admission → Broker lease consumer |
| this step does | 六类结构化合同、schema/unknown-field fence、模板版本绑定、默认不可再委派、grant/path/budget/supervision 校验和 child scope intersection |
| this step does not | 不实现 durable Cell projector、跨进程 lease recovery、完整 scheduler 或实际多 Agent 并行运行；这些由 P1-C-02、SW、AUT、ER/PD 后续步骤负责 |

## 2. Contract rules

- `AgentTemplate` 是按角色和固定版本解析的 immutable 模板；`CellSpec.template_id/template_version/role_id` 必须与注册模板一致，未知字段被拒绝。
- `SpawnPlan` 是 proposed→validated→reserved→committed 的计划合同；`CellSpec` 绑定 parent、partition、input/output refs 以及 grant/budget/supervision IDs，不能只凭模型文本创建 Cell。
- `BudgetLease` 只允许在 max tool/token/wall/concurrency/effect 限额内 reserve/consume；`SupervisionLease` 限制 heartbeat、stall 和 retry。
- `CapabilityGrant::contains` 要求 child capability/operation/resource/path/expiry 都是 parent 的子集，且 child 不能在 parent 不允许时打开 delegation；CellRegistry 在 reserve 和 snapshot 恢复时重复检查。
- 新建模板默认 `delegation_allowed=false`；只有显式 controller 模板在受控 registry 中开启委派，并同时受 max_children/max_depth 与 parent grant 约束。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `child_grant_cannot_exceed_parent_grant` | 子 grant 扩大 path、expiry 或 delegation 时不再是 parent 的子集 |
| `templates_pin_version_and_default_to_non_delegable` | RoleSpec 生成的模板固定版本、默认不可委派，unknown field fail-closed |
| `cell_admission_consumes_versioned_contracts_and_intersects_child_scope` | core 注册表解析模板版本，重复校验合同，并在 child admission 检查 parent grant containment、delegation、budget、supervision |

`.github/workflows/p1-c01-contract.yml` 在 GitHub runner 执行 domain fixtures、core source guard、fmt 和 domain/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 当前 `MemoryCellRegistry` 是进程内 adapter；Cell/Grant/Budget/Lease 状态尚未由 EventLog durable projector 重建，也没有跨进程 stale lease recovery。
- 六类合同和 source guard 证明代码边界，不证明所有入口都已通过 CellRegistry；P1-C-02、P1-H、SW、CP/ER/PD 继续覆盖生命周期、权限、恢复和事实账本。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
