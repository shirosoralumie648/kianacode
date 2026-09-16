# UI-02 unified error, capability and surface handshake baseline

> 快照日期：2026-09-16。本文记录 UI-02 的 typed initialize/health handshake、能力交集和稳定错误映射；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`UI-02`](ui-entrypoints.md#step-ui-02) |
| source snapshot | `21d2b04`（UI-01 versioned DTO 提交后的干净基线） |
| feature_status | `implemented`（protocol/client handshake and error/capability source） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | surface client → typed UiHandshakeRequest → DaemonHost/ControlPlane command → typed response/error/capability intersection |
| this step does | 提供 UiHandshakeRequest/Response、UiHealth、UiSurface、requested capability 和 `intersect_ui_capabilities`；将 Input/PolicyDenied/Approval/Conflict/Capacity/Unavailable/Cancelled/Failed/Unknown/Persistence 映射到 UiError/retry disposition；`KianaClient::initialize`/`health` 在传输前后做 schema/typed validation；固定错误文案不回显内部路径/token |
| this step does not | 不改变 daemon 业务命令路由或创建实例 transport，不把 capability projection 当授权，不自动 retry Unknown/Denied，不把 health 变成 liveness 事实；UI-03 负责 instance/transport，UI-04+ 负责 action journal/projector |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Handshake/error/capability DTO | `kiana-protocol/src/ui_contracts.rs` | `8efcb78110a14baf43052641b6f159274c819ff70c48639dec126b96a6089002` |
| Protocol schema registry | `kiana-domain/src/contracts.rs` | `97f8fc237565dd9177537b0dd3b366bab757687478e60eab895b031322f720fa` |
| Typed client facade | `kiana-client/src/lib.rs` | `b4b9c59ae2df8a2e6d86c5692160aff509b3d335c827813c9ecc3c4abce5d4b2` |
| Fixtures | `kiana-protocol/tests/ui02_handshake.rs`, `kiana-client/tests/ui02_handshake.rs`, `.github/workflows/ui02-handshake.yml` | `63c8ae507cff3f548d484387c61587202c97f1f16a36cfd081be0ad5127964ae`, `edf69aedee70aff6a97fd0154f33e0857e286f362a5a45eed546a9efc37aa7f8`, `bdb9f4617d723e2b198e176d9131319d758a0be846a98d94ea42626973c188b7` |

hash 只用于 UI-02 源码漂移复核，不构成真实 daemon handshake、transport authentication、权限授予、持久 projector 或业务结果证明。

## 2. Handshake contract

`UiHandshakeRequest` 绑定 client version、surface、requested capability IDs/feature versions，以及可选 known instance/authority epoch。请求字段重复、未知 schema、空/超限 feature 或 epoch=0 直接拒绝。`UiHandshakeResponse` 返回 server version、opaque instance ID、authority epoch、能力摘要和 limitations；能力 ID 在响应中唯一，scope digest 必须有效。

`intersect_ui_capabilities` 将 server-derived principal capabilities 与 surface capabilities 按 capability ID 求交，client requested list 只能进一步收窄；scope digest 不同、任一侧 disabled 或任一侧缺失都生成 disabled capability/reason，不能提升 enabled/actions。最终命令仍需 ControlPlane/Broker 重新检查 owner/scope/policy/gate/approval。

## 3. Stable errors and health

`stable_error_from_response` 将现有 `CapabilityErrorCode`/ExecutionStatus 映射到 UI 的封闭错误 union，Unknown 始终给出 `QueryOriginal`，PolicyDenied/Conflict/Cancelled/Failed 不给安全重试。错误 message 使用固定用户文案，不复制 response.error 的路径、token、provider detail 或内部异常。`UiHealth` 只返回 instance/epoch/status/capability IDs/limitations，不暴露 folder、socket、secret 或 raw failure。

`KianaClient::initialize` 在发送前验证 handshake，在响应非 Completed 或 typed decode/validate 失败时返回 `ClientError::Protocol`；`health` 同样只接受有效 UiHealth。Client facade 只传输请求，不在本地执行、授权或重试。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `handshake_capabilities_are_the_principal_surface_intersection` | principal/surface scope 或 enabled 不一致时能力被禁用，不能通过 client request 扩大 |
| `handshake_and_health_are_versioned_and_unknown_fields_fail_closed` | handshake/health schema 与 unknown field 边界严格 |
| `stable_error_mapping_keeps_policy_denial_and_unknown_distinct` | PolicyDenied 与 Unknown 使用不同 code/retry，Unknown 只 QueryOriginal |
| `client_initialize_and_health_return_typed_handshake_data` | typed client 只接受有效服务端 DTO |
| `client_rejects_invalid_handshake_before_transport` | 客户端在发送前拒绝错误 schema |

`.github/workflows/ui02-handshake.yml` 在 GitHub runner 执行 protocol/client handshake fixtures 和 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 5. 限制与交接

- 当前 `ui.initialize`/`ui.health` 是 typed client command facade，daemon 尚未实现独立 handshake route/instance discovery；UI-03 才接真实 in-process/Unix socket/named pipe 与 epoch/peer validation。
- 能力交集只处理已由服务端产生的摘要和 scope digest，不能替代 ControlPlane grant/policy；UI 按钮隐藏不等于服务端拒绝，直接构造请求仍需 UI-04+ / core gate。
- Error message 已做固定文案，但 `ResponseEnvelope` 旧错误字段仍由旧兼容面承载，不能把其原文直接展示给用户；health/failure projection 仍需 daemon redaction/health contract。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
