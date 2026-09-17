# EQ-06 evaluation/quality protocol baseline

> 快照日期：2026-09-17。本页记录 quality command/event 的 versioned wire registry；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`EQ-06`](../roadmap.md#step-eq-06) |
| feature_status | `implemented`（typed quality command names, request envelope and event registry） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | RequestEnvelope::Command → Daemon/ControlPlane route → registered RuntimeEvent facts |
| this step does | versioned `QualityCommandKind`/`QualityCommandRequest`, six command/event names, strict request validation, quality/eval required-family registry and payload allow-list |
| this step does not | 不实现 quality command handler、EvalStore/ports、experiment runner、feedback/promotion authority 或直接执行模型/provider/Broker；EQ-07+ 负责 |

## 2. Protocol rules

`QualityCommandKind` 固定 `eval.run`、`eval.capture`、`eval.compare`、`quality.feedback`、`quality.promote`、`quality.rollback`；`QualityCommandRequest` 使用 `kiana.quality-command.v1`、typed request ID 和 object arguments，unknown schema/ID/non-object fail-closed。`RequestEnvelope::quality_command` 只构造现有通用 Command body，不自行路由或授权。

RuntimeEvent registry 同时登记六个 versioned quality/eval kinds，required IDs/allowed fields 固定，`eval.`/`quality.` unknown family 不会被当 opaque event 忽略。Schema contract 将 command wire owner 定为 `kiana-protocol`；EventLog/ControlPlane 仍是事实来源，quality command/event 本身不能跳过 authority/approval 或触发第二执行循环。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `quality_protocol_has_no_unversioned_events` | 六个 command/event 名称均有 registry/schema/payload contract，future quality kind 拒绝 |
| `quality_command_is_a_typed_normal_route_and_rejects_bad_envelopes` | generic Command route 构造、unknown schema、非 object arguments 拒绝 |
| `quality_protocol_is_versioned_and_does_not_bypass_control_plane` | protocol/event registry source guard 无 tokio/Broker bypass |

`.github/workflows/eq06-quality-protocol.yml` 在 GitHub runner 执行 protocol fixture、core source guard、fmt 和 domain/protocol/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 目前只有 wire contract/registry，`eval.run` 等命令没有实现质量操作 handler；未知/未授权命令仍须由 Daemon/ControlPlane 拒绝。
- Event registry 登记不证明事件已经由生产路径写入或可 replay；source/owner/evidence、EvalStore、TraceNormalizer、ports、isolated runner、feedback/promotion 仍由后续步骤负责。
- `QualityCommandRequest` arguments 是 JSON object contract，不包含 operation-specific secret filtering；具体 DTO/FixtureStore/Secret policy 继续使用 domain/ControlPlane 边界。
