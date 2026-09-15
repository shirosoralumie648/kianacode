# CAP-03 immutable ExecutionScope baseline

> 快照日期：2026-09-16。本文记录 CAP-03 的 server-derived ExecutionScope 与 authority-chain 交集边界；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CAP-03`](capability.md#step-cap-03) |
| source snapshot | `8996d12`（CAP-02 input/digest 后的干净基线） |
| feature_status | `implemented`（typed ExecutionScope source + ControlPlane/Broker binding） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | authenticated RequestContext + normalized action → ScopeSet intersection → ExecutionScope → PreparedAction/permit/Broker/handler |
| this step does | 明确 principal/project/session/run/turn/cell、Grant/Budget refs、roots/denies、Memory/server/network、workspace/environment、authority/trust/data/cancel epochs、deadline/fencing/catalog/action/scope digests；Broker 在 handler 前校验 scope |
| this step does not | 不让 caller/model 直接 mint scope，不把 scope 当 Grant/Approval/Permit，不宣称完整 authority epoch、durable ToolSnapshot、OS containment、外部 effect 或跨进程恢复 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain ExecutionScope/request | `kiana-domain/src/execution_scope.rs`, `kiana-domain/src/capabilities.rs`, `kiana-domain/src/contracts.rs`, `kiana-domain/src/lib.rs` | `0a9b7e16f2ad27069a24434341b91686ef0129e8975968e439f2abb72fa3961b`, `8a3d7a4486e1db7c624f094b4f1714006baf68f25279bb02940dcae94fa3c8f9`, `5795c39ef3fa7087d51bd74090c5462775577aaf9d432a6cae657c7654018d20`, `a642faa64150ddd7111aca2b999e5945e95299363830657a3b5ff355530b95be` |
| Core/Broker/ScopeSet | `kiana-core/src/capabilities.rs`, `kiana-core/src/invocation_projection.rs`, `kiana-core/src/events.rs`, `kiana-capability-broker/src/lib.rs`, `kiana-domain/src/scope.rs` | `099355117c49223b60d1d3df2fd8e4a73a6908f2c3f4e36afe5309ee73311023`, `95a6810846c97ae66f38b9f2939895842576e21431dc0898e9dcf236d12d7cb0`, `bac9e871d5b55850025a135cc0a56c584e271027b844c5aae87a81ef5d2eaf20`, `c535f59333c07f437f81126b70340ff0fe64a4b3c2f75eef03c7e46fd158e093`, `6cda19bf05066994580b1e5aa434bfc718712fc33b179e053ea6f42126a2f390` |
| Fixtures/guard/workflow | `kiana-domain/tests/cap03_execution_scope.rs`, `kiana-core/tests/cap03_execution_scope_guard.rs`, `.github/workflows/cap03-execution-scope.yml` | `c8422cb4117b5da20dad33c5d1b425db6aa9b0023931c167e9230215d346d61f`, `8950725595bc3aebce3d52d4e5ce6dcb089fd65bc8181dd747600e29da0242e0`, `979e53242354f7a8fed94d3188567faa6b794a1b6602459b3c89e752e68f3389` |

hash 只用于 CAP-03 Scope 漂移复核，不构成 Grant/Approval/Permit 或外部执行证明。

## 2. ExecutionScope fields

| 组 | 字段与来源 | 约束 |
|---|---|---|
| identity | `principal`、`project`、`session_id`、可选 `run_id`/`turn_id`/`cell_id` | 由 server context/ProjectIdentity 生成；turn 不能脱离 run；principal/project digest 校验 |
| resources | `grant_refs`、`budget_lease_id`、`work_packet_id`、read/write roots/denies | 只保留已绑定 refs/路径；列表有界、去重和 NUL 拒绝 |
| environment | `environment_id`、`workspace_revision` | handler/backend 环境标识独立于 action arguments；未知环境不扩大权限 |
| scope dimensions | `permission_scope` + `permission_scope_digest` | 复用 CP-04 ScopeSet；operation/path/namespace/network 只交集，budget/depth 只取更小值 |
| lifecycle limits | `authority_epoch`、`trust_revision`、`data_epoch`、`cancellation_epoch`、`deadline_unix_ms`、`fencing_token` | 必须非零；旧 epoch/fence 不可复用，deadline 只能收紧 |
| action binding | `catalog_digest`、`action_digest`、`scope_digest` | catalog/action drift、scope 内部 digest mismatch fail-closed；不代表 permit 已提交 |

ExecutionScope 带 `kiana.execution-scope.v1`、minor-compatible version 和全对象 digest。`scope_digest` 是 scope snapshot digest，`permission_scope_digest` 是内嵌 ScopeSet digest，二者不可混用。

## 3. Derivation and enforcement

ControlPlane 在最终 hook 归一化后：

1. 清除 caller-provided `execution_scope`，避免把公开 DTO 当 authority；
2. 将 Harness 的 server-derived run/turn reference、角色/项目/session、Cell Grant/Budget refs 和 action/context ScopeSet 绑定；direct command 仍有 ExecutionScope，但 `run_id`/`turn_id` 为空；
3. 生成 principal/project identity、environment/roots/limits/epochs 和 action/catalog digests；
4. 把 scope 放进 normalized `CapabilityRequest`，再构造 PreparedAction/pin action；
5. Broker 对 normalized action、scope schema/digest/catalog/action/resource/run/turn 重新校验，handler 只收到通过 scope 的 Authorized request。

Scope 是独立 ExecutionContext 输入，不是 `arguments` 中可以覆盖的 role/project/trust 字段。scope 缺失、空 effective scope、跨 project/session/Cell、root/role/server spoof、digest/epoch/fence mismatch 都在 handler 前拒绝；后续 CP/SC 仍需把真实 authority/lease/approval/retention 状态纳入同一 snapshot。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `empty_effective_scope_never_becomes_workspace_write` | Restricted 空 operation/path scope 不能降级为 `.`/workspace-write |
| `forged_root_role_or_server_scope_is_rejected` | caller/model fields 不能覆盖 server project/role/server scope |
| `scope_intersection_never_grows_under_delegation` | child ExecutionScope 每个维度均为 parent subset，deadline/budget/depth 收紧 |
| `same_scope_reaches_shell_patch_mcp_and_memory` | shell/patch/MCP/Memory 请求共享同一 server-derived scope/digest（后续 integration fixture） |
| `execution_scope_is_server_shaped_and_digest_bound` | typed scope schema、identity/epoch/resource/digest/unknown-field 校验 |
| `execution_scope_is_derived_before_broker_and_cannot_be_caller_minted` | core/request/Broker source guard |

`.github/workflows/cap03-execution-scope.yml` 在 GitHub runner 执行 domain fixtures 与 core guard、fmt/fetch；本地不运行测试。

## 5. 限制与交接

- 当前 authority/trust/data/cancel epoch 在本地 scope 中以受控 snapshot 值接线；完整 authority revision、Grant/Approval/lease ledger、revocation propagation 和 cross-process fencing 由 CP-08+、SC-06+、ER/PD 继续完成。
- ScopeSet 的路径/namespace/network 交集是字符串级 bounded contract；不替代 dirfd/inode/generation、DNS/SSRF、OS sandbox、process supervisor 或 SecretStore。
- Broker scope 校验不等于 permit consumption、handler 已启动、结果已确认或外部业务成功；Unknown/reconcile、receipt/delivery、retention/delete 仍需后续步骤。
- 旧事件/API 缺 execution_scope 仍可 query/read；迁移不能把缺失 scope 的历史事实直接变成可执行 authority。
- 本地只做格式、workspace test-target 静态编译和 diff 检查；GitHub CI 结果不等待，不提升 local_behavior/durable/live/physical。
