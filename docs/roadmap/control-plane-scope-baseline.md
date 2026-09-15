# CP-04 scope intersection and monotonic authorization baseline

> 快照日期：2026-09-16。本文记录 CP-04 的 ScopeSet、权限交集和 core 单调决策 source slice；运行时夹具只在 GitHub CI 执行，本地不运行测试。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CP-04`](control-plane.md#step-cp-04) |
| source snapshot | `4903fdb`（CP-03 action catalog 校正后的干净基线） |
| feature_status | `implemented`（typed scope intersection + deny-first merge source） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，运行时测试由 GitHub CI 执行 |
| canonical path | server principal/project/assignment → action normalization → ScopeSet intersection → policy → gate/hook/approval → permit/Broker |
| this step does | 区分 `NotApplicable` 与 `Restricted`，对 operation/path/namespace/network/budget/depth 求交，提供 subset/digest/limits，并在 prepare 阶段拒绝空交集；固定 policy/gate/hook 的 Deny/Ask 单调语义 |
| this step does not | 不把 AllowAll/模型声明/Approval/Grant/Cell DTO 本身当权限；不新增第二授权循环、不执行 I/O、不声称完整 tenant/OS/外部 effect 或 durable grant epoch |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Scope contract | `kiana-domain/src/scope.rs` | `6cda19bf05066994580b1e5aa434bfc718712fc33b179e053ea6f42126a2f390` |
| Domain/action context | `kiana-domain/src/capabilities.rs`, `kiana-domain/src/actions.rs`, `kiana-domain/src/contracts.rs`, `kiana-domain/src/lib.rs` | `1e8c6cb45c2875fcc35a9c876cc780bc2011913d15db7d4376a8ced1c117b522`, `cd170e01e581015c917fdc0ac2c419149349a5de88164c9f7369c07eecd0a020`, `7b9e7fe0b061d5f6d85bf789552e50d14781aa9f8390ed17fadb6c159beb8955`, `4d77c0c495963746f22150fc1f74f4cdf7a2629537f318fc91b12647a81e3999` |
| Core/policy/gate | `kiana-core/src/capabilities.rs`, `kiana-core/src/approvals.rs`, `kiana-core/src/cell_registry.rs`, `kiana-policy/src/lib.rs`, `kiana-gates/src/lib.rs` | `f869c545b6aaf24f494bceb2352ef0a1874ff503de55e1bdf951f5414f7b435a`, `439a602ee0463d1be33ce144d787e5b7b6cbdb19db7277de2da248aa0cf28b1e`, `218ae2f9efec2c173881cd5d33d41d414c7e76f9db9e130d6518bfde602d9663`, `aad5391917fdab78dd60fdeea3f70733c19b076c60b3d2154eda8f65abd9e389`, `b240b7e74dedb7f7b8eb05ca0fda2f7fc0cd5df2925c8fb61b0c147c7d18425e` |
| Remote fixtures/guard/workflow | `kiana-domain/tests/cp04_scope.rs`, `kiana-core/tests/cp04_scope_guard.rs`, `.github/workflows/cp04-scope.yml` | `4a957019b5dbe0a257aeff8b202db9ccbb7476c9c4347bdbeb698b1c04fac085`, `4b7c919c768682fd334117eda8746bb84f76444f6c783a67ff27ab2a1359aa7e`, `f310a4ee8342ab069b1c67926558db66049905eca45a760462f8c666ada0d322` |

hash 只用于本步源码漂移复核；它不是授权 token、撤销 epoch、持久 Grant 或外部副作用证明。

## 2. ScopeSet contract

`kiana-domain/src/scope.rs` 定义 `ScopeSet`：

| 维度 | `NotApplicable` | `Restricted(values/limit)` |
|---|---|---|
| operations | 本层不限制 operation | 仅允许列出的 canonical operation；空集合与其他 Restricted 求交时 fail-closed |
| paths | 本层不限制 path | 规范化相对路径集合；path 交集按父/子前缀取更窄值，不把两侧拼成并集 |
| namespaces | 本层不限制 collection/account namespace | 只保留双方共有 namespace |
| network | 本层不限制 server/endpoint | 只保留双方共有网络目标；具体 DNS/SSRF 仍由后续 CAP/SC 负责 |
| budget | 本层不提供预算限制 | 双方取较小上限，`Restricted(0)` 仍与 NotApplicable 不同 |
| depth | 本层不提供深度限制 | 双方取较小上限，禁止子级增加 depth |

每个 ScopeSet 带 `kiana.scope-set.v1`、版本和 `scope_digest`。values 数量/长度、NUL、重复、未知字段和非 canonical 顺序均拒绝；digest 只证明值完整性，不是授权凭证。`intersect_all` 没有 layers 时拒绝，任何 Restricted 维度的空交集返回 `scope_intersection_empty`。

## 3. Core enforcement

`prepare_capability_action_cancellable` 在最终 hook 归一化后构造 action scope 与 server `RequestContext.path_allow` scope 并求交。请求路径、MCP server、Memory collection 和 canonical operation 只会被缩减；`..`、NUL、无效 context path、未知 operation 或空交集在 policy/gate/Broker 前失败。Cell/Grant 继续由 `MemoryCellRegistry` 的父子 `grant.contains`、owned path 和 budget/lease 检查收缩，ScopeSet 不取代其事实账本。

Policy/Gate 合并规则：

1. DefaultPolicy 的 hard denial（untrusted project、actor/context incomplete、role/tool/path/memory/operator mismatch、action risk violation）先于可替换 policy，不能被 AllowAll 覆盖；
2. 任一 policy/gate/hook `Deny` 保持 Deny，稳定 reason 原样落账；
3. 任一层 `Ask` 保持 AwaitingApproval，普通 Allow 不能消除其他层的 approval requirement；多个 Ask requirement 排序、去重后合并；
4. 只有 policy/gate authorization ID 非空且完全一致，才可进入 ControlPlane 的授权封装；Gate 不 mint 新 ID；
5. Approval 只 discharge 相同的 requirements/action digest/subject，不扩大 scope。PreparedAction、Grant、Authority、Budget、Lease、Permit 仍各自有独立校验。

## 4. Deny-first fixture matrix

| Fixture | 先证明的拒绝 | 成功/回归边界 |
|---|---|---|
| `cp_allow_all_gate_cannot_override_hard_deny` | untrusted/role/path/action mismatch 即使 Gate AllowAll 也不得到 Broker | 仅在 server context 与 action scope 均有效时继续 |
| `cp_hook_allow_cannot_discharge_other_approval_requirement` | policy Ask 或其他 hook Ask 不能被 Allow hook 消除 | requirements 按集合合并，Approval 只匹配精确 digest |
| `cp_child_scope_is_subset_for_every_dimension` | child operation/path/namespace/network/budget/depth 超过 parent 拒绝 | child 交集可用于受限 cell/packet/approval，不能 union |
| `scope_intersection_is_monotonic_and_path_aware` | 父/子 path 交集、预算/深度取 min、空交集 | 子 scope 对每个维度 `is_subset_of(parent)` |
| `not_applicable_does_not_mean_empty_restricted_scope` | NA 与空 Restricted 的语义不可混淆 | NA 作为交集单位，空 Restricted 保持拒绝 |

## 5. 限制与交接

- ScopeSet 目前是 domain/core 的 typed source contract；Cell/Grant/Approval/authority epoch 尚未统一持久到一个 ExecutionContext，完整权限交集和撤销由 CP-08/09、CAP-03、SC-07..10 继续完成。
- `NotApplicable` 仅表示本层不约束，不表示全局 unrestricted；真实 role/project/path/network/secret policy 仍必须在上层提供 Restricted scope。
- path 前缀交集是字符串级 canonical helper，不替代 descriptor-relative open、inode/generation、bind-mount、DNS rebinding 或 OS sandbox 证明。
- Policy/Gate/Hook 的纯函数单调性不证明人工审批、Broker handler、外部账户或现实 effect；Allow 也不等于已执行。
- 本地只做静态编译；GitHub CI 结果按用户要求不等待，历史局部行为证据仍绑定各自快照，不提升 durable/live/physical。
