# P0-A-02 stable error code baseline

> 快照日期：2026-09-16。本文记录 CapabilityErrorCode、failure_code/failure_policy 与 CLI/HTTP/retry/reconciliation 映射；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P0-A-02`](../roadmap.md#step-p0-a-02) |
| source snapshot | `2e61ded`（P0-A-01b parent）加本步 evidence/fixtures；最终 commit 记录在 git history |
| feature_status | `implemented`（existing stable error enum/policy mapping verified and CI-wired） |
| proof_level | `source`；静态编译与远程 fixtures 不提升为 local_behavior/durable/live/physical |
| authority path | CapabilityResult.failure_code → CapabilityErrorCode::policy → protocol ResponseEnvelope.failure_policy → CLI/HTTP presentation; ResultUnknown enters reconciliation only |
| this step does | 为现有 append-only CapabilityErrorCode/CapabilityErrorPolicy 和 `CapabilityResult::failure_code()` 建立专用 acceptance fixture/source guard；固定每码 CLI exit、HTTP status、retry、new authorization、compensation、reconciliation 语义，unknown reason 保守归类 |
| this step does not | 错误分类不授权重试、不执行补偿、不替代 ControlPlane 状态；入口展示仍必须服从 response status/error，低层 diagnostic 字符串不创建新权限 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain error contract | `kiana-domain/src/errors.rs`, `kiana-domain/src/capabilities.rs` | `96de29f035a0ff4f3591e9ffbd3873a5c00d2ef485a4c225910b1f2de150f827`, `b1d3474fd7f34516b21082d6965681fe7a1b6f515cd8e407b0ec78ce281f847b` |
| Protocol/entrypoint mapping | `kiana-protocol/src/lib.rs`, `kiana-entrypoints/src/command_dispatch.rs`, `kiana-entrypoints/src/web.rs` | `a5efad3bbff0cd67f82ad79b4d137dfec3424cc1c6f3969ba9eaacc356cc260c`, `c78480c008785cfa509aabca24c640be1ee77468d333cf055e837ebb4e620dc5`, `9c6b356111a93d87e595cbc4a234f5b9352591d7efc2bbe00948b786e62c09ad` |
| Fixtures and CI | `kiana-domain/tests/p0_a02_error_codes.rs`, `kiana-core/tests/p0_a02_error_codes_guard.rs`, `.github/workflows/p0-a02-error-codes.yml` | `a779d8dfb2049eb6a99f7840790ed0a3d96321caf60eeb784337c5f0452a52ac`, `e83ec717ea08ee74bfa02e1b92c4c9872b367c30f677d8bf8af7081230429db5`, `06047846d4b072729c7b891c2d4ed4fcd5684f7e371893cb206c296f36256cdd` |

## 2. Error policy

`CapabilityErrorCode` 是 append-only wire enum；每个码的 `CapabilityErrorPolicy` 固定 CLI exit、HTTP status、是否可重试、是否需要新授权/补偿/对账。当前所有码均不允许自动 retry；`ResultUnknown` 与 `CompensationRequired` 强制 reconciliation，不能把未知现实效果当作失败后可安全重跑。

`CapabilityResult::failure_code` 优先读取受控 `error_code`，否则只把稳定 reason 前缀映射到 enum；未知 reason 保守落到 `ExecutionFailed`。`ResponseEnvelope.failure_code/failure_policy` 对 `ResultUnknown`、Cancelled、AwaitingApproval 和 blocked/denied 做状态优先映射，CLI/HTTP 只呈现统一状态，不在 UI/adapter 里另造错误分类。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `path_escape_and_result_unknown_use_stable_non_retryable_policies` | path_escape 映射 403/exit3/new auth，ResultUnknown 映射 reconciliation 且不可 retry |
| `unknown_reason_keeps_conservative_execution_failed_classification` | unknown diagnostic 不提升权限或 retry，port_conflict 仍保留 Conflict |
| `stable_error_mapping_is_shared_by_domain_protocol_and_entrypoints` | domain/protocol/CLI/HTTP 共用同一错误 policy 边界 |

`.github/workflows/p0-a02-error-codes.yml` 在 GitHub runner 执行 domain error fixtures、core source guard、fmt 和 domain/protocol/core/entrypoints test-target compile；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 现有 `from_reason` 仍保留历史字符串兼容映射；新增错误码必须追加 enum、policy、protocol/入口映射和 fixture，不能修改既有语义。
- policy 只分类错误，不证明真实 effect、外部服务结果或 reconciliation 已完成；这些由 CAP/ER/PD/INT/DEP 逐步提供证据。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
