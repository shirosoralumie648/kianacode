# P4-J7-04 Provider 可复核基线

> 快照日期：2026-09-14。本文是 `P4-J7-04` 的 source-only 基线，不是运行时验收，也不改变产品行为。
> 本轮不在本地运行测试；行为测试由 GitHub CI 执行。调研文档（`docs/provider-design-research.md` §2）的源码观察固定在 2026-09-12，本轮逐条复核并标记已被 `kiana-provider` crate 修复的项。

## 1. 快照与范围

| 项目 | 记录 |
|---|---|
| source snapshot | `55cac10251feebbdca7233f8e64da44b5500e394`（H01 收口提交） |
| worktree 基线 | `master`，工作树干净；本文件及关联账本变更是本步文档提交 |
| 目标 | 按 [P4-J7-04 卡](provider.md#step-p4-j7-04)：固定 Provider 基线快照、对照调研 §2 复核缺口、索引现有测试、区分产品路径与遗留 fixture |
| 本轮范围 | model_client 产品/遗留分界、kiana-provider crate 全量、kiana-services 可达性、五个验收测试的代码归属、usage/finish 数据流 |
| 本轮不做 | 不实现 J7-05+ 的解析修复，不迁移遗留 fixture，不改产品代码 |

相关源码快照 hash：

| 边界 | 文件 | SHA-256 |
|---|---|---|
| 模型客户端装配 | `kiana-daemon/src/model_client.rs` | `e04c10bf4291a2bc0ac426bee9475acdd40f6c0cdd6d49b7d61a7e3dd2bfa847` |
| provider gateway | `kiana-provider/src/lib.rs` | `371f3baf389836b4e5d5af29b27a554582f470ae9351a3ad946c2b9a43124e1d` |
| 连接/profile 配置 | `kiana-provider/src/config.rs` | `c3d8d3ab8d8b48b9d2a8bebb16d02a0e466de5f5171ff70c3c23504e58df50c2` |
| 请求编译 | `kiana-provider/src/request.rs` | `8f3b1a3cc1e814cfa39a505a2350a0ac2a186f7d874abdcd55771c112d975877` |
| 传输/重试 | `kiana-provider/src/transport.rs` | `56d1fd235705792b9fa64a22e5cc480e75cfa9d69de6c8d98f2698c06dde9392` |
| 响应解码 | `kiana-provider/src/response.rs` | `e669c0e1ca82528feda6c9fef37d1fb6c8e674fc5ef2a1a5eaf9f452bd32829d` |
| 遗留 provider 栈 | `kiana-services/src/api/provider.rs` | `0d40ac404838850f514a8eb9592b91f41471aaddb4479dace5f67376adf5883f` |
| 遗留 client | `kiana-services/src/api/client.rs` | `28267e54be068fe4bd18a887543bee1ae3aebaa2d2a300b0b9dba00bd3afc5a5` |
| 遗留 retry | `kiana-services/src/api/retry.rs` | `61ed78278b5ae82ba4d953037c2ff286d86e760c7a09fcc68d887f0caed7da4b` |
| 遗留 streaming | `kiana-services/src/api/streaming.rs` | `53a356646cc86e3d639daee1a6e440c764f76e07d030003193c15b3dd89964cd` |
| runner 模型门面 | `kiana-runner/src/model.rs` | `1e361f183b4cdba8a9114b24120feb6f8bf641763808ea0733dacef8259191cd` |
| 模型合同 | `kiana-domain/src/model.rs` | `9b771e6f1f3fcb517f2ed8b02fb8b8e1b5a04529e9d0b7cf3c88477ec967ff51` |
| usage 记录 | `kiana-domain/src/usage.rs` | `e254ce2af238bd693647c3a52a2cb80c9fdb7b37bc0d192b42c0431731471b70` |
| provider_standard 测试 | `kiana-services/tests/provider_standard.rs` | `11bcd8a435a2ddd202876efd8fa511bcf83f163616e8e81fbf50ecf704e3f89d` |

## 2. 产品路径与遗留分界（本轮最重要的基线事实）

1. **`kiana-daemon/src/model_client.rs` 只有 1–39 行是产品代码**：`from_config` 的选择顺序为 ① `KIANA_HARNESS_SCRIPT` → ScriptedModel；② `provider==fake` → 预载 128 轮固定文本的 ScriptedModel；③ 其余 → `kiana_provider::ProviderGateway::from_env`。**42–2102 行全部是 `#[cfg(test)] mod legacy_fixtures`**（注释明言 "not linked into product builds"）：`ConfiguredProfile`、`ProfileRouter`、`aggregate_provider_stream`、`map_tools`、`output_from_response` 都只存在于该模块。
2. **`kiana-provider` 是唯一网络化产品路径**，且**没有任何测试**（无 `#[test]`、无 `tests/` 目录）。五个协议：anthropic（v1/messages）、openai/openai-compatible（chat/completions）、openai-responses、ollama（api/chat）、gemini（interactions）；全部原生流式（SSE/NDJSON），无合成事件。
3. **kiana-services 遗留栈仍可达但不在执行主干**：`kiana-commands` 的 `model smoke --live` 诊断（`kiana-commands/src/model.rs:615/649` 直接构造 AnthropicProvider）和 bridge/remote 兼容面引用它；执行主干（DaemonHost→ControlPlane→Harness）不经过。
4. **profile 路由是产品路径但换了实现**：`KIANA_MODEL_PROFILES_JSON` 由 `kiana-provider/src/config.rs:83-176` 解析（64KiB 上限、角色 profile 校验 `model_profile_unknown`、`inherit_default`、api_key_env 格式、信号量去重）；角色 → `ModelAssignment`（`lifecycle.rs:227-238`）→ runner 绑定 → `ModelCallSpec.assignment`。六角色 catalog 的 model_profile 为 executing/planning/monitoring/initiating/closing，**均非 "default"**——不配置 profiles 时全部回退 default 连接。

## 3. 调研 §2 缺口复核（10 项）

| 调研缺口 | 现状 | 证据锚点 |
|---|---|---|
| ProfileRouter 仅 test 模块 | **已修复**：产品路由在 kiana-provider `connections()`/`connection()` | config.rs:83-176, lib.rs:27-42 |
| `message_stop` OR 任意 stop_reason 即终态 | **已修复**：旧聚合器在 legacy_fixtures；新解码严格 | response.rs（流式终态校验） |
| openai tool 坏 JSON 回 `{}` | **已修复**：`parse_arguments` 拒绝坏 JSON（`provider_tool_json_invalid`）、缺 id 拒绝（`provider_required_identity_missing`） | response.rs:46-56, 117-118 |
| 非流式缺 ID/name 缺省 + filter_map 丢坏 schema | **代码仍在但非产品**：`map_tools`/`output_from_response` 仅 legacy_fixtures；产品路径严格拒绝 | model_client.rs:778-799（test-only） |
| OpenAI/Ollama 合成流 | **已修复**：产品栈原生 SSE/NDJSON，无 synthetic | kiana-provider transport.rs |
| client.rs unwrap/expect、无统一时限 | **已修复**：`TransportLimits{headers 30s, first_event 60s, idle 45s, total 180s, max_body 8MiB, max_frame 256KiB}`；client 构造失败映射为 ModelError；redirect 禁用。**残留**：kiana-services client.rs 的 unwrap/expect 仍被 `model smoke --live` 诊断路径触及 | config.rs:50-69, 361-367; services client.rs:24/38 |
| 重试按消息子串猜类别、TLS 证书错误可重试 | **已修复**：`ModelRetryClass{Never,BeforeSend,Rejected}` 结构化分类（429/503→Rejected 带 Retry-After；connect→BeforeSend；其余 Never；cert 错误→Never）；runner 按枚举匹配不解析字符串。**残留**：cert 检测本身仍靠 "certificate" 子串 | model.rs:465-504, transport.rs:52-82, harness.rs:956-978 |
| delta 仅 Text、usage 仅 input/output、stop_reason 仅元数据 | **部分成立**：`ModelDelta` 仅 Text 变体；`ModelUsage` 仅两 token 字段（Gemini thought tokens 折进 output）；**stop_reason 已修复**——`ModelFinish` 枚举（EndTurn/ToolUse/Length/Refusal/Pause/Incomplete）为一等字段，Length/Refusal/Pause fail-closed | model.rs:234-240, 189-196, 423-464 |
| UsageRecord/CostLedger 缺 attempt/实服务模型/cache/reasoning/价格版本/失败用量 | **部分成立**：缺 cache/reasoning tokens（Anthropic cache 字段被丢弃）、价格版本（cost_micros 恒 None）、失败 attempt 用量；**attempt 与实际模型已修复**（`ModelCallSpec.attempt_id`、`ModelOutput.model_id` 流入 UsageRecord） | usage.rs:6-16, receipts.rs:519-535 |
| request_context 字节记账非最终 wire 预算、输出预留固定 4096 | **已修复**：`request.rs:127-140` 对**最终完整 wire body**（含协议映射、system、schemas、framing）计一次预算，输出预留为 `connection.max_output`；4096 仅是 builtin catalog 兜底。`ModelRequestContext::for_request` 的硬编码仅剩离线 ScriptedModel 使用 | request.rs:127-140, config.rs:344 |

## 4. 现有测试索引

| 测试 | 位置 | 测的是什么代码 |
|---|---|---|
| `native_streaming_without_a_terminal_event_fails_closed` | model_client.rs:1546 | **仅 fixture**：legacy `aggregate_provider_stream`（等价产品逻辑在 transport.rs:191 / response.rs） |
| `native_streaming_invalid_tool_json_fails_closed` | model_client.rs:1582 | **仅 fixture**：同上 |
| `provider_does_not_retry_auth_errors` | model_client.rs:1966 | **混合**：断言的产品代码是 kiana-services `OpenAiCompatibleProvider` + `with_retry`（仍编译进产品、被 --live 诊断使用），harness 是 fixture |
| `provider_wrapper_maps_tool_calls_and_final_text` | model_client.rs:1637 | **仅 fixture**（FakeProvider + ProviderModelClient 均为 legacy 栈） |
| `anthropic_native_streaming_aggregates_output_without_network` | model_client.rs:1696 | **仅 fixture**：产品 AnthropicProvider（kiana-services）对 loopback mock；非 kiana-provider crate |
| `provider_standard`（5 个子测试） | kiana-services/tests/provider_standard.rs:26-172 | **确认 Fake-only**；断言 metadata、unsupported_tools 拒绝、tool 映射、合成流事件 |
| kiana-services provider.rs mod tests（9 个） | provider.rs:1435+ | loopback mock HTTP；产品代码（遗留栈） |
| **kiana-provider crate** | — | **零测试** |

**结论（RED 记录，不声称已修复）**：`kiana-provider`——当前唯一网络化产品路径——完全没有测试保护。卡片要求的「先拒绝」语义（不完整流、坏 tool JSON、auth 不重试）在新 crate 有实现但无测试；这是 `P4-J7-05`（严格非流式解析）及后续测试迁移卡的直接输入。遗留 fixture 测试在旧栈上的回归价值仅覆盖 `model smoke --live` 诊断路径。

## 5. 交接

- J7-05 的严格解析验收应落在 `kiana-provider`（response.rs decode 路径），不是修 legacy fixtures。
- 迁移归属：`ProfileRouter`/`aggregate_provider_stream` 等 legacy 符号无第二套产品实现，未来清理只需处理 kiana-services 可达面（`model smoke --live`、bridge/remote）。
- `ModelRequestContext::for_request` 硬编码 128k/4096 仅 ScriptedModel 路径使用，产品路径已用 connection 能力快照；J7-08/-11/-12 不需要再改它。
