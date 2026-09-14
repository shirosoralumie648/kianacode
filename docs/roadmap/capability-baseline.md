# CAP-00 Capability 可复核基线

> 快照日期：2026-09-14。本文是 `CAP-00` 的 source-only 基线，不是运行时验收，也不改变产品行为。
> 本轮不在本地运行测试；行为测试由后续 GitHub CI 执行。类型、源码测试名称和静态检查不能提升为 `local_behavior`、`durable`、`live` 或 `physical` 证明。

## 1. 快照与范围

| 项目 | 记录 |
|---|---|
| source snapshot | `b49cd62772943aa117c8ea4adec383580f739237` |
| worktree 基线 | `master`，ER-00 已推送；本文件及关联账本变更前源码工作树干净 |
| 目标 | 对照 `DaemonHost → ControlPlane → Broker → Handler` 固定 capability 的实际接线、缺口和后续 owner |
| 本轮范围 | registry、scope/authority、cancel/stop、patch、MCP、memory 六条链；模型可见工具集合；timeout 分层；失败/拒绝边界 |
| 本轮不做 | 不扩展模型工具、不打开 HTTP MCP、不实现统一执行合同、不修改 handler、policy、sandbox 或 EventStore |

相关源码的快照 hash（用于后续漂移复核）：

| 边界 | 文件 | SHA-256 |
|---|---|---|
| 模型目录 | `kiana-domain/src/tool_catalog.rs` | `9c46d8713d6ce55a8eca707179e566036111b2da285539693aac11426dd480ac` |
| action descriptor | `kiana-domain/src/actions.rs` | `47b596ea33e36ecedd6fdc087b84e2e8a008d1509ad30dd5528e53b957ee46c9` |
| capability 值对象 | `kiana-domain/src/capabilities.rs` | `b488bb89905c09776876ca5248620cac2bb412fd8a8b86c21e0d8282d7897de7` |
| runner mapping | `kiana-runner/src/tools.rs` | `2a757b5ba06622622d949cccacbf50f4caf6d38b04c8806611853e6f4498d213` |
| core capability admission | `kiana-core/src/capabilities.rs` | `3409701ee141c1c9dd8e6df90e7eeb47efa9f02f3202c25cdee541c26cb5b39d` |
| identity stamping | `kiana-core/src/events.rs` | `2f5da5345ae1a2f738b976f283eace1f5e012b8c442e044b6935504b6c83f616` |
| session cancellation | `kiana-core/src/sessions.rs` | `809a40c13768b6f02b5c64c00a2077af59ed98de869190b19dba0c17a3304415` |
| broker | `kiana-capability-broker/src/lib.rs` | `9f68ef808f0370403a5f8e9b91fc643c339bbf7405326c6fc4407bdb23612b1c` |
| sandbox | `kiana-daemon/src/harness_sandbox.rs` | `7e06c97ecf88015cf664afcaa3b126bd6a6d0413703c905b91005a3f4a44bc69` |
| shell adapter | `kiana-daemon/src/harness_capabilities.rs` | `3494dfa79deff338e6372fd28f974899d556aa1f08a2c75f6d539f399aae3ecb` |
| patch adapter | `kiana-daemon/src/apply_patch.rs` | `a0fad66d0ae27250cf6b5c1d4bde1c1298d150aa8eb43964c791a410a785581e` |
| MCP adapter | `kiana-daemon/src/harness_mcp.rs` | `9e7a27090750a3524af01f2e036fe3ed3d30e58aedb706a7d5837133957fa5d4` |
| MCP stdio | `kiana-daemon/src/mcp_stdio.rs` | `e2bb6713f4b28ceab1b7f0fd189abd4f647dcf161a9d8407af1c38b60dc0f739` |
| pre-tool hook | `kiana-daemon/src/pre_tool_hooks.rs` | `2e34e24bfbb1c1a71a438b1a8cc018ad1aeb5b67c2f6166c120a2b95637f61a6` |
| memory adapter | `kiana-daemon/src/harness_memory.rs` | `6379bfb054a745f07f9977cf63122107f97e3753b5e15bcfc90217d8518b953c` |

## 2. 现有主链与单一事实源

1. `DaemonHost` 组装入口、`ControlPlane` 做身份/策略/审批/生命周期判断；`CapabilityBrokerPort` 只接受 `AuthorizedCapabilityRequest`，不能被 UI、workflow 或模型直接调用。
2. `kiana-runner/src/tools.rs` 将模型调用映射为 `CapabilityRequest`。当前模型 schema 只有 `shell`、`apply_patch`、`mcp`、`memory.search`、`memory.write` 五项；别名只用于规范化，不能生成第二套执行循环。
3. `kiana-domain/src/actions.rs` 是服务端闭合 action catalog，包含 operation、binding version、risk、schema、effect、cancellation、reconciliation 和 idempotency 元数据。`PreparedAction` 是准备结果，不是派发许可。
4. `kiana-core/src/capabilities.rs` 已有 normalize、policy/gate/hook、Cell lease 与结果关联；`claim_invocation`/dispatch fence 和统一结果合同仍由 CP/CAP 后续卡补齐。
5. Broker 已做 `(CapabilityKind, operation)` 精确查找、重复注册拒绝、catalog seal、扩展准入和 permit verifier 接口；仍需把 prepare/authorize/dispatch/finalize 的事实边界与 adapter 停止报告统一起来。
6. daemon 已有 shell、apply_patch、stdio MCP、memory、hook 和 Linux bwrap 适配器。它们的局部拒绝语义应被保留，但不能从局部类型或历史测试推断跨适配器原子性、取消确认或现实效果。

模型可见集合与服务端 operator-only operation 必须分开。`mcp.discover`、`workspace.transaction`、`local.package`、execution/process controls、`environment.inspect` 和 `tool.search` 等不应因 catalog 存在而自动暴露给模型。HTTP MCP 当前明确返回 unsupported，`CAP-29` 只是后续 target；本基线不打开它。

## 3. 六条调用链

| 调用链 | 当前已接线事实 | 当前缺口 / 未证明 | 后续 owner |
|---|---|---|---|
| registry | tool catalog、closed action catalog、Broker 精确 key、重复注册拒绝、binding version 字段已存在 | schema、policy、binding、handler 和 result contract 尚未由一个装配期一致性检查统一；扩展 binding 的持久准入未闭环 | `CAP-01`、`CAP-02`、`CAP-05`；CP-03/05 |
| scope / authority | core 服务器覆盖身份并计算角色/packet/path 交集；Broker 接受已授权请求；ProjectTrust 在 daemon 侧存在 | authority 仍部分藏在 JSON arguments；资源/server/memory/network scope、epoch、deadline、写集和审批绑定尚未成为不可变共同输入 | `CAP-03`、`CAP-05`、CP-04/05；H09/H11 |
| cancel / stop | run cancellation tracker、shell timeout/进程组停止、MCP/Hook 的局部停止入口存在 | 多 in-flight execution 集合、阻塞任务 supervisor、停止确认、quarantine/fence、result_unknown 和跨重启恢复尚未统一 | `CAP-12`、`CAP-17`、`CAP-24`；CP-15/18；H17 |
| patch / filesystem | pre-plan、project lock、precondition、symlink/hardlink reject、dirfd/openat/renameat 和 rollback 路径存在 | core 授权写集、缺失路径语义、提交点取消、跨文件原子性和恢复 journal 未由共同 invocation 证明 | `CAP-08`、`CAP-10`、`CAP-15`、`CAP-16`；ER-24/PD |
| MCP | stdio-only planner 使用 bwrap；帧、通知、输入/结果基本 schema、server/tool pinning 和信任搜索路径有局部校验 | outputSchema、分页/背压、配置与 schema 在审批前绑定、drift、取消确认、池隔离和远端效果对账缺失；HTTP 仍 unsupported | `CAP-20`–`CAP-22`、`CAP-29`；CP-25 |
| memory | ACL、collection path、candidate/review、model write 的 `origin=model` + `candidate/draft` 及审批前不可检索已接线 | `spawn_blocking` 取消时的真实写入停止、Memory scope 与工作区 scope 的共同摘要、跨重启 candidate/review 事实尚未闭环 | `CAP-19`；CM-01/02；ER/PD |

## 4. 时间、输出和失败口径

现有超时是分层事实，不是一个已经统一的全局预算：

| 层 | 当前来源 | 口径 |
|---|---|---|
| run | Runner/Daemon `RuntimeConfig` 的 wall-time 与 max-steps | 限制模型循环；耗尽不等于已取消每个 in-flight effect |
| action | capability normalize 的 `timeout_ms` 上限（当前有 60 秒边界） | 规范化输入；不能扩大 run/approval deadline |
| shell | argv/`sh -c`、进程组停止、stdout/stderr 有界读取 | `timed_out` 和 exit=124 是进程事实；效果是否确认由 invocation 决定 |
| MCP | stdio frame/read/write/operation 限制 | 响应丢失或取消不能推断远端没有副作用 |
| hook | 每命令及整体 hook 时间边界 | hook 仍需纳入同一 supervisor 和 run cancel 集合 |

统一合同完成前，任何写操作 timeout、取消、transport disconnect、结果落盘失败都不能直接投影为成功或“确定未执行”；无法确认的效果保持 `result_unknown`，并等待后续 reconciliation。失败分类至少保留：unknown tool、invalid arguments、unknown/unsealed operation、missing binding、permit missing/rejected/expired、scope intersection empty、untrusted project/extension、unsupported backend/transport、handler/IO error、stop unconfirmed 和 result unknown。

## 5. 源码验收索引（不代表本轮已运行）

以下是当前源码中可定位的测试入口，作为 GitHub CI 的后续矩阵；本轮只做 source index 和静态检查：

| 区域 | 源码测试计数 | 代表性负向断言 |
|---|---:|---|
| core ControlPlane | 106 | `malicious_unknown_tool_is_denied_before_the_broker`、`forged_low_risk_mcp_call_is_denied_before_broker`、`incomplete_cell_capability_scope_is_rejected_before_broker`、取消与 result-unknown 路径 |
| runner tools | 14 | 五工具 schema、非法参数、shell/patch/MCP/memory capability kind 与未知工具拒绝 |
| Broker | 2 | `unregistered_capability_fails_closed`、`static_registration_rejects_duplicate_handler_keys` |
| shell adapter | 10 | timeout、后代进程停止、输出上限 |
| sandbox | 5 | danger full access、cap-drop/clearenv、workspace bind、只读计划 |
| patch adapter | 21 | parent escape、symlink/hardlink、precondition、rollback |
| MCP adapter | 10 | oversized input、unknown/ambiguous tool、schema/result failure |
| memory adapter | 4 | ACL、路径边界、candidate/review 和存储读取 |
| pre-tool hook | 0 direct `#[test]` | 依赖 daemon/core 集成路径，不能把无 direct test 写成已验证 |

CI 应先跑拒绝路径，再跑成功路径和回归；测试命名是索引，不是本轮回执。尤其需要在 CP/CAP 接口完成后补：重复 dispatch 的 durable CAS、统一 permit、scope 不可扩张、停止确认、patch 事务恢复、MCP drift、memory blocking write cancellation。

## 6. CP / H 交接与退出判断

| 交接 | 本基线固定的共同边界 | 不能在 CAP-00 宣称 |
|---|---|---|
| CP-04/05 ↔ CAP-01/03/05 | `prepare → authorize → dispatch → finalize` 只保留一条主链；CP 拥有 identity/policy/approval/lease/epoch，CAP 拥有 adapter plan/stop/effect evidence | `AuthorizedCapabilityRequest::new` 通过不等于真实授权存在；Broker handler 可调用不等于 permit 可消费 |
| CP-15/18 ↔ CAP-12/17/24 | cancel、stop report、quarantine、Unknown、reconciliation 共享 execution/invocation identity | drop future、PID 日志或某个 handler 返回值不等于停止或现实效果确认 |
| H09/H11/H17 ↔ CAP-01/12/19/20 | Harness 只消费五工具 catalog 和 ControlPlane 结果；长任务/Memory/MCP 的独立 invocation 回到同一主链 | 不新增模型工具，不复制 runner loop，不把 transcript/cache 当事实源 |
| ER/PD ↔ CAP-16/24/25 | EventLog/Receipt 是事实与投影边界；adapter 只提供 evidence，恢复由 ControlPlane 许可 | receipt 存在不等于外部业务 outcome 正确；source snapshot 不等于 durable/live |

CAP-00 的可交付物是本文件、module map 链接、roadmap 状态和 `CURRENT_STATUS.md` 证据块。仅“Capability baseline artifact”可标为 `feature_status=implemented`；产品能力仍分别是 `partial`、`target` 或 `deferred`，本轮 `proof_level=source`（静态检查只作为 source hygiene，不升级行为证明）。每个后续 CAP step 开始前必须重新计算 snapshot/hash 并标注漂移。

