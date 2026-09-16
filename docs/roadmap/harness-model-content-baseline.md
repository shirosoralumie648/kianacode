# H04 structured model content and provider conversion baseline

> 快照日期：2026-09-16。本文记录 H04 的结构化模型消息、工具结果、受控附件引用与 provider 边界；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`H04`](harness.md#step-h04) |
| source snapshot | `6d493c4`（H03 guard 修订后的干净基线） |
| feature_status | `implemented`（typed content source + provider pre-wire guard） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | ModelMessage/ModelOutput structured content → history validation → ProviderGateway request compiler → provider-specific wire body |
| this step does | 增加 `ModelContent`（文本/工具调用/工具结果/AttachmentRef/ProviderOpaque）、ProviderContinuation、legacy text/tool_calls 兼容转换、orphan/非法 ref 校验、provider/route/version 绑定和请求发出前 unsupported/cross-provider 拒绝 |
| this step does not | 不发送真实附件 bytes、不解析或展示 opaque 内部 payload、不跨模型迁移 continuation、不改变 Broker/ControlPlane 授权、不宣称 live provider 或外部 effect |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain content contract | `kiana-domain/src/model.rs`, `kiana-domain/src/contracts.rs` | `85034cccbb921158ad70b6ad2167b845c3f1d52447597be762b7136b8d99025e`, `39b3b42c80eda6f4269b08f188591e60b658332dc526293d3de44d4d700a473a` |
| Provider conversion boundary | `kiana-provider/src/request.rs`, `kiana-provider/src/response.rs`, `kiana-daemon/src/model_client.rs` | `ec5a05bf3fd136bdbfbc4bab8351dc3a807c2f67ecacf072f89e7a01cab75471`, `06b416a096671fee2637b5c3c782a7e9042f21a23c7f27fd80b85db6a3dbbe66`, `7f3d435f44dbf7c48f3cfd6dd2663e1d88c7e0f9b92744d7d8de673ab8bfa00a` |
| Runner compatibility | `kiana-runner/src/model.rs` | `458f46de8d8a320e1987ea7b5a47bd87faa053143898b843c5d1493c9275b7ff` |
| Fixtures/workflow | `kiana-domain/tests/h04_model_content.rs`, `kiana-provider/tests/h04_model_content_guard.rs`, `.github/workflows/h04-model-content.yml` | `3d25af2300a137aa437e24c5826ed78217c4887882672e28c79421b17e501ea5`, `0ede32ab0bd5e0f67e4c6db0f3d8cc1910b8ced827f689786e144d997a1ba6fa`, `ebcd7ec515778c710991283f62e40e000d3867eca87410963d27e34ffc95f35b` |

hash 只用于 H04 源码漂移复核，不构成真实网络请求、Provider receipt 或附件效果证明。

## 2. Structured content contract

`ModelMessage`/`ModelOutput` 保留原有 `text`、`tool_calls`、`tool_call_id` 字段；新 `content`/`continuation` 字段使用 `serde(default)`，旧 cassette 缺失字段仍能读取，空 content 时由 `content_blocks()` 生成兼容块。Content block 只允许：

- `Text`：有界、无 NUL 的文本；
- `ToolCall`：复用 bounded `ModelToolCall`/唯一 call ID；
- `ToolResult`：绑定 call ID，不可脱离 pending assistant batch；
- `AttachmentRef`：仅 `artifact:` 引用、media type 和 `sha256`，不携带 bytes；
- `ProviderOpaque`：provider/protocol/route digest + protected artifact ref，不含可展示内部 payload。

`ProviderContinuation` 同样只携带 provider/protocol/route digest 和引用。`validate_model_history` 使用 structured blocks 做 assistant→tool 配对，任何 orphan、跨 role 工具结果、重复/越界调用在 provider 请求前拒绝。

## 3. Provider boundary

Provider compiler 先验证旧/新消息历史，再按最终 route 归一化 structured content。文本、工具调用和工具结果可无损落入现有各协议 mapper；AttachmentRef 在当前无明确 wire 映射时拒绝，ProviderOpaque/Continuation 若 provider、protocol 或 route digest 不匹配返回 `opaque_item_cannot_cross_provider`，即使匹配当前 provider 也不会猜测其内部形状。Provider-specific raw response、headers、secret 和 opaque bytes 不进入普通消息或 telemetry。

Daemon 的 legacy fixture adapter 继续走同一 ModelMessage/ModelOutput 兼容形状；生产模型路径仍是 `DaemonHost → ProviderGateway`，没有新增第二模型循环。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `opaque_item_cannot_cross_provider` | opaque continuation/item 的 provider/protocol/route 绑定不能跨 provider 搬运 |
| `unsupported_content_block_fails_before_request` | AttachmentRef 只有受控引用；当前不支持的 wire 模态在网络请求前拒绝 |
| `orphan_tool_result_is_rejected` | tool result 必须匹配同一 assistant batch 的 call ID |
| `legacy_cassette_and_typed_items_roundtrip` | 旧 text/tool_calls cassette 与新 typed blocks 可互读，兼容字段不丢失 |
| `provider_content_boundary_is_validated_before_wire_compilation` | provider 编译入口包含 content/route/opaque/unsupported guards，且不序列化 raw response |

`.github/workflows/h04-model-content.yml` 在 GitHub runner 执行 domain fixtures、provider source guard 和 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 5. 限制与交接

- AttachmentRef/ProviderOpaque 是受控引用合同，不等于 provider 已接收 bytes、真实渲染或外部业务结果；具体 image/file adapter 仍需后续 provider/integration steps。
- 当前 provider compiler 对 opaque/continuation 采取拒绝优先，避免跨模型复制不可读内部数据；需要支持时必须新增版本化 adapter、route binding 和 CI wire fixture。
- UI 文本视图继续从 safe ModelMessage/Receipt projection 读取，不把 provider wire body 或 raw response 当展示/事实源。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
