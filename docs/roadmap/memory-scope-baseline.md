# CM-03 server-derived Memory read/write scope baseline

> 快照日期：2026-09-16。本文记录 CM-03 的 MemoryScope 交集与 ControlPlane/daemon 接线；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CM-03`](context-memory.md#step-cm-03) |
| source snapshot | `961965d`（CM-02 生命周期提交后的干净基线） |
| feature_status | `implemented`（domain intersection + core/daemon enforcement source） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | server RequestContext/Role/ExecutionScope → MemoryScope collection/purpose intersection → memory handler read/write |
| this step does | MemoryScope 只接受 server principal/project/session，集合按 grant/role 交集收紧，write 必须显式 collection/allow；ControlPlane 把 role-granted collections 写入 ExecutionScope，daemon handler 从该 scope 重建并复核 |
| this step does not | 不从模型文本/arguments 生成 principal、project、scope 或 write authority；不建立 processing-grant/retention ledger、Memory mutation transaction 或 durable scope store |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain scope intersection | `kiana-domain/src/context_scope.rs` | `76b4dc08bda0d2e7a880d43c0975d602cdba01b5cc4b0ac79144650a1d98ca52` |
| Core derivation | `kiana-core/src/capabilities.rs` | `cca98e5485845b1e0c07c36456cf0362097ae687a105998913f19f848c75c128` |
| Daemon enforcement | `kiana-daemon/src/harness_memory.rs` | `6050fda6a33e3ab0f1fabc30a23f764c505886da547d6eba32f7d0275ea50a58` |
| Fixtures/workflow | `kiana-domain/tests/cm03_memory_scope.rs`, `kiana-core/tests/cm03_scope_guard.rs`, `.github/workflows/cm03-memory-scope.yml` | `42ccf6e3b62b282ba3820530bc85793ee112cdc511856abde474e05d25e1b005`, `884fe8509ff7446c30bdb6d24dec63c77ce12ac7bdb1be6aaa129814c7a4f8bb`, `40761fa3c70726efc9411910f913d750105024633aa047dc4ce8b6de7016cc95` |

hash 只用于 CM-03 源码漂移复核，不构成 Memory durable、审批或业务结果证明。

## 2. Scope intersection

`MemoryScope::intersect` 要求 principal/project/session/purpose 完全一致；collections 逐对选取更窄的 `MemoryCollection`，无覆盖关系时为空交集并拒绝；`allow_write` 只取逻辑 AND。这样 parent `project` 与 child `project:code` 的结果只能是 `project:code`，任何 context text 或模型声称都不能扩大 scope。Scope digest 覆盖 identity、collections、purpose 和 write bit。

## 3. ControlPlane and handler boundary

`build_execution_scope` 对 `memory.search` 无显式 collection 时只写入 RoleSpec granted collections；显式 collection 先按 role knowledge/write policy 校验，`memory.write` 缺 collection 直接拒绝。生成的 ExecutionScope 由 Broker 继续核验。daemon handler 要求 `request.execution_scope`，通过 `MemoryScope::from_execution_scope` 重建 server principal/project/session/collections；显式 collection 必须被 scope 覆盖，write 不能依赖模型自报字段。存储 HOME/path 仍是单独的 storage boundary，不代表权限。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `read_scope_is_intersection_of_all_grants` | 多个 scope 交集只保留共同/更窄 collection，scope digest 变化可见 |
| `write_scope_cannot_be_widened_by_context_text` | read-only 与 write 请求交集保持 `allow_write=false`，跨 session 直接拒绝 |
| `memory_scope_is_server_derived_before_handler_dispatch` | core/daemon 从 ExecutionScope/Role 派生并复核 scope，未读取 principal 自报字段 |

`.github/workflows/cm03-memory-scope.yml` 在 GitHub runner 执行 domain intersection fixtures、core source guard 和 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 5. 限制与交接

- 目前 Core 的 epoch/Grant/Approval/processing grant 仍是受控 snapshot/已有 role gate；完整 purpose/classification/retention/revocation 和跨项目 user-private 传播需 CM-04+、PD/SC。
- MemoryScope 交集是逻辑 collection 级，不等于文件句柄、索引 generation、content freshness、embedding 或 durable EventStore transaction。
- daemon storage scope 仍保留本地 HOME/path 校验；它只约束存储位置，不能成为第二授权中心。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
