# Responses API 调研：Codex 怎么用，Kiana 要不要跟

> **本文速览（导读，非规范）**
>
> - Codex 已经把 chat completions **删了**，只剩 Responses 一条线；它把推理状态、工具调用、结构化输出、缓存都建在 Responses 语义上。
> - Kiana 现在走的是 Anthropic messages + OpenAI chat completions + Ollama 三条 chat 风格的线，**没有 Responses**。
> - 结论：Responses 比 chat completions 多的核心不是"字段多一点"，而是**推理状态可以跨轮带着走**；对 Kiana 值得做，但**第一步不是写适配器**，而是先把它挂进今天刚解冻的 live provider 范围里，用 fake/cassette 落一个 `partial`，再谈 live。
> - 本文里 `reference/codex/` 的代码只是**参考审计材料**，只能证明"Codex 这么做"，**不能**证明 Kiana 已有这些能力。
> - 当前事实以 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准；本文是调研，不是承诺。

---

## 0. 一句话结论

**Responses 相比 chat completions 多的是"推理状态 + 服务端可寻址的响应对象"这套语义：推理内容（encrypted reasoning）可以原样带回下一轮，响应有 id、有结构化 usage、有内置工具和 json_schema 输出格式——chat completions 这些要么没有、要么做得很别扭。对 Kiana 值得采用，但只值得作为"一条新的 provider 线"，不是推翻现有 chat 路径，而且采用它的真正门槛在证据（live），不在代码。**

补一句更直白的：Codex 把 chat 彻底删掉（配置里写 `wire_api = "chat"` 直接报错），说明它认为**工具调用、状态重放、结构化输出这些能力必须走 Responses**。Kiana 现在没有这条路。

---

## 1. Responses API 支持什么（Codex 怎么用 + 证据）

下面每一条都按「能力 → Codex 怎么用 → 文件路径证据」写。路径都在 `reference/codex/codex-rs/` 下，属于审计材料。

### 1.1 推理（reasoning）

- **Codex 怎么用**：请求体里带 `reasoning: { effort, summary, context }`；`effort` 取显式值或模型默认，`summary` 只在模型支持该参数时才发，`context` 只在 `responses_lite` 下设 `all_turns`。主采样请求固定 `include: ["reasoning.encrypted_content"]` + `store: false`，意思是"把加密的推理内容给我，但你别在服务端存"。模型返回的 `ResponseItem::Reasoning { summary, content, encrypted_content }` 会写进本地 history，下一轮原样回填；回填时只带 summary + encrypted_content，**不回传原始思维链**。
- **证据**：`codex-api/src/common.rs:170-197`（`Reasoning`/`ReasoningContext`/`StreamOptions`）；`core/src/client.rs:870-889`（`build_reasoning`）、`:959`（`include`）、`:986`（`store:false`）；`protocol/src/models.rs:1016-1028`、`:1592-1599`（序列化时跳过原始 content）；`core/src/context_manager/history.rs:771-794`、`:1093-1113`（Reasoning 作为持久项回填）。
- **一个附加细节**：服务端响应头 `x-reasoning-included` 为真时，Codex 会把历史里非最近一轮的 encrypted reasoning 也计入 token 估算，避免重复计费（`codex-api/src/sse/responses.rs:30,53-56`；`history.rs:636-694`）。

### 1.2 多轮状态

- **Codex 怎么用**：HTTP 路径的 `ResponsesApiRequest` **根本没有 `previous_response_id` 字段**，而且 `store` 恒为 `false`。也就是说每个 HTTP 请求都把完整 history 全量重发，Codex 自己的 history 是唯一真源。
- **`previous_response_id` 只用于 WebSocket 增量**：`ResponseCreateWsRequest` 才有这个字段；只有当本次 input 是上次 input + 服务端输出项的严格扩展时，才带上上次 response id 并只发增量 items，失败（`previous_response_not_found`）就退回全量 HTTP。
- **证据**：`codex-api/src/common.rs:281-307`（HTTP 请求体无 `previous_response_id`）、`:309-332`（From 时置 None）、`:334-364`（WS 请求才有）；`core/src/client.rs:986`、`:1331-1411`（增量计算）；`codex-api/src/endpoint/responses_websocket.rs:159-163,620-642`、`core/src/responses_retry.rs:44-129`（失效兜底）。
- **注意**：OpenAI 官方 API 本身在 HTTP 上也支持 `previous_response_id`，是 **Codex 主动选择不用**。这是 Codex 的取舍，不是 API 限制——别把"Codex HTTP 不发"读成"HTTP 不能发"。

### 1.3 工具调用

- **请求侧**：`tools` 是一组 tag=`type` 的枚举：`function` / `namespace` / `tool_search` / `web_search` / `custom`。函数工具是 `ResponsesApiTool { name, description, strict, defer_loading?, parameters, output_schema }`；自由格式工具是 `FreeformTool { name, description, format }`。请求里以原始 JSON 数组嵌入。
- **响应侧**：输出项 `ResponseItem` 也是 tag=`type` 的枚举，包括 `message`、`reasoning`、`function_call`、`function_call_output`、`custom_tool_call`、`custom_tool_call_output`、`web_search_call`、`image_generation_call`、`compaction` 等，未知 type 落到 `Other`。**`FunctionCall.arguments` 是字符串形式的 JSON**，Codex 保留原样再自己解析。
- **参数增量有个坑**：`response.function_call_arguments.delta/.done` 被 Codex **显式忽略**（只打 trace 日志），函数工具的完整参数从 `response.output_item.done` 整包拿；真正做增量的是自由格式工具的 `response.custom_tool_call_input.delta`。
- **并行**：`parallel_tool_calls` 是协议字段，执行侧用 `FuturesOrdered` 并发，每个工具是否可并行由 `ToolRouter::tool_supports_parallel` 决定（读锁并行 / 写锁串行）。history 归一化强制每个 call 都有配对 output，删掉孤立 output。
- **证据**：`tools/src/tool_spec.rs:18-56,82-149`；`tools/src/responses_api.rs:15-80`；`protocol/src/models.rs:978-1214`、`:1042-1061`、`:1077-1101`、`:2055-2219`；`codex-api/src/sse/responses.rs:353-549`（`process_responses_event`）、`:370-379`、`:520-545`（unhandled 列表）；`core/src/session/turn.rs:2232-2255,2325,2502-2504`；`core/src/tools/parallel.rs:116-178`；`core/src/context_manager/history.rs:702-720`。

### 1.4 内置工具

- **Codex 怎么用**：会发服务端托管的 `web_search` 工具（可按 `external_web_access` / `indexed_web_access` / 位置 / 上下文大小 / 内容类型配置），模型触发的 `web_search_call` 输出项被解析成 `ResponseItem::WebSearchCall` 展示。另有独立的 `alpha/search` 端点和 `image_gen.imagegen` 命名空间扩展工具，各有 feature 门控。
- **容易误读的地方**：`file_search` / `computer_use` / `local_shell` **不是** Codex 的 Responses 内置工具，它们只出现在"保留命名空间"列表里防止撞名；当前 shell 能力是以普通 function 工具暴露的，不会发 `{"type":"local_shell"}`。
- **证据**：`tools/src/tool_spec.rs:33-53`；`core/src/tools/hosted_spec.rs:14-46`；`protocol/src/models.rs:1151-1172`；`codex-api/src/endpoint/search.rs:31-44`；`codex-api/src/search.rs:1-80`；`core/src/tools/spec_plan.rs:1038-1047,699-730,1431-1452`；`app-server/src/request_processors/thread_processor.rs:295-305`（保留命名空间含 file_search/computer/browser/python）。
- **对 Kiana 的硬边界**：这些内置工具会扩大"模型可见工具"面。Kiana 的工具面锁死为 5 个（`docs/features/03-five-tools.md`），所以就算接 Responses，也**只允许映射现有 5 个 function 工具，不许打开 web_search / image_gen / tool_search / namespace**。

### 1.5 结构化输出

- **Codex 怎么用**：`Prompt` 带 `output_schema` / `output_schema_strict`，`create_text_param_for_request` 把它们转成 `TextControls { verbosity, format: TextFormat { type: json_schema, strict, schema, name: "codex_output_schema" } }`，序列化进请求的 `text` 字段；`verbosity`（low/medium/high）也走同一个 `TextControls`。两者都为空时不下发 `text`。
- **证据**：`codex-api/src/common.rs:199-245,396-414`；`core/src/client.rs:960-975`；`core/src/session/turn.rs:1404-1405`；`core/src/guardian/assessment.rs:80`。

### 1.6 缓存与用量

- **缓存**：请求带 `prompt_cache_key`（优先 session 级 override，否则 `{source}:{parent_thread_id}` 或 session id）；命中信息来自 `response.completed.response.usage.input_tokens_details.cached_tokens` 和 `cache_write_tokens`，映射进 `TokenUsage.cached_input_tokens` / `cache_write_input_tokens`。
- **用量**：`response.completed` 给出 `usage`（input / cached_input / cache_write_input / output / reasoning_output / total / 可选的 codex_rollout_budget_units）、`usage_metadata`、`end_turn`。`end_turn = Some(false)` 时触发 `needs_follow_up` 继续循环。
- **证据**：`core/src/client.rs:504-516,976`；`codex-api/src/common.rs:299-300`；`codex-api/src/sse/responses.rs:116-166,483-508`；`protocol/src/protocol.rs:2216-2223`；`core/src/session/turn.rs:2650-2694`。

### 1.7 流式事件

- **传输**：HTTP 走 `POST {base_url}/responses`，`Accept: text/event-stream`；另有 WebSocket 传输复用同一请求类型和同一个 `process_responses_event` 解析器。鉴权由 `BearerAuthProvider` 注入 `Authorization: Bearer` 与 `ChatGPT-Account-ID`。
- **事件分派**：`process_responses_event` 按 `type` 分派 `response.created`、`response.output_item.added/done`、`response.output_text.delta`、`response.custom_tool_call_input.delta`、`response.reasoning_summary_text.delta/done`、`response.reasoning_text.delta`、`response.reasoning_summary_part.added`、`response.completed`、`response.failed`、`response.incomplete`。流在 `response.completed` 前关闭会报 `stream closed before response.completed`。
- **错误分类**：`response.failed.error.code` 映射成一组明确错误：`context_length_exceeded` → 上下文超限、`insufficient_quota` → 配额、`usage_not_included`、`cyber_policy`、`misalignment_policy_violation`、`invalid_prompt`/`bio_policy`、`server_is_overloaded`、`rate_limit_exceeded`（还会从 message 里正则解析重试秒数），其余归 `Retryable`；`response.incomplete` 取 `incomplete_details.reason`。**顶层 `type="error"` 事件 Codex 没有单独处理**，会落到未知事件分支——Kiana 若做可以补上，免得静默丢错。
- **响应头派生事件**：读 SSE 之前先从 HTTP 响应头生成事件，如 `openai-model` → ServerModel、限流头 → RateLimits、`x-reasoning-included`、`x-codex-turn-state` 供本回合后续请求回传。
- **证据**：`codex-api/src/endpoint/responses.rs:26-47,102-190`；`codex-api/src/sse/responses.rs:36-102,168-184,193-275,353-549,568-690`；`codex-api/src/common.rs:101-153`（`ResponseEvent`）；`model-provider/src/bearer_auth_provider.rs`；`model-provider-info/src/lib.rs`（默认 base_url / 内置 provider）。

---

## 2. Kiana 现状对比

先给结论：**Kiana 的 provider 层是"Anthropic 形状"的，Responses 缺的不是一个字段，是一整套"类型化的输入项 + 推理状态 + 原生 SSE"**。

### 2.1 现在有什么（代码事实）

| 项 | Kiana 现状 | 证据 |
|---|---|---|
| provider 协议 | `ProviderProtocol` 只有 `AnthropicMessages` / `OpenAiChatCompletions` / `OllamaChat` / `Fake`，**没有 Responses** | `kiana-services/src/api/provider.rs:75-80` |
| Anthropic | `POST {base}/v1/messages`，**原生 SSE** 解析（`message_start` / `content_block_delta` / `message_delta` / `thinking_delta` 等） | `kiana-services/src/api/messages.rs:48`；`kiana-services/src/api/streaming.rs:103,132-148` |
| OpenAI-compatible | `POST {base}/chat/completions`；**`stream_message` 先发非流式请求，再把整段响应合成事件**（`StreamingMode::Synthetic`） | `kiana-services/src/api/provider.rs:513-514,559-563`；`provider.rs:281` |
| Ollama | `POST {base}/api/chat`，同样是 Synthetic | `kiana-services/src/api/provider.rs:895-896` |
| 内部消息模型 | `MessagesRequest { model, messages, max_tokens, system, temperature, tools, thinking, stream }`——Anthropic 形状 | `kiana-services/src/api/messages.rs:13-28` |
| usage | 只有 `input_tokens` / `output_tokens` | `kiana-services/src/api/messages.rs:40-44`；`kiana-runner/src/model.rs:140-148` |
| 工具调用 | 支持，但走 chat：从**完整响应**里取 `tool_use` / `tool_calls` 聚合成 `ModelToolCall`，没有参数增量 | `kiana-daemon/src/model_client.rs:420-452,626-664`；`provider.rs:670,700` |
| 结构化输出 | 全部 `supports_structured_output: false`，没有实现 | `kiana-services/src/api/provider.rs:351,371`；`kiana-daemon/src/model_client.rs:772,826,867` |
| 流式接缝 | 已落地 `ModelClient::complete_streaming`（带默认实现）+ `ModelDelta::Text`；daemon 有 `aggregate_provider_stream` | `kiana-runner/src/model.rs:196-226`；`kiana-daemon/src/model_client.rs:231-283,330-452` |
| 状态账本 | live provider 与 token streaming 整体仍是 `not_supported | source` | `CURRENT_STATUS.md:61-62` |

### 2.2 缺哪些能力

对着第 1 节逐条比：

- **推理状态**：没有 OpenAI 的 `reasoning` 项、没有 `encrypted_content` 回填、没有 reasoning token 计费。Anthropic 的 `thinking` 块是另一套东西，不能替代跨轮推理状态。
- **多轮状态**：没有 `previous_response_id`（Codex 的 HTTP 路径也没用，但 Kiana 连"服务端响应对象"这个概念都没有，只有本地 messages 历史）。
- **工具调用**：有 function tool 调用，但**没有 Responses 的类型化输出项**（`function_call` / `custom_tool_call` / `function_call_output` 的 wire 形状），没有 custom tool 的参数增量，没有 `parallel_tool_calls` 协议字段。
- **内置工具**：完全没有；而且按仓库硬边界，Kiana 也不该加（工具面锁死 5 个）。
- **结构化输出**：完全没有（`supports_structured_output: false`）。
- **缓存与用量**：没有 `prompt_cache_key`、没有 cached/cache_write token、没有 reasoning token、没有 `end_turn`。
- **流式事件**：只有 Anthropic 是**真原生 SSE**；OpenAI 路径是"先整段再拆"的 Synthetic。Responses 的 SSE 事件类型（`response.output_item.done` 等）**一个都没解析**。

---

## 3. 如果 Kiana 要加 Responses 适配器

### 3.1 最小实现清单

**A. 请求映射**

- 在 `ProviderProtocol` 加一个 `OpenAiResponses` 变体，并在 `built_in_provider_registry()` 加一条 registry entry（provider_id、base_url、env vars、`live_smoke_required: true`）。
  - 证据：`kiana-services/src/api/provider.rs:75-80,234-320`。
- 把 Kiana 的 `MessagesRequest` 映射成 Responses 请求：`model`、`instructions`（= system，**不是 system message**）、`input: Vec<ResponseItem>`、`tools`（类型化 JSON 数组）、`tool_choice: "auto"`、`parallel_tool_calls`、`reasoning`、`store: false`、`stream: true`、`include: ["reasoning.encrypted_content"]`、`prompt_cache_key`、`text`（verbosity + json_schema format）。
  - 证据（Codex 侧）：`codex-api/src/common.rs:281-307`；`core/src/client.rs:891-997`。
- **最关键的一步**：Kiana 内部是 Anthropic 的 content block（`tool_use` / `tool_result` / `thinking`），要写一层双向翻译到 Responses 的类型化 item（`message` / `function_call` / `function_call_output` / `reasoning`）。这层不做，后面的流式和推理状态都无处落。
  - 证据（Kiana 侧）：`kiana-daemon/src/model_client.rs:574-624`（`map_messages`）、`provider.rs:594-736`。
- 注意 Responses 请求体**没有** `max_output_tokens` / `temperature` / `top_p`（Codex 就不发）。Kiana 的 `max_tokens` / `temperature` 要么丢掉、要么显式声明不支持，不能假装映射过去了。
  - 证据：`codex-api/src/common.rs:281-307`。

**B. SSE 解析**

- 现在 Kiana 只有 Anthropic 的 SSE 解析器（`eventsource_stream` + `StreamEvent`），OpenAI 路径根本不发 `stream: true`。要新增一个按 `event.type` 分派的 Responses 解析器，覆盖 `response.created` / `output_item.added` / `output_item.done` / `output_text.delta` / `reasoning_summary_text.delta+done` / `reasoning_text.delta` / `custom_tool_call_input.delta` / `completed` / `failed` / `incomplete`。
  - 证据（Codex 侧）：`codex-api/src/sse/responses.rs:353-549`；（Kiana 侧）`kiana-services/src/api/streaming.rs:132-148`。
- 要带**空闲超时 + 有界 channel + 取消检查**；流在 `response.completed` 前关闭必须 fail-closed，不能当成功。
  - 证据：`codex-api/src/sse/responses.rs:568-690`；`docs/streaming-unfreeze-plan.md` §5（取消 / 断线 / SEC-08）。

**C. 工具调用聚合**

- 从 `response.output_item.done` 整包拿 `FunctionCall.arguments`（字符串 JSON，自己解析）；**不要**依赖 `response.function_call_arguments.delta`（Codex 显式忽略）。自由格式工具才用 `custom_tool_call_input.delta`。
  - 证据：`codex-api/src/sse/responses.rs:370-379,520-545`；`protocol/src/models.rs:1042-1061`。
- 保证每个 call 都有配对 output（Codex 用 history 归一化强制），否则下一轮回填会出现悬空 call。
  - 证据：`core/src/context_manager/history.rs:702-720`。
- 工具面只映射现有 5 个 function 工具，不注册任何 hosted / namespace 工具。
  - 证据（Kiana 硬边界）：`docs/features/03-five-tools.md`；`docs/coding-pack-matrix.md` FZ-TOOLS。

**D. 用量 / 停止原因 / 错误**

- 从 `response.completed.response.usage` 取 input / cached / cache_write / output / reasoning / total，并读 `end_turn`；Kiana 的 `ModelUsage` 目前只有两个字段，要扩展。
  - 证据（Codex 侧）：`codex-api/src/sse/responses.rs:116-166,483-508`；（Kiana 侧）`kiana-runner/src/model.rs:140-148`。
- 建一张 `response.failed.error.code` → 错误类别的表（`context_length_exceeded` / `insufficient_quota` / `rate_limit_exceeded` / `server_is_overloaded` / `cyber_policy` / `misalignment_policy_violation` 等），`response.incomplete` 映射成流错误；**额外补上顶层 `type="error"` 的处理**（Codex 没处理）。
  - 证据：`codex-api/src/sse/responses.rs:417-482,685-748`。

### 3.2 工作量估计

| 切片 | 估计 | 理由 |
|---|---|---|
| 请求映射 + item 双向翻译 | **M** | 最贵。内部模型是 Anthropic 形状，翻译层是新增逻辑，且要保住推理状态不丢 |
| Responses SSE 解析 | **S–M** | 依赖 `eventsource_stream` 已在，Anthropic 解析器可当模板；但事件类型多、错误分类和取消要补 |
| 工具调用聚合 | **S** | 沿用现有 `ModelToolCall` 聚合形状，只是数据来源换成 `output_item.done` |
| 用量 / 停止原因 / 错误表 | **S** | 纯映射，但要扩 `ModelUsage` |
| **适配器整体** | **M** | 不碰协议、不动工具面、默认关闭时零回归 |

**如果还要"接三个界面的真流式 + live 证据块"，整体是 L**：跨 provider 层、runner、core 事件通道、CLI/workbench/web 四个面，外加证据套件。这部分不是 Responses 独有的成本，是 [`docs/streaming-unfreeze-plan.md`](streaming-unfreeze-plan.md) 方案 B 的成本。

### 3.3 风险

- **与现有 provider 抽象的关系（最大风险）**：`Provider` trait 的两个方法是 `create_message` / `stream_message`，入参是 Anthropic 形状的 `MessagesRequest`，返回的是 Anthropic 形状的 `StreamEvent`。Responses 不是 messages——硬塞进去会丢推理项、丢响应 id、丢结构化输出。两条路：(a) 在 trait 内部做翻译（改动小、但翻译层会一直别扭）；(b) 抽一个中立的内部消息 / 事件模型（改动大、但更干净）。**建议先按 (a) 落 `partial`，把 (b) 留作需要推理状态跨轮时的第二步**。证据：`kiana-services/src/api/provider.rs:221-232`；`kiana-runner/src/model.rs:185-226`。
- **与流式切片的关系**：Kiana 的 OpenAI 路径现在是 Synthetic（整段再拆），Responses 必须真 SSE。这意味着接 Responses 会顺带把"OpenAI 系原生流式"这条也打开，牵连取消、断线重连、跨 chunk 脱敏（SEC-05）、配额（SEC-12）、事件账本不能逐 token 写（AGENTS §6 单一事实源）。这些在 [`docs/streaming-unfreeze-plan.md`](streaming-unfreeze-plan.md) §5 已逐条列过，Responses 适配器要复用那套约束，不能另起一套。
- **与证据纪律的关系**：现在 `live provider | not_supported | source`、`token streaming | not_supported | source`（`CURRENT_STATUS.md:61-62`）。**写出适配器代码不会自动把状态升级**。按 `docs/README.md` §4，`implemented` 只是"代码路径存在"，还要看 `proof_level`；按 FZ-LIVE（`docs/coding-pack-matrix.md:168`），"当完成"的门槛是**真实 provider adapter + 对账 + 证据块**，且首发只能落 `partial` + `local_behavior`。
- **OpenAI 专属 vs 可移植**：`encrypted reasoning`、`prompt_cache_key`、`x-codex-*` 头、`responses_lite`、`service_tier`、hosted web_search 这些很可能不是通用 Responses 规范，而是 OpenAI / Codex 自己的东西。别的 provider 即使有 `/responses` 端点，也可能忽略或直接拒绝。适配器必须**逐项 fail-closed 或显式降级**，不能默认对方全支持。
- **别把 chat 路径删了**：Codex 能删 chat 是因为它只服务 OpenAI；Kiana 的 `openai-compatible` 面向的是"任何像 OpenAI 的服务"，很多只提供 chat completions。Responses 适配器是**新增一条线**，不是替换。

---

## 4. 建议

### 4.1 分几步

**第 0 步（先做，且是硬门槛）：把这件事挂进已有书面决定，不要新开口子。**

- 2026-09-09 的书面决定已经把 **live provider + token streaming 解冻**，范围是"流式全链路、三个界面"，**证明上限必须到 `live`（真实 provider adapter + 对账 + 证据块）**；其余 live provider 能力（批处理、文件接口等）继续冻结。见 `AGENTS.md:167`、`docs/coding-pack-matrix.md:168`、[`docs/streaming-unfreeze-plan.md`](streaming-unfreeze-plan.md) §8。
- Responses 适配器正好落在"真实 provider adapter"这条线里。**但要在决定里写清**：它是流式/provider 线的一部分，不是新开冻结项；且**不包含** hosted 工具、批处理、文件接口。
- 产出：一份一页的范围说明 + 明确的"首发只到 `partial` + `local_behavior`"。

**第 1 步（第一个代码 PR）：只走 fake / cassette 的适配器骨架。**

- 加 `ProviderProtocol::OpenAiResponses` + registry entry；实现请求映射、SSE 解析、工具聚合、usage/stop/error 映射；用**本地假 SSE 服务器 + cassette** 证明，不接真服务。
- 默认关闭 / 不进产品默认路径；既有 golden / cassette 行为零回归。
- 产出：`partial` + `local_behavior` 的证据块，**不声称 live**。
- 这一步刻意不碰：协议版本、工具面、三个界面、live。

**第 2 步：接进已落地的流式接缝。**

- 复用 `ModelClient::complete_streaming` + daemon 的 `aggregate_provider_stream`，按 `streaming-unfreeze-plan.md` 方案 B 的切片接 CLI / workbench / web（loopback SSE）；开关默认按计划是 `auto`，开发期 opt-in 默认关闭。
- 补齐取消、断线、跨 chunk 脱敏、事件账本粒度等负向路径。

**第 3 步：live 证据块，然后才改状态。**

- 对真实 provider 跑通，做 usage/stop 对账，补负向证据（不支持时 `unsupported_streaming` fail-closed、取消终态 `cancelled`、无法确认时 `result_unknown`）。
- 证据齐了再把 `CURRENT_STATUS.md` 从 `not_supported | source` 往上改。

### 4.2 第一步具体是什么

**先写范围说明（挂进第 0 步的书面决定），再落一个"只走 fake/cassette、默认关闭"的 Responses 适配器骨架——请求映射 + SSE 解析 + 工具聚合 + 用量/停止/错误映射，附单测，不接 live、不改协议、不新增任何模型可见工具。**

### 4.3 和"必须到 live"的证据要求怎么衔接

- 这次解冻的口径是"**首发证明上限必须到 `live`**"——意思是**对外宣称"支持"**之前，必须拿到真实 provider adapter + 对账 + 证据块，不接受只用 fake/synthetic 就说完成。
- 但**开发过程仍然可以先落 `partial` + `local_behavior`**（用 fake/cassette 证明拒绝路径和聚合正确），只是这个 `partial` 不能对外说成"支持 Responses / 支持 live provider"。
- 换句话说：**适配器是 live 证据的前提，不是 live 证据本身。** 代码写出来只到 `implemented | source`；本地假服务器跑到 `local_behavior`；真 provider + 对账 + 证据块才到 `live`。三者不能混。
- 另外，按 AGENTS §8，**先证明拒绝路径再证明成功路径**：`unsupported` / 取消 / `result_unknown` / 重放 / 配额这些负向证据缺一不可。

---

## 5. 不确定的地方（如实列出）

1. **Codex 的哪些行为是 OpenAI 专属、哪些是通用 Responses 规范？** 调研只覆盖了 Codex + OpenAI。`reasoning.encrypted_content`、`include` 的取值、`prompt_cache_key`、`x-codex-*` 头、`ChatGPT-Account-ID`、`responses_lite`、`service_tier`、`access_programs`、guardian 端点，看起来都偏 OpenAI / Codex 私有；没有证据说明第三方 `/responses` 端点会接受。
2. **DeepSeek 等第三方 provider 的 Responses 是否完全兼容？** 调研数据里**没有**任何第三方 provider 的 Responses 实现证据。需要单独做一次 live 探测（端点是否存在、支持哪些字段、不支持时怎么报错），不能从 Codex 推断。
3. **`previous_response_id` 在 HTTP 上到底能不能用？** OpenAI 官方 API 支持，但 Codex 的 HTTP 路径主动不用、只在 WS 用。Kiana 若走 HTTP，是否需要它、能不能用，取决于目标 provider，调研未验证。
4. **`function_call_arguments.delta` 被忽略，是 Codex 的选择还是 OpenAI 不可靠？** 只有 Codex 源码证据（显式 unhandled），没有服务端行为证据。Kiana 若想做函数参数实时展示，需要先探测目标 provider 是否稳定发这个事件。
5. **`response.output_text.done` 同样被 Codex 忽略**，它靠 `output_item.done` 拿完整文本。第三方 provider 若不发 `output_item.done`，这套聚合就会缺最终文本——兼容性待验。
6. **Kiana 内部模型能不能装下推理项？** 现在是 Anthropic content block + `MessagesRequest`，没有"opaque 加密状态"的位置。要不要先抽中立消息模型，需要一次设计 spike 才能判断，本文的工作量估计不含这次 spike。
7. **工作量估计是粗估**，没有做过实现验证；SSE 事件数量、错误分类、取消语义都可能让 M 变成 M+。
8. **流式解冻的落地进度**：调研期间看到接缝（`complete_streaming`）和订阅（`subscribe_run`）已落地，但整体状态仍是 `not_supported | source`。第 2 步能复用多少、还要补多少，要看这些切片最终合并成什么样。
9. **"不新增模型可见工具"与 hosted 工具的关系**：Responses 的 `web_search` / `image_gen` 是服务端托管的，但从模型视角仍是工具。本文按仓库硬边界把它们全部排除，只映射现有 5 个 function 工具；如果将来要谈，需要单独决定，不在本次范围。

---

## 附：一句话记住

Responses 的核心增量是**"推理状态可以跨轮带着走 + 响应是可寻址对象"**，不是字段更多。Kiana 值得加一条线，但先把范围挂进 live 解冻决定，用 fake/cassette 落 `partial`，别急着说"支持"。
