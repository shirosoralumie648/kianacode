docs/reference-agent-audit/99-kiana-mapping.md

# Kiana 映射与实现顺序

## 目标

将参考项目中已经验证的 Agent 机制收敛为 Kiana 的单 Agent Runtime；先让一个 Agent 能够稳定完成“读取 → 修改 → 验证 → 修复 → 输出”，再复用它实现 Workflow 和 Multi-Agent 编排。

## 推荐目标架构

```text
CLI / Web / SDK / IDE
          │
  thin transport adapters
          │
  Agent application service
          │
 Session / Turn / Run store
          │
   Single Agent Runtime
      ┌───┼────┐
      │   │    │
 Model Policy EventStore
Gateway       │
      │       │
 Provider Capability Broker
              │
       shell / fs / patch / test / MCP
```

## 统一领域对象

| 对象 | 职责 | 参考来源 |
|---|---|---|
| `Session` | 长期会话、workspace、owner、fork/resume | DeepSeek, Continue, OpenCode, Letta |
| `Turn` | 一次用户任务的边界 | DeepSeek, Codex, Agno |
| `Run` | Turn 的执行实例、queue、cancel、terminal state | Crush, Agno, Goose |
| `Invocation` | 一个工具副作用请求及其审批、证据、结果 | OpenCode, DeepSeek, Agent Framework |
| `RuntimeEvent` | 唯一事实来源和可重放记录 | DeepSeek, OpenCode, Goose |
| `ApprovalRequirement` | 可暂停、可恢复的人类决策 | Agno, Cline, Letta, Crush |
| `Receipt` | 从事件和 evidence 投影出的最终报告 | Codex, Kiana current core |

## P0 — 单 Agent 契约

- 固定 `Session / Turn / Run / Invocation / Event / Receipt` 术语及其 ownership。
- 由一个 `ModelGateway` 统一处理 provider、stream、usage、retry、fallback、stop reason 和错误分类。
- 由一个 capability catalog 同时提供模型 schema、policy metadata、executor binding、approval behavior 和 result schema。
- `tool/call`、`tool/result`、`approval_required`、`run.completed/failed/cancelled` 全部纳入统一事件流。
- 对未知模型输出、未知工具和 malformed arguments 必须 fail closed，不能进入无界循环。

## P1 — 可靠运行

- 持久化活跃 run、审批、工具批次和 terminal state。
- 使用 revision/cursor 支持恢复；加载 snapshot 后，合并 cursor 之后的新事件。
- 实现取消竞态保护：先注册 active context，再发起模型或工具 I/O。
- 取消时停止新的 dispatch，drain 已开始的工具，并为尚未开始的调用写入 synthetic result。
- 当副作用结果不确定时，标记为 `result_unknown`，不能自动重复执行。
- 为 diff、测试命令、stdout/stderr、exit code 和未执行的验证生成 evidence。
- 为跨 session、跨 actor、跨 project 的 run/receipt/cancel 增加 ownership 测试。

## P2 — 产品适配器

- CLI 同步等待 terminal receipt。
- Web/IDE 使用 run ID + event cursor/SSE，而不是维护第二套 Agent loop。
- hydration 期间合并新事件，避免覆盖实时状态。
- 统一 idle、streaming、approval、applying、verifying、failed、completed UI 状态。
- 增加 clean composition e2e：真实入口、fake model、真实 broker、真实文件和测试验证。

## P3 — 编排

只有 P0/P1 稳定后再实现：

```text
AgentRuntime(role=planner)
  → AgentRuntime(role=builder)
  → AgentRuntime(role=verifier)
  → AgentRuntime(role=reviewer)
```

编排器只负责提交任务、订阅事件、处理 approval、读取 receipt 和决定下一步；不直接操作模型历史或 shell。

## 参考项目的取舍

### 深度借鉴

- DeepSeek Harness：事件溯源 Session、step boundary、流式 block、工具顺序提交。
- Crush：RunID 相关性、queue/cancel race、可靠 terminal event、Permission wait。
- OpenCode：event projector、tool part 生命周期、prompt processor、hydration merge。
- Cline：共享 Local Runtime Host、resume、abort、SDK/CLI/IDE 适配器。
- Goose：effect persistence 和状态机 checkpoint。
- Agno/Agent Framework/Letta：HITL、checkpoint、fork、stream recovery。

### 选择性借鉴

- Aider/mini-swe-agent：repo context、diff、reflection、最小 CLI 闭环。
- 12-Factor Agents：最小 Thread → Tool → Approval → State loop。
- CrewAI/AutoGen/MetaGPT/GraphExecutor：未来的 Task、Role、Graph、Team 编排。

### 不应直接复制

- Demo 中基于内存 Map 的 session。
- 通过 Markdown transcript 反推结构化 tool state。
- 未经统一 policy 管理的 shell/subprocess。
- 长期并存的双 runtime。
- 不受限制的 `while(true)` model loop。
- 未认证的 approval HTTP endpoint。
- 在 approval 之外自动执行的 install/debug/dependency path。