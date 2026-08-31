# Kiana CompanyOS 参考设计矩阵

> 本文是参考项目的能力速查表，不是 Kiana 当前能力清单。
>
> 参考材料位于 [`reference-agent-audit/`](reference-agent-audit/) 和仓库 `reference/`；它们只用于提出可验证的设计假设。采用任何模式之前，都必须在 Kiana 的 ControlPlane、EventLog、Policy、Broker 和本地 fixture 中重新验证。

> **本文速览（导读，非规范）**
>
> - **讲什么**：26 个外部 Agent 项目"学什么、不学什么"的速查表——每项能力的首选参考项目、可吸收的具体设计、Kiana 的边界约束，以及明确排除的模式（自由消息总线、无限 Swarm 等）。
> - **回答的问题**："这个功能别人是怎么做的、我们学哪部分、坚决不学哪部分。"
> - **注意**：参考项目里存在某个实现，不等于 Kiana 已实现或应该照抄。
> - **什么时候读**：设计新能力前找参考时；配合 [`reference-agent-audit/`](reference-agent-audit/README.md) 的逐项目审计使用。
>
> 术语看不懂先查 [`company-os-overview.md`](company-os-overview.md) 的白话词典。

## 1. 取舍原则

参考项目的代码量、依赖数量或 UI 完整度不能直接作为成熟度指标。优先选择满足以下条件的设计：

1. 可以收敛到一个规范 Runtime；
2. 有清晰的状态、事件和 ownership；
3. 能在本地 fake model/cassette 中重放；
4. 不要求默认放宽 shell、网络、Secret 或外部副作用；
5. 能与 `DaemonHost → ControlPlane → Broker` 集成；
6. 能说明失败、取消、Unknown、恢复、迁移和回滚。

## 2. 能力到参考项目映射

| CompanyOS 能力 | 首选参考 | 可吸收的具体设计 | Kiana 的边界 | 阶段 |
|---|---|---|---|---|
| Session/Turn/Run ledger | DeepSeek Harness | step boundary、事件追加、tool result 回灌 | 事件必须进入 Kiana EventLog，不能另起 runtime | P0 |
| Event projector/hydration | OpenCode | event projector、session 串行化、snapshot 与 live event 合并 | UI 事件不是事实源 | P0 |
| Durable effect/recovery | Goose | effect-before-event、状态机 checkpoint、按步骤恢复 | 仍需 Kiana CAS、Receipt 和 Unknown 语义 | P0 |
| Approval continuation | Agno | 可序列化 requirement、pause/continue | Approval 绑定 actor、run、invocation digest 和 expiry | P0 |
| Approval lifecycle | Agent Framework | 原始 FunctionCall identity、approval state | 不能通过未绑定 call 的全局 approval | P0 |
| Cancellation | Crush | RunID、queued/active/terminal、cancel race 测试 | 取消未确认停止时必须是 cancelling/Unknown | P0 |
| Process-tree fencing | DeepSeek Harness | 子进程树和 timeout cleanup | 不把协作式 token 当成强制终止 | P0 |
| Runtime host | Cline | CLI/IDE/UI 共享 Local Runtime Host | 统一归入 DaemonHost，不能让 UI 授权 | P0 |
| Context assembly | Aider | repo map、任务相关上下文、输入预算 | repo map 只能检索，不能授予写权限 | P1 |
| Context provider | Continue | 多入口共享 Core、history 和 context provider | 所有入口复用同一 ControlPlane | P1 |
| History/UI 分离 | Roo Code | model history 与 UI timeline 分开，恢复补齐悬空 tool result | 不从 transcript 推断 Invocation 状态 | P1 |
| History tree | Pi | append-only JSONL tree、fork/resume/compaction | 增加 CAS、ACL、provenance 和 Receipt | P1 |
| Prompt/context compression | OpenCode、Goose | context processor、checkpoint、重试和压缩边界 | compaction 不覆盖原始事件 | P1 |
| Prompt cache | Claude API cache pattern | stable prefix、deterministic tool order、usage telemetry | 当前无真实 Provider 命中证明 | P1 |
| Tool runtime | DeepSeek Harness | Central ToolRuntime、schema/approval/执行集中 | 搜索、Policy、Broker、Executor 仍分层 | P1 |
| Tool permission | OpenCode | allow/deny/ask、tool lifecycle | 最终 verdict 由 ControlPlane 产生 | P1 |
| Extension/MCP host | Cline、Goose | host/runtime 分离、extension/tool approval | 当前以 stdio MCP 为边界，HTTP MCP 不支持 | P1 |
| Tool schema/approval | Agent Framework | typed schema、调用 identity 绑定 | schema hash drift 必须使相关 approval 失效 | P1 |
| Typed graph | Pydantic AI | Graph、typed state、validate-before-defer | 领域事件和 wire schema 由 Kiana 定义 | P2 |
| Event-driven workflow | CrewAI | Flow 与 Crew 分离、checkpoint | 不吞掉 checkpoint/handler failure | P2 |
| YAML/DAG process | Archon | fresh context、确定性节点、人工 Gate、worktree | YAML 是配置，不是权限和事实源 | P2 |
| Workflow graph | ChatDev 2 / MacNet | node 可连接 memory/human/tool | 不复用共享 ChatChain 作为状态 | P2 |
| Runtime/team separation | AutoGen | RequestToSpeak、progress ledger、有界 turns | 不采用无限 group chat | P2 |
| Role-based team | MetaGPT | PM/Architect/Engineer/QA 角色与工件 | 不采用全员广播 Environment | P3 |
| Directed handoff | Agency Swarm | communication flows、handoff 与 orchestrator-worker | 使用 WorkPacket/DelegationPacket，不使用自由 SendMessage | P3 |
| Fan-out/fan-in | AutoGen、Agency Swarm | 定向分工、结果聚合、max turns | 必须有 partition、budget、TTL、merge 和 retire | P3 |
| Project task state | gpt-pilot | task/command state chain | 文件与事件事务边界必须显式 | P1/P2 |
| Code repository map | Aider | 结构化 repo map、相关文件选择 | stale index 必须带 freshness，不能伪装最新 | P1 |
| Client replay | Letta Code、Crush | cursor、seq/epoch、terminal result 必达 | 当前 Web 不宣称 token streaming | P2/P4 |
| Golden lifecycle tests | DeepSeek、Cline、OpenCode | fake model、effect test、dispatch race | 失败/拒绝/Unknown 和成功都要覆盖 | P0 |
| Cost/usage | Agno、DeepSeek | run usage、token/cost/step 计量 | 成本不能抵消安全或证据失败 | P1 |
| Lessons/memory | MemPalace、memorix | diary、retro、lessons 写回 | 需要 ACL、purpose、retention 和 provenance | P1/P2 |

## 3. 按项目总结

### DeepSeek Harness

适合借鉴：

- 耐久 Session/Run ledger；
- step boundary；
- Central ToolRuntime；
- provider 和工具结果归一化；
- 子进程树清理；
- fake model lifecycle tests。

不直接复制：TypeScript package topology、Provider 实现和与 Kiana 不兼容的默认执行权限。

### OpenCode

适合借鉴：

- event projector；
- per-session runner；
- event hydration；
- context processor；
- permission allow/deny/ask；
- retry/compaction 边界。

不直接复制：让 TUI 或 SSE 事件成为事实源。

### Cline

适合借鉴：

- Local Runtime Host；
- CLI/IDE/UI 共享运行时；
- structured stream；
- abort/queue；
- extensions 与 host 分离。

不直接复制：多层兼容路径和 UI 私有状态。

### Goose

适合借鉴：

- effect-before-event；
- session state machine；
- checkpoint/reload；
- permission manager；
- extension 生命周期。

不直接复制：长期并行的 legacy loop。

### Crush

适合借鉴：

- RunID correlation；
- accepted/active/queued cancel；
- terminal event；
- dispatch race tests。

不直接复制：把协作式 cancel 误认为强制副作用停止。

### Agno、Agent Framework、Letta Code

适合借鉴：

- serializable requirements；
- Approval pause/resume；
- checkpoint/fork；
- cursor/reconnect；
- persisted run state。

不直接复制：云端身份、外部 SDK callback 或进程内状态替代 durable contract。

### Aider、Continue、Roo Code、Pi

适合借鉴：

- repo map；
- context budget；
- history/UI 分离；
- fork/resume/compaction；
- 本地代码交互体验。

不直接复制：Markdown/trajectory log 作为事实源、默认不受限 shell 或弱身份边界。

### Pydantic AI、CrewAI、Archon、ChatDev、AutoGen

适合借鉴：

- typed graph；
- Flow 与 Crew 分离；
- DAG/fresh context；
- 有界 RequestToSpeak；
- 人工 Gate 和确定性节点。

不直接复制：让 LLM 决定全部流程状态、共享 ChatChain、无限 GroupChat 或无边界 Agent 递归。

### MetaGPT、Agency Swarm

适合借鉴：

- 角色分工；
- artifact-driven handoff；
- directed communication flow；
- orchestrator-worker 关系。

不直接复制：全员广播、自由私聊和把自然语言消息当正式交接。

## 4. 推荐吸收顺序

```text
P0  DeepSeek + OpenCode + Goose + Crush
    先稳定 Runtime、Event、Approval、Cancel、Recovery

P1  Cline + Aider + Continue + Roo + Pi + Agno
    再稳定 Context、Memory、Cache、Checkpoint、客户端重放

P2  Pydantic AI + CrewAI + Archon
    再建立确定性 Workflow、DAG、Human Gate

P3  AutoGen + Agency Swarm + MetaGPT + ChatDev
    最后加入有界的 Swarm、Role、Handoff 和 Symposium
```

## 5. 明确排除的参考模式

下列模式即使在参考项目中存在，也不进入当前 Kiana 产品主线：

- 任意角色间的自由 `SendMessage`；
- 无上限 `while(true)` 或无限 GroupChat；
- 没有 parent、budget、TTL 和 merge contract 的自动 spawn；
- 让模型文本、网页、工具描述或 Memory 直接授予权限；
- 让 Markdown transcript、UI timeline 或缓存取代 Event Store；
- 把普通 shell 或自动安装依赖作为无条件能力；
- 在未有真实 adapter、幂等、对账和人工安全控制前实现支付、IoT 或物理动作；
- 用参考目录的代码规模推断 CompanyOS 功能完整度。

## 6. Reference 使用记录

实现任何新能力时，在实施卡中至少记录：

```text
reference_project
reference_path_or_audit
adopted_pattern
rejected_pattern
Kiana adaptation
security difference
local fixture
expected event sequence
proof_level
```
