# SC-09 GrantScope intersection 与 Cell inheritance 基线

> 快照日期：2026-09-17。本页记录 policy/domain source contract 与 GitHub CI-only fixture；不把
> 纯值交集写成已经签发 permit、完整 Cell 生命周期或 Broker effect 证明。

| 项目 | 记录 |
|---|---|
| roadmap card | [`SC-09`](security-compliance.md#step-sc-09) |
| feature_status | `implemented`（GrantScope、ScopeSet intersection、legacy grant adapter） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | ControlPlane/CellRegistry 仍是实际 grant/lease authority；GrantScope 只提供 narrowing value |
| this step does | parent/template/department/project/packet/approval layers 的交集模型、capability/secret/external/path/budget narrowing、subset/expiry/epoch guards |
| this step does not | 不实现 durable Grant ledger、Approval consumption、SecretStore、TOCTOU/egress、child lifecycle 或第二授权循环 |

## 1. Contract

`kiana-policy/src/grant_scope.rs` 新增 strict `GrantScope`：绑定 `GrantId`、optional parent、
principal/project、复用 domain `ScopeSet` 的 operations/paths/namespaces/network/budget/depth，
以及独立 capability、secret、external、delegation、authority epoch 和 expiry 维度。构造和
`intersect/intersect_all` 只做集合/预算/expiry/boolean narrowing：不同 principal/project/epoch、
空 capability/scope 交集、secret/external 与 capability 不一致、grant self-parent、重复/非
canonical capability 全部拒绝。`contains` 只能证明 child 是 parent 子集，不能转移或扩权。

`from_capability_grant` 是只读兼容 adapter，将历史 grant 映射为一层显式 scope；它不会把旧
`CapabilityGrant` 当作新的授权事实。`allows_request` 仅检查已经存在的 scope/capability/risk/
expiry，不调用 Broker 或 handler。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `grant_scope_intersects_all_layers_without_union_or_transfer` | parent/child 交集严格收窄并满足 subset |
| `grant_scope_rejects_empty_capability_intersection_cross_scope_and_mixed_dimensions` | 空交集、跨主体/项目和 secret/external 混用拒绝 |
| `grant_scope_allows_only_explicit_capability_scope_and_expiry` | operation/path/risk/expiry 边界不越权 |
| `historical_capability_grant_adapts_to_narrow_scope` | legacy grant 只读转成窄 scope |
| `grant_scope_reuses_scope_set_and_cannot_create_a_broker_or_union_path` | source guard 固定复用交集算法、无 union/Broker/执行依赖 |

## 3. Proof ceiling and handoff

SC-09 的 proof ceiling 是 `source`：交集、subset 和 deny-first 值对象已经固定，但尚未在所有
Cell/Grant/Approval/Packet 层形成 durable 原子 admission；现有 MemoryCellRegistry 和
ControlPlane 仍保留历史兼容路径。SC-10 将把精确 approval 绑定到该 scope，SC-12 将把 scope/epoch
接入 permit/CAS；Secret、TOCTOU、network、cross-process revoke 和 external/live/physical proof
仍未宣称。
