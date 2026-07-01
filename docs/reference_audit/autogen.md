# autogen Reference Audit

## 1. 在 Kiana 中的参考定位

`reference/autogen` 的主要参考价值是多智能体会话、team orchestration、termination condition、agent state、workbench/tool boundary 和 benchmark/eval。它不是 CLI coding agent 产品形态，Kiana 不应照搬 AutoGen 的 Python runtime 或 distributed agent abstraction；适合抽取为未来 advanced agents 的行为合同和测试场景。

## 2. Capability Inventory

| Capability | 用户可见行为 | 关键文件路径 | 实现机制摘要 | Kiana 相关性 | 是否适合借鉴 | 风险说明 |
|---|---|---|---|---|---|---|
| Chat agents | 用户可创建 assistant/user proxy agents 并收发消息 | `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/agents/_assistant_agent.py`, `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/agents/_user_proxy_agent.py`, `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/base/_chat_agent.py`, `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/messages.py` | agentchat 把 agent、message、tool call 和 response 类型分层 | future `kiana-agents`, `kiana-types/src/runtime.rs` | 部分 | 对 Kiana core 过重，先不引入多 agent runtime |
| Team orchestration | 多个 agent 可 round-robin、selector 或 swarm 协作 | `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/teams/_group_chat/_base_group_chat.py`, `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/teams/_group_chat/_round_robin_group_chat.py`, `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/teams/_group_chat/_selector_group_chat.py`, `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/teams/_group_chat/_swarm_group_chat.py` | team 管理 speaker selection、message routing 和 shared state | future advanced agents | 部分 | 当前 Kiana P0 不需要多 agent scheduler |
| Termination/state | 会话可由 max messages、text mention、handoff 等条件终止 | `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/conditions/_terminations.py`, `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/state/_states.py`, `reference/autogen/python/packages/autogen-agentchat/tests/test_group_chat.py` | termination condition 和 state snapshot 与 team runtime 分离 | `kiana-entrypoints/src/runner.rs`, future agent state | 是 | 条件名可借，具体 Python object 不借 |
| Workbench/tool boundary | tools/workbench 可封装 agent 或 team | `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/tools/_agent.py`, `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/tools/_team.py`, `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/tools/_task_runner_tool.py`, `reference/autogen/python/packages/autogen-core/tests/test_workbench.py` | agent/team 可作为工具被调用，workbench 统一 tool execution | `kiana-tools/src/tool.rs`, future subagent tool | 是 | 需要严格权限边界，避免递归工具失控 |
| Core runtime docs | 文档解释 agent runtime、消息通信和 distributed runtime | `reference/autogen/python/docs/src/user-guide/core-user-guide/framework/agent-and-agent-runtime.ipynb`, `reference/autogen/python/docs/src/user-guide/core-user-guide/framework/message-and-communication.ipynb`, `reference/autogen/python/docs/src/user-guide/core-user-guide/framework/distributed-agent-runtime.ipynb` | runtime docs 以消息总线和 distributed runtime 组织概念 | future architecture docs | 部分 | distributed runtime 暂不适合 Kiana 当前 |
| Benchmark/eval | 用户可运行 agent benchmark | `reference/autogen/python/packages/agbench/src/agbench/cli.py`, `reference/autogen/python/packages/agbench/benchmarks/HumanEval/README.md`, `reference/autogen/python/packages/autogen-agentchat/tests/test_group_chat.py` | agbench CLI 组织 benchmark，agentchat tests 断言 team 行为 | future `kiana-eval` | 是 | benchmark 数据和模型依赖要 opt-in |
| Magentic-One CLI | 示例 CLI 启动复杂 agent flow | `reference/autogen/python/packages/magentic-one-cli/src/magentic_one_cli/__main__.py`, `reference/autogen/python/packages/magentic-one-cli/src/magentic_one_cli/_m1.py` | CLI 包装 multi-agent workflow | future advanced workflows | 部分 | 不作为 Kiana core CLI 参考 |

## 3. 值得概念性借鉴的实现模式

### Pattern: Termination conditions as first-class runtime decisions

来源文件：
- `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/conditions/_terminations.py`
- `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/teams/_group_chat/_base_group_chat.py`
- `reference/autogen/python/packages/autogen-agentchat/tests/test_group_chat.py`

机制摘要：
- 终止条件独立于具体 agent。
- runtime 在每轮消息后评估 condition。
- tests 通过 team run 验证终止行为。

Kiana 可借鉴方式：
- 在 `kiana-entrypoints/src/runner.rs` 明确 max_turns、stop_hooks、tool_error_policy、budget_exhausted 等 termination reason。
- RuntimeResult 应包含 stop reason。

不应该照搬的部分：
- 不引入 AutoGen 的 async team hierarchy。
- 不让 termination condition 持有 UI state。

### Pattern: Agent/team as a tool

来源文件：
- `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/tools/_agent.py`
- `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/tools/_team.py`
- `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/tools/_task_runner_tool.py`

机制摘要：
- 一个 agent 或 team 可包装为 tool。
- 外层 runtime 把它看作普通工具调用。
- 内层执行仍有独立消息和结果。

Kiana 可借鉴方式：
- 未来 subagent 可以作为 `kiana-tools` 的一个特殊 tool。
- subagent tool 必须继承 permission profile、budget 和 cwd。

不应该照搬的部分：
- 不允许无限递归 team-as-tool。
- 不默认开启多 agent 并发。

## 4. Behavior Contracts to Port into Kiana

### Contract: Explicit Stop Reason

Input:
- runner config
- current turn state
- stop hooks
- budget/tool/error state

Decision:
- 是否继续下一轮。
- 哪个 termination reason 优先。
- 是否将 stop reason 暴露给 user/SDK。

Execution:
- 每轮 assistant/tool 后评估 stop policy。
- 记录 first terminal reason。
- 写入 RuntimeResult。

Output:
- final assistant result。
- stop_reason enum。

Runtime Events:
- SessionEvent
- ToolResult
- RuntimeError
- RuntimeResult

Acceptance Tests:
- max_turns 命中时 stop_reason=max_turns。
- stop hook 命中时不会继续执行下一轮 tool。
- tool error policy=abort 时 stop_reason=tool_error。

### Contract: Subagent Tool Boundary

Input:
- subagent task prompt
- inherited cwd
- inherited permission profile
- budget/depth limit

Decision:
- 当前 profile 是否允许 subagent。
- recursion depth 是否超过限制。
- 子任务是否可使用 mutating tools。

Execution:
- 创建 child session。
- 执行 child runner。
- 汇总 child result 给 parent tool_result。

Output:
- parent-visible tool result。
- child session id and summary。

Runtime Events:
- ToolCall
- SessionEvent
- ToolResult
- RuntimeError

Acceptance Tests:
- child session 继承 cwd 和 permission profile。
- depth limit 阻止 subagent 递归。
- parent session 只接收 summary，不内联全部 child transcript。

## 5. Kiana Gap Analysis

| Capability | Kiana 当前状态 | 缺失行为 | 建议修改模块/crate | 测试要求 | 优先级 |
|---|---|---|---|---|---|
| Stop reason | runner/runtime events 有终止结果 | stop reason enum 和优先级 contract 需明确 | `kiana-entrypoints/src/runner.rs`, `kiana-types/src/runtime.rs` | max_turns, hook, tool_error fixtures | P0 |
| Subagent boundary | 未作为核心 capability 完成 | child session、budget、permission inheritance 未定义 | future `kiana-agents`, `kiana-tools/src/tool_execution.rs` | child session isolation tests | P2 |
| Multi-agent scheduler | 当前重点是 single-agent coding loop | round-robin/selector/swarm 不存在 | future `kiana-agents` | deterministic scheduler tests | P3 |
| Eval benchmark | release smoke 有基础 | HumanEval/SWE-style replay harness 未成体系 | future `kiana-eval`, `scripts/` | offline benchmark fixtures | P2 |

## 6. Atomic Implementation Tasks

### Task: Add Runtime StopReason Contract

Goal:
- 给每次 runner 结束一个可序列化、可测试的 stop_reason。

Scope:
- 允许修改 `kiana-types/src/runtime.rs`, `kiana-entrypoints/src/runner.rs`。
- 不允许改变现有 final text 输出。

Implementation Notes:
- StopReason 可从 max_turns、user_cancelled、tool_error、stop_hook、model_stop、budget_exhausted 开始。
- RuntimeResult 包含 stop_reason。

Acceptance Criteria:
- 所有 terminal runner path 都设置 stop_reason。
- stream-json 输出 stop_reason。
- legacy consumer 未读取 stop_reason 时不受影响。

Tests:
- `cargo test -p kiana-entrypoints runner_stop_reason`
- `cargo test -p kiana-types runtime_event_schema`

Manual Verification:
- `kiana -p "loop" --max-turns 1 --output-format=stream-json`

### Task: Design Subagent Tool Contract

Goal:
- 输出一份可实现的 subagent tool contract，为未来多智能体保留边界。

Scope:
- 允许新增 `docs/subagent-tool-contract.md` 和 tests skeleton。
- 不实现完整 multi-agent scheduler。

Implementation Notes:
- 文档必须定义 child session、permission inheritance、budget/depth limit、event summary。
- 与 existing RuntimeEvent 对齐。

Acceptance Criteria:
- contract 覆盖 parent/child session id。
- contract 明确禁止默认递归无限调用。
- contract 映射到未来 `kiana-tools` tool schema。

Tests:
- 文档示例 JSON 可反序列化。
- schema fixture 可通过 `cargo test -p kiana-types`。

Manual Verification:
- Review `docs/subagent-tool-contract.md` against this audit.
