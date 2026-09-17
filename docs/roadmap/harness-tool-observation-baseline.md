# H11 Tool Observation 基线

> 快照日期：2026-09-18。本页记录能力结果到模型可见 observation 的分类、脱敏和修复边界；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H11`](harness.md#step-h11) |
| feature_status | `implemented`（typed ToolObservation + runner feedback gate） |
| proof_level | `source`；本地仅做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | CapabilityResult dimensions/error policy and ControlPlane decide lifecycle; observation is data-only model context |
| this step does | succeeded/failed_known/denied/cancelled_not_started/unknown/pending classification、调用身份/exit code/摘要/引用/repair class、bounded redaction、unknown/deny/cancel no-retry |
| this step does not | 不把 observation 当 grant/permission，不把 unknown effect 自动重试，不把模型修复建议直接执行，不宣称外部副作用已回滚 |

## 1. Contract

`ToolObservation::from_result` 依据 `CapabilityResult::dimensions` 与共享
`CapabilityErrorCode::policy` 生成严格 `kiana.tool-observation.v1`。成功、已知失败、拒绝、
未启动取消和未知结果使用闭合 status；只有 `ExecutionFailed`/`InvalidArguments` 等已确认错误
可标记 `ModelRepair`。摘要经脱敏并限制 8 KiB，保留 output digest 和可选 evidence ref，且
`untrusted=true` 始终存在。

Runner 对 unknown/denied/cancelled_not_started 直接收口，不把同一意图再次送入模型。已确认
命令非零/patch 类失败才以 Tool role 携带 observation 进入下一步；模型收到的 JSON 没有任何
grant、permission 或 execution scope 字段，下一次 capability 仍须完整经过 ControlPlane/Broker。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `unknown_result_never_triggers_automatic_retry` | unknown effect 不调用第二次模型 |
| `tool_output_cannot_grant_permissions` | 外部输出只作为 `untrusted` Tool observation，不产生权限字段 |
| `failed_test_observation_allows_bounded_model_fix` | 已知失败 observation 可推动一次后续模型修复 |
| `tool_observation_is_bounded_and_typed` | status/repair、摘要上限和 schema 受严格校验 |
| `h11_tool_results_are_classified_before_model_feedback` | source guard 固定分类、no-retry 和数据边界 |

## 3. Proof ceiling and handoff

H11 proof ceiling 为 `source`：统一 observation 合同和 runner gate 已接入，CI-only fixtures/source
guard 固化 failure-first 路径。完整串行批次、Invocation 持久结果、审批恢复、自动修复策略、
provider/外部 effect 对账与 live/physical proof 仍留待 H12+ / CP/PD/INT。
