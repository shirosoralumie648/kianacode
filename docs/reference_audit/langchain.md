# langchain Reference Audit

## 1. 在 Kiana 中的参考定位

`reference/langchain` 是框架型参考，适合借鉴 runnable/event streaming、agent middleware、tool schema、provider model profiles、standard tests、retrievers/vectorstores 和 SSRF/security tests。它不适合作为 Kiana 产品结构参考；Kiana 应抽取 contract、test harness 和 provider abstraction，不应引入 LangChain 风格的泛型链式框架。

## 2. Capability Inventory

| Capability | 用户可见行为 | 关键文件路径 | 实现机制摘要 | Kiana 相关性 | 是否适合借鉴 | 风险说明 |
|---|---|---|---|---|---|---|
| Runnable/events/concurrency | 用户可组合 runnable 并流式接收 v2 events | `reference/langchain/libs/core/tests/unit_tests/runnables/test_runnable.py`, `reference/langchain/libs/core/tests/unit_tests/runnables/test_runnable_events_v2.py`, `reference/langchain/libs/core/tests/unit_tests/runnables/test_concurrency.py` | runnable 抽象有统一 invoke/stream/events/concurrency tests | `kiana-entrypoints/src/runner.rs`, `kiana-types/src/runtime.rs` | 部分 | 泛型 runnable 框架过重 |
| Agent middleware | Agent 支持 human-in-loop、tool retry、model fallback、summarization | `reference/langchain/libs/langchain_v1/langchain/agents/factory.py`, `reference/langchain/libs/langchain_v1/langchain/agents/middleware/human_in_the_loop.py`, `reference/langchain/libs/langchain_v1/langchain/agents/middleware/tool_retry.py`, `reference/langchain/libs/langchain_v1/langchain/agents/middleware/model_fallback.py`, `reference/langchain/libs/langchain_v1/langchain/agents/middleware/summarization.py` | agent factory 组合 middleware，middleware 处理策略逻辑 | `kiana-entrypoints/src/runner.rs`, `kiana-commands/src/compact.rs` | 是 | 不要把 middleware 链做成不可调试黑盒 |
| Tool schema/tests | 工具有标准 tests 和 ToolNode | `reference/langchain/libs/langchain_v1/langchain/tools/tool_node.py`, `reference/langchain/libs/core/tests/unit_tests/test_tools.py`, `reference/langchain/libs/standard-tests/langchain_tests/unit_tests/tools.py` | tools 有标准行为测试，tool node 处理调用 | `kiana-tools/src/tool.rs`, `kiana-tools/src/tool_execution.rs` | 是 | Python decorator/tool schema 不直接移植 |
| Provider/chat models | OpenAI/Anthropic/Ollama 等 chat model adapters 与 profiles | `reference/langchain/libs/partners/openai/langchain_openai/chat_models/base.py`, `reference/langchain/libs/partners/openai/langchain_openai/chat_models/azure.py`, `reference/langchain/libs/partners/anthropic/langchain_anthropic/chat_models.py`, `reference/langchain/libs/partners/ollama/langchain_ollama/chat_models.py`, `reference/langchain/libs/model-profiles/README.md` | provider adapter 与 model profile 分离 | `kiana-services/src/api`, `kiana-commands/src/model.rs` | 是 | provider APIs 时间敏感，需官方 docs/fixtures 跟进 |
| Standard tests | 集成/单元标准测试约束 chat model behavior | `reference/langchain/libs/standard-tests/README.md`, `reference/langchain/libs/standard-tests/langchain_tests/integration_tests/chat_models.py`, `reference/langchain/libs/standard-tests/langchain_tests/unit_tests/chat_models.py` | partner integrations 通过统一标准测试 | `kiana-services/src/api` | 是 | Live integration 默认不跑 |
| RAG/retrievers/vectorstores | retriever/vectorstore 行为有 core tests 和 provider implementations | `reference/langchain/libs/core/tests/unit_tests/test_retrievers.py`, `reference/langchain/libs/core/tests/unit_tests/vectorstores/test_in_memory.py`, `reference/langchain/libs/partners/chroma/langchain_chroma/vectorstores.py`, `reference/langchain/libs/langchain/langchain_classic/vectorstores/faiss.py` | retriever interface、in-memory tests、FAISS/Chroma provider | `kiana-query/src/repo_map.rs` | 部分 | Vector DB 不是 Kiana P0 |
| Output/schema/prompt tests | messages、chat prompt、pydantic output parser 有 tests | `reference/langchain/libs/core/tests/unit_tests/test_messages.py`, `reference/langchain/libs/core/tests/unit_tests/prompts/test_chat.py`, `reference/langchain/libs/core/tests/unit_tests/output_parsers/test_pydantic_parser.py` | prompt/message/parser 都有 schema tests | `kiana-types/src/runtime.rs`, `kiana-entrypoints/src/runner.rs` | 是 | Pydantic 不适用于 Rust，借测试形态 |
| SSRF/security | 网络请求策略有单元测试 | `reference/langchain/libs/core/tests/unit_tests/test_ssrf_protection.py`, `reference/langchain/libs/core/tests/unit_tests/test_ssrf_policy_transport.py` | SSRF protection 和 policy transport 独立测试 | `kiana-services/src/network_policy.rs` | 是 | Kiana 需覆盖 MCP/http/file URL 场景 |

## 3. 值得概念性借鉴的实现模式

### Pattern: Standard tests for provider integrations

来源文件：
- `reference/langchain/libs/standard-tests/README.md`
- `reference/langchain/libs/standard-tests/langchain_tests/unit_tests/chat_models.py`
- `reference/langchain/libs/standard-tests/langchain_tests/integration_tests/chat_models.py`

机制摘要：
- 每个 provider adapter 必须通过同一组行为测试。
- unit 和 integration tests 分离。
- 标准测试定义最小兼容合同。

Kiana 可借鉴方式：
- 为 `kiana-services/src/api` 定义 provider adapter standard tests。
- fake provider 跑 unit，live provider 只在 opt-in 时跑 integration。

不应该照搬的部分：
- 不使用 LangChain 的 BaseChatModel API。
- 不把 Python test harness 移植到 Rust。

### Pattern: Agent middleware as explicit policies

来源文件：
- `reference/langchain/libs/langchain_v1/langchain/agents/middleware/human_in_the_loop.py`
- `reference/langchain/libs/langchain_v1/langchain/agents/middleware/tool_retry.py`
- `reference/langchain/libs/langchain_v1/langchain/agents/middleware/model_fallback.py`
- `reference/langchain/libs/langchain_v1/langchain/agents/middleware/summarization.py`

机制摘要：
- human-in-loop、retry、fallback、summarization 都是独立策略。
- agent factory 组合策略。
- tests 可针对单一策略编写。

Kiana 可借鉴方式：
- 将 permission approval、tool retry、model fallback、auto compact 做成 runner policy，而不是散在 UI/handler。

不应该照搬的部分：
- 不做无限可组合 middleware 链。
- 不隐藏 policy 执行顺序。

## 4. Behavior Contracts to Port into Kiana

### Contract: Provider Standard Test Suite

Input:
- provider adapter
- model profile
- test mode: fake/unit/live

Decision:
- provider 是否应跑该测试。
- 缺少 feature 是 skip 还是 fail。
- live key 缺失如何处理。

Execution:
- 运行 text、streaming、tools、error mapping tests。
- 记录 provider capability report。

Output:
- standard test report。
- capability pass/fail/skip matrix。

Runtime Events:
- SessionEvent
- RuntimeError

Acceptance Tests:
- fake provider 通过所有 required tests。
- 不支持 tools 的 provider skip optional tool tests。
- adapter error 被映射为 typed Kiana error。

### Contract: Runner Policy Ordering

Input:
- runner config
- permission policy
- tool retry policy
- model fallback policy
- compaction policy

Decision:
- 每轮执行哪些 policy。
- policy 失败是否 abort。
- retry/fallback 是否消耗 budget。

Execution:
- 在固定顺序执行 policies。
- 记录 policy decision events。
- 调用 model/tool runtime。

Output:
- final result。
- policy decision trace。

Runtime Events:
- SessionEvent
- PermissionRequest
- RuntimeError
- RuntimeResult

Acceptance Tests:
- permission gate 发生在 tool retry 前。
- model fallback 不会重复已执行的 mutating tool。
- compaction 只在安全边界触发并记录 event。

## 5. Kiana Gap Analysis

| Capability | Kiana 当前状态 | 缺失行为 | 建议修改模块/crate | 测试要求 | 优先级 |
|---|---|---|---|---|---|
| Provider standard tests | provider registry 未完整 | 缺少每个 adapter 统一行为测试 | `kiana-services/src/api` | fake/unit/live split | P0 |
| Runner policies | permission/compact/checks 分散存在 | policy ordering 和 trace 不够显式 | `kiana-entrypoints/src/runner.rs`, `kiana-commands/src/compact.rs` | policy ordering fixtures | P0 |
| Network security | network_policy 有基础 | SSRF/url policy matrix 不完整 | `kiana-services/src/network_policy.rs`, `kiana-tools/src/mcp_tool.rs` | SSRF/MCP URL fixtures | P1 |
| RAG/vectorstores | repo_map 有基础 | vectorstore/retriever 不属于当前 P0 | `kiana-query/src/repo_map.rs` | offline retriever tests | P3 |

## 6. Atomic Implementation Tasks

### Task: Create Provider Standard Tests

Goal:
- 用统一测试套件约束 Kiana provider adapter 的 text、streaming、tool call 和错误映射。

Scope:
- 允许新增 `kiana-services/tests/provider_standard.rs`。
- 不允许 live tests 默认进入 CI。

Implementation Notes:
- 使用 fake provider 驱动 required tests。
- live provider tests 通过 env gate。

Acceptance Criteria:
- fake provider 通过 required standard tests。
- optional feature 可 skip 并记录 skip reason。
- provider error 统一映射到 Kiana error enum。

Tests:
- `cargo test -p kiana-services --test provider_standard`

Manual Verification:
- `KIANA_LIVE_PROVIDER_TESTS=1 cargo test -p kiana-services --test provider_standard -- --ignored`

### Task: Make Runner Policy Order Explicit

Goal:
- 固化 permission、retry、fallback、compaction、stop hook 的执行顺序。

Scope:
- 允许修改 `kiana-entrypoints/src/runner.rs`, `kiana-types/src/runtime.rs`。
- 不允许把 policy 决策放入 UI 层。

Implementation Notes:
- 定义 RunnerPolicyDecision event 或 SessionEvent payload。
- tests 使用 fake provider 和 fake tool。

Acceptance Criteria:
- permission gate 总在 tool handler 前。
- mutating tool 成功后 model fallback 不会重放该 tool。
- stop hook 命中后不再进入 fallback/retry。

Tests:
- `cargo test -p kiana-entrypoints runner_policy_order`
- `cargo test -p kiana-types runtime_event_schema`

Manual Verification:
- `kiana -p "trigger tool retry" --output-format=stream-json`
