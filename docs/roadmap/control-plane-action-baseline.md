# CP-03 action catalog and server-owned action baseline

> 快照日期：2026-09-16。本文记录 CP-03 的 action descriptor、PreparedAction、参数归一化和
> 服务端执行元数据边界；运行时夹具只在 GitHub Actions 执行，本地不运行测试。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CP-03`](control-plane.md#step-cp-03) |
| source snapshot | `2e3b7f6`（P0-G-04 CI gate 后的干净基线） |
| feature_status | `implemented`（闭合 action catalog、PreparedAction、normalize/digest source） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub CI 才运行夹具 |
| canonical path | Runner/tool intent → ControlPlane `prepare_capability_action` → descriptor/schema/risk normalization → policy/gate/approval → EventLog action digest → Broker permit/handler |
| this step does | 为注册 operation 固定 capability/risk/schema/resource/effect/cancel/reconcile/idempotency，拒绝未知/不完整 descriptor，生成不可变 PreparedAction 和 catalog/action digest |
| this step does not | 不让模型自报 risk 产生权限，不把 PreparedAction 当 permit，不增加模型可见工具，不在 UI、Provider、Handler 或 direct Company command 另起目录 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Action catalog/normalizer | `kiana-domain/src/actions.rs`, `kiana-domain/src/tool_catalog.rs` | `cd170e01e581015c917fdc0ac2c419149349a5de88164c9f7369c07eecd0a020`, `9c46d8713d6ce55a8eca707179e566036111b2da285539693aac11426dd480ac` |
| Schema/domain exports | `kiana-domain/src/contracts.rs`, `kiana-domain/src/lib.rs` | `5422c0b111578fe872882a9ab56943840e2e0f528adb94212aafdb0f80c72ce1`, `9ca22a4486a8eba0b9b2647d2d6eb4bc3f0aa8607532f7fc8ff139b10b2984a5` |
| Core preparation/dispatch | `kiana-core/src/approvals.rs`, `kiana-core/src/capabilities.rs`, `kiana-core/src/dispatch.rs`, `kiana-core/src/events.rs` | `439a602ee0463d1be33ce144d787e5b7b6cbdb19db7277de2da248aa0cf28b1e`, `0b6a94174b25b4e78069488a73e3226c80c88646be46ed41a99339154134d816`, `464b68ec143743580a201b01d26e7ca8d4f1dc542acb4116edb228a885b65cd0`, `5a0348f5e1940363119d920244724428af1e1373f692f6424ccfd3b8a6dfe26e` |
| Runner/daemon/protocol | `kiana-runner/src/tools.rs`, `kiana-daemon/src/harness_capabilities.rs`, `kiana-protocol/src/lib.rs` | `2a757b5ba06622622d949cccacbf50f4caf6d38b04c8806611853e6f4498d213`, `342dcbca27709f41821872c60ab199595f51ebc6fa9dccabbaf0b040ae5c46ce`, `46ca3fb7dce1a9c8e7cf97d07cedaac1c17817475b5986b191c1253d8260cd2f` |
| Fixtures/guard/workflow | `kiana-domain/tests/cp03_action_contract.rs`, `kiana-core/tests/cp03_action_guard.rs`, `.github/workflows/cp03-action-contract.yml` | `86c37f17fca8489d4818ab29f31fdaa3db62aa7fd331744882f28caf046fcaf7`, `01e05a5d87507d31ecbc23a2dfe90a9a516301aa16116431e78ecdcc0ca076ff`, `ed580f3d49cafec2fc45acdf0235ed4eb939f99845f6d5a108752ee00ce6cef5` |

hash 是当前源码静态锚点；后续修改 catalog、schema validator、prepare 或 handler binding 必须刷新，不构成授权或外部效果证明。

## 2. Server-owned action catalog

`kiana-domain/src/actions.rs` 的 `ACTION_OPERATIONS` 是闭合的产品 operation 集合。每个条目必须有：

| 元数据 | 作用 |
|---|---|
| `capability` / `minimum_risk` | 固定资源类别和最低效果风险；请求 risk 只能相等或更严格，不能降级 |
| `argument_schema` / `result_schema` | 参数与结果的 bounded JSON Schema；`additionalProperties` 明确出现在 schema 中 |
| `resource_fields` | 参与 scope/path/server 绑定的字段；空值或重复资源名使 catalog 无效 |
| `effect` | `read`、workspace patch、sandbox process、external tool 等效果分类 |
| `cancellation` / `reconciliation` | 停止确认与 Unknown 处理要求；不能用普通 IO error 覆盖 |
| `idempotency` | permit 一次消费和禁止无证据自动重试的策略描述 |
| `binding_version` | handler 路由版本；catalog 或实现版本漂移使旧 PreparedAction 失效 |

`validate_action_catalog()` 在准备动作前检查 catalog 非空、operation 唯一、descriptor 完整、资源字段有界、参数/结果 schema 可解析且 additionalProperties 已显式声明。未知 operation 只返回结构化拒绝，不回退为普通 read。

当前 catalog 同时登记五个模型工具（`shell`、`apply_patch`、`mcp`、`memory.search`、`memory.write`）和 direct/operator-only operations。operator-only 条目不会因为存在于 catalog 就暴露给模型；模型可见面仍由 Runner 的固定 schema 决定。

## 3. PreparedAction 与归一化

`prepare_capability_action` 依次：

1. 验证 arguments 为 bounded object、canonical operation 和模型可见性；
2. 移除 caller 不能提供的 grant/permit/authority 字段，按 DaemonHost/ControlPlane context 覆写 role、department、session、project、trust 和 path intersection；
3. 补 `workdir`/sandbox 等默认值，规范化相对路径、MCP alias、timeout 和数值边界；
4. 运行 pre-tool hook 后重新归一化并重算 action；
5. 绑定 Cell/Grant/Budget（若有），构造 `PreparedAction`，冻结 `catalog_digest` 和 `action_digest`，再 pin action authority。

PreparedAction 的私有字段阻止调用者原地改写；`validate()` 会重跑同一归一化、比较当前 catalog digest、规范化请求和 action digest。它是 policy/approval/handler 共享的动作快照，不是授权凭证；只有 ControlPlane 提交的 DispatchPermit 才能交给 Broker。

## 4. 参数、风险和输入边界

| 输入 | 服务端规则 | 失败 reason |
|---|---|---|
| 未知 operation/工具 | 只接受闭合 catalog；Runner 只能看到五个固定模型工具 | `action_operation_unknown` / `action_not_model_visible` |
| capability kind | 与 canonical operation 精确匹配 | `action_capability_mismatch` |
| risk | 由 descriptor/参数（sandbox、action、binding）计算 minimum；ReadOnly 不能伪装写/外部效果 | `action_risk_downgrade` |
| JSON duplicate/非有限数/超限 | 版本化入口使用 `parse_bounded_json`；复杂度、深度、bytes、数组/对象数量有界 | `json_duplicate_key` / `json_number_invalid` / `action_json_limit` |
| path/workdir/sandbox | canonical relative path、角色/项目 path intersection、已授权 sandbox；不能通过 `..`、NUL、别名或 caller field 扩权 | `action_path_invalid` / `action_workdir_invalid` / `sandbox_unsupported` |
| MCP alias/tool arguments | `tool` 与 `tool_name` 不得冲突；arguments 必须 object；server/tool 仍需 daemon trust/schema | `action_tool_alias_conflict` / `action_tool_arguments_object_required` |
| timeout/limit/rows/cols | 零值和非法数拒绝，timeout 有 60 秒上限；不扩大 run/approval deadline | `action_numeric_argument_invalid` |

`additionalProperties` 由每个 descriptor schema 显式决定。当前兼容 schema 对 server-stamped metadata 保持允许，但 handler、policy 和 action digest 仍只消费 canonical normalized request；未来收紧必须升级 catalog/binding 版本，不能静默改变旧批准动作。

## 5. 共享 digest 与执行链

```text
raw intent
  -> normalized CapabilityRequest
  -> PreparedAction(catalog_digest, action_digest)
  -> policy/gate/approval exact request hash
  -> EventLog action pin / DispatchPermit
  -> Broker verify permit + same action digest
  -> handler result / receipt projection
```

Preview、审批卡、policy decision、EventLog action pin 和 handler 输入都必须来自同一个 PreparedAction/request digest；UI preview 或模型输出不能直接成为执行参数。Broker 重新验证 project identity、authority versions、permit expiry、request ID 和 action digest，catalog drift 或 action 改写会 fail-closed。

Direct Company/context command 保持显式 Command scope；没有 Run/Turn 时不生成 Harness identity，也不因 catalog descriptor 存在而直接执行。Company 业务链仍需其自己的 ControlPlane command、review/acceptance/delivery 事实。

## 6. CI-only fixture catalog

| Fixture | Purpose |
|---|---|
| `action_catalog_has_complete_server_owned_descriptors` | 所有 operation descriptor/schema/resource/effect 元数据完整且 schema 边界明确 |
| `forged_readonly_risk_cannot_downgrade_registered_effect` | apply_patch、mcp.call、memory.write 的 ReadOnly 伪造在 Broker 前拒绝 |
| `cp_preview_and_execution_use_identical_action` | 默认值、canonical operation、catalog/action digest 和 immutable snapshot |
| `duplicate_json_and_invalid_numeric_arguments_fail_closed` | duplicate JSON key 与 zero timeout 拒绝 |
| `action_catalog_preparation_and_execution_share_server_metadata` | core/domain/runner/daemon source guard，确保 preview/preparation/dispatch 共用动作值 |

`.github/workflows/cp03-action-contract.yml` 在 GitHub runner 执行 domain fixtures、core source guard、协议/动作相关格式与 `cargo fetch --locked`。本地不运行测试；静态编译通过不提升为 local_behavior/durable/live/physical。

## 7. 限制与交接

- descriptor/catalog 是源码闭合合同，不是持久 ToolSnapshot、签名 manifest 或跨进程可信执行器证明；后续 CAP-01+、CP-04/05、ER/PD/SC 负责持久 binding、scope 交集、permit/CAS 和 evidence。
- 参数 schema 当前为有界子集；不宣称完整 JSON Schema、所有 MCP outputSchema、任意编码 secret、bind-mount/TOCTOU、provider/connector 外部效果或物理安全。
- `PreparedAction` 不授予权限；Grant、Approval、authority epoch、预算、lease 和 sandbox 仍必须由 ControlPlane/Broker 重新核验。
- direct Company、Memory、MCP 和 context operation 的 handler 仍各自有适配器边界；本步骤只保证它们共享 catalog/normalize/digest 入口，不宣称三条执行路径行为已完全等价。
- CI 结果按用户要求不等待，历史测试回执仍只绑定其源码快照；任何后续修改 `actions.rs`、`tool_catalog.rs` 或 handler binding 都必须刷新 catalog digest 和本基线 hash。
