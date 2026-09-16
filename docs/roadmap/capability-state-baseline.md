# CAP-04 typed capability state and outcome baseline

> 快照日期：2026-09-16。本文记录 CAP-04 的 capability 状态、结果维度和稳定错误映射；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CAP-04`](capability.md#step-cap-04) |
| source snapshot | `53d357a`（CAP-03 immutable ExecutionScope 后的干净基线） |
| feature_status | `implemented`（typed state/outcome source + core projection + structured shell result） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | CapabilityRequest → policy/gate → typed CapabilityExecutionState → Broker result dimensions → EventLog/Receipt/Protocol |
| this step does | queued/authorized/dispatching 阶段的取消、启动失败和恢复 Unknown 转移；process/stop/effect 结果维度；稳定 `CapabilityErrorCode`；非零 shell exit 的结构化失败 |
| this step does not | 不承诺外部副作用回滚、exactly-once、真实 provider/connector/业务 Outcome、跨进程 attempt ledger 或 live/physical effect |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain state/result contract | `kiana-domain/src/states.rs`, `kiana-domain/src/capabilities.rs`, `kiana-domain/src/actions.rs` | `46d66d1411a3bc2238f5d272e5243f736cc0bf7425011303e41aefcc282c6793`, `b1d3474fd7f34516b21082d6965681fe7a1b6f515cd8e407b0ec78ce281f847b`, `2fa6aa563fa4cc379de452b05c146304ea8d223e802030432c54537bdb4cb840` |
| Registry/wire mapping | `kiana-domain/src/contracts.rs`, `kiana-protocol/src/lib.rs` | `12581c45ca10a0a6eae4de9c26dca9ddb29e7586db3860538620c192704c3945`, `da9a2cafbd0b4ff559f69ad75552d702d916038d86529e03259ddd334df3402e` |
| Core projection/classification | `kiana-core/src/invocation_projection.rs`, `kiana-core/src/capability_attempt_projection.rs`, `kiana-core/src/events.rs`, `kiana-core/src/lifecycle.rs` | `d0cdb7a5ed44ae9f7e7ab196464787f88049a73f68c742a549b68ba7be62dd01`, `f2bdfc608e574b06f48c00b7fc2867de42b8728853a15e7b5788bd69e1120a86`, `a0d53bdff9fe2ebaf37c0c56fdb2e0a116c29445fb3b5d8d2e21ac07f305d228`, `fb97a2e4aaf55f8974f2948ef298fa825da44fdcb8eb92a47f165022e813a04d` |
| Supporting projections | `kiana-core/src/metrics.rs`, `kiana-core/src/platform.rs`, `kiana-core/src/span_projection.rs`, `kiana-core/src/company_business.rs`, `kiana-core/src/data_governance.rs` | `3d79a190de889fb0fdbfa619d8addbd0d419f521209e436580dbc426aec30f1f`, `c02e3fff626d7b61d014b00fd5c50df10ea54093b13dba73914a8a022feefeb6`, `32c3c10d53322030e31e256a8a10cd79c26762db263ff0fca6a111bf95416008`, `0885fdff073d65f4565ab7fc8c54712d0da8135c14d69fe4a22655bf7c825aaf`, `45074ad8dbe9dfcf93a9df33234a3fb2ff42fda47cf759c26142732b8feb288f` |
| Daemon process result | `kiana-daemon/src/harness_capabilities.rs` | `5bb0de516e723b62eeeddfd62c97b339fb455100f5d29e39951d602267f11ce0` |
| Fixtures/guard/workflow | `kiana-domain/tests/cap04_state.rs`, `kiana-core/tests/cap04_state.rs`, `kiana-core/tests/cap04_state_guard.rs`, `kiana-protocol/tests/cap04_mapping.rs`, `.github/workflows/cap04-state.yml` | `9341f9ed40ba801851d53d538359b09c4659c0c4eec2daf1e941c418a5fae104`, `5704c91bc8d22d87da1e8b689af73bb74590ee6a769a59000a374d5ceb31a7d5`, `cf81a96f26795c896f7fbfa854828e92744a0c7b132fc85d08ce9a298f7afa2f`, `42dd52f32ad980a8da438b71d9064973acdde0872b69a35d562f2a0fadadda8b`, `4678c492bdce0d219436b2610ad1728b7fcb2aa534729bacc4312dac1683befb` |

hash 只用于 CAP-04 源码漂移复核，不构成运行时或外部效果证明。

## 2. Typed state and outcome

`CapabilityExecutionState` 保留单一 invocation 生命周期，并新增 `Queued` 以及 queued、authorized、dispatching 的取消/失败/Unknown 合法边。事件投影调用 `can_transition_via`，把少量压缩事实（policy decision、dispatch result）集中在 domain 合同，不在 adapter 内复制字符串状态机。终态一旦形成不能回到成功或其他终态；重复事实只有相同终态签名才可视为重放。

`CapabilityResultDimensions` 将 handler 结果拆为：

- `process`：`not_started` / `running` / `exited` / `unknown`；
- `stop`：`not_requested` / `requested` / `confirmed` / `unconfirmed` / `unknown`；
- `effect`：`not_started` / `started` / `succeeded` / `failed` / `unknown`；
- `exit_code` 与稳定 `failure_code`。

`CapabilityResult.success` 只表示 handler 对能力调用的成功声明，不是任意子进程退出码的别名。归一化器拒绝“success=true + 非零 exit_code”并产生 `execution_failed:shell_exit`；`failure_code`、协议 status、CLI exit、HTTP status 和 retry/reconciliation policy 都从同一 `CapabilityErrorCode` 派生，原始 error 仅作诊断 detail。

## 3. Projection and attempt fences

- `execution.result_committed` 的 `effect_known=false` 固定映射到 `Unknown`，即使 payload 同时带 `cancelled=true` 也不能投影为 `Cancelled`。
- `CapabilityAttempt` 结果必须先看到同一 `(request_id, attempt)` 的请求事实；未知 attempt 或已绑定的 turn/invocation/execution identity 不一致时返回 `capability_attempt_foreign_result`。
- dispatching/executing 在没有终态事实的恢复投影中降为 `Unknown`，保持 fenced，不会被当作成功或自动重试。
- Event payload 使用 `CapabilityErrorCode`，不再用 `contains("result_unknown")` 之类的诊断文本猜测控制状态。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `terminal_execution_cannot_transition_to_success_again` | 已失败 invocation 后的成功终态被 projection 拒绝 |
| `unknown_effect_cannot_be_projected_as_cancelled` | Unknown effect 与后续 cancellation 冲突，不能降级成已确认取消 |
| `foreign_attempt_result_is_rejected` | 同 request 的未声明 attempt 终态结果 fail-closed |
| `nonzero_shell_exit_remains_a_structured_tool_result` | 非零 exit 保留 exit/output 证据，状态为 failed，错误码稳定且不可自动重试 |
| `recovery_of_dispatching_execution_is_unknown_not_success` | 只有 dispatching 事实的重建结果是 Unknown |

`.github/workflows/cap04-state.yml` 在 GitHub runner 执行 domain/core fixtures、source guard 和 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 5. 限制与交接

- 本步的 attempt identity fence 是事件投影边界，不是持久 permit 消费或 OS/外部系统 exactly-once；CAP-05/CP/ER/PD 继续补齐。
- `CapabilityResultDimensions` 只表达当前 handler 证据；`Unknown` 仍可能有副作用，不能释放预算、路径锁或宣称业务回滚。
- 旧事件缺少 typed fields 时只做保守兼容解析，不把历史缺失字段升级为 authority；完整 upcaster/retention/delete 仍属 ER/PD。
- 当前 shell adapter 已将非零 exit 结构化为失败，但真实进程、MCP、provider、network 和 physical effect 仍未做 live/physical 证明。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
