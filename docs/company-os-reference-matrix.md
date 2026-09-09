# Kiana CompanyOS 参考设计矩阵

> 本文是参考项目的能力速查表，不是 Kiana 当前能力清单。
>
> 参考材料位于 [`reference-agent-audit/`](reference-agent-audit/) 和仓库 `reference/`；它们只用于提出可验证的设计假设。采用任何模式之前，都必须在 Kiana 的 ControlPlane、EventLog、Policy、Broker 和本地 fixture 中重新验证。

> **本文速览（导读，非规范）**
>
> - **讲什么**：外部 Agent 项目"学什么、不学什么"的速查表，对照 [`reference-agent-audit/`](reference-agent-audit/README.md) 的 26 份结构化审计，以及 `reference/` 中尚未结构化审计的目录级参考——每项能力的首选参考项目、可吸收的具体设计、Kiana 的边界约束，以及明确排除的模式（自由消息总线、无限 Swarm 等）。
> - **覆盖**：§3 按项目小结并标注审计状态与最后核验日；26 份结构化审计的逐项目覆盖状态、最后核验日与基准 commit 以审计 README 的覆盖状态表为准。grok-build、temporal、beads、claude-task-master、graphiti、mem0、container-use、gastown、a2a、spec-kit、OpenSpec 等目前只做目录级参考，补审计是待办（本轮不执行）。
> - **回答的问题**："这个功能别人是怎么做的、我们学哪部分、坚决不学哪部分。"
> - **注意**：参考项目里存在某个实现，不等于 Kiana 已实现或应该照抄；**目录里有不等于已审计**。
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
| Session 内输入语义（start/steer/recover/suspend） | Codex | idle 启动、running 注入、仅 idle 恢复、suspend 转移 ownership 且不写 terminal event | 必须映射到 Kiana Run 状态机；suspend 类状态要有 terminal event 对账，不能无限等 TurnComplete | P0 |
| Event projector/hydration | OpenCode | event projector、session 串行化、snapshot 与 live event 合并 | UI 事件不是事实源 | P0 |
| Durable effect/recovery | Goose | effect-before-event、状态机 checkpoint、按步骤恢复 | 仍需 Kiana CAS、Receipt 和 Unknown 语义 | P0 |
| Approval continuation | Agno | 可序列化 requirement、pause/continue | Approval 绑定 actor、run、invocation digest 和 expiry | P0 |
| Approval lifecycle | Agent Framework | 原始 FunctionCall identity、approval state | 不能通过未绑定 call 的全局 approval | P0 |
| Event 单消费者队列 | adk-python | non-partial Event 持久化后才解除生产方等待；partial 只用于流式 | Kiana EventLog append 成功后才继续；partial 不改变持久 state | P0 |
| Approval 绑定原始调用 | adk-python | 审批校验 tool、call ID、name、args 与历史一致，只重执行同一原始调用，消费后删除 | Kiana ApprovalRequirement 绑定 canonical tool identity，不可换调用/换参数 | P0 |
| Cancellation | Crush | RunID、queued/active/terminal、cancel race 测试 | 取消未确认停止时必须是 `cancel_requested`（UI 投影名 `cancelling`）/`result_unknown` | P0 |
| 双取消模式 | openai-agents-python | immediate 与 after_turn 两种取消；cancel 后仍需 drain event 完成清理 | Kiana 提供 cancel_now / stop_after_turn 两种明确命令，UI 显示「正在收敛」 | P0 |
| Process-tree fencing | DeepSeek Harness | 子进程树和 timeout cleanup | 不把协作式 token 当成强制终止 | P0 |
| Runtime host | Cline | CLI/IDE/UI 共享 Local Runtime Host | 统一归入 DaemonHost，不能让 UI 授权 | P0 |
| Approval 位于 Core | Codex | approval policy / permission profile 是 Session/Core 状态，UI 只显示和提交 decision；带 amendment 的批准持久化 | Kiana 由 ControlPlane 强制审批，UI 不是授权边界 | P0 |
| Sandbox / permission profile | Codex | read-only / workspace-write / danger-full-access 档位与审批、profile 绑定 Session | Kiana 默认 read-only、写盘显式、工作区外 fail-closed；不引入 danger-full-access | P0 |
| Cancel / pause / stop-goal 语义分离 | OpenHands（仅 agent-canvas 前端/适配层） | Local interrupt、Cloud pause、Goal stop 含义不同，不能统一当「已取消」 | Kiana 区分 `cancel_requested`（UI 投影名 `cancelling`）/`result_unknown` 与 paused/terminal，取消未确认不得写成已停止 | P0 |
| Web 事件桥（tail preload + since 订阅） | OpenHands（仅 agent-canvas 前端/适配层） | 最近 N 条 REST preload + `resend_mode='since'` 增量订阅、事件 ID 去重、副作用前去重 | 当前 Web 仅 loopback 且不宣称 token streaming；UI metadata 不是授权事实 | P1 |
| Context assembly | Aider | repo map、任务相关上下文、输入预算 | repo map 只能检索，不能授予写权限 | P1 |
| Context provider | Continue | 多入口共享 Core、history 和 context provider | 所有入口复用同一 ControlPlane | P1 |
| History/UI 分离 | Roo Code | model history 与 UI timeline 分开，恢复补齐悬空 tool result | 不从 transcript 推断 Invocation 状态 | P1 |
| History tree | Pi | append-only JSONL tree、fork/resume/compaction | 增加 CAS、ACL、provenance 和 Receipt | P1 |
| Versioned RunState / resume | openai-agents-python | 版本化 RunState 覆盖 approval、trace、sandbox、max turns、pending input，未知版本 fail fast；resume 不重复写 tool | Kiana 规范已定义 durable `RunSnapshot` + 调用账本 + 迁移/拒绝策略（见 [`company-os-spec-index.md`](company-os-spec-index.md) §4.4）；**实现层的跨进程恢复仍为 deferred** | P1 |
| 隔离 / 检查点 | container-use、gastown | environment 状态机与「所有副作用经 environment」、checkpoint 字段、estop 熔断 | 首发状态层 + 编辑级 undo（复用 apply_patch 前置快照），文件层 shadow git 列为第二阶段；只取本地形状，不引入 Dagger / town-mail | P1 |
| Prompt/context compression | OpenCode、Goose | context processor、checkpoint、重试和压缩边界 | compaction 不覆盖原始事件 | P1 |
| Prompt cache | Claude API cache pattern | stable prefix、deterministic tool order、usage telemetry | 当前无真实 Provider 命中证明 | P1 |
| Tool runtime | DeepSeek Harness | Central ToolRuntime、schema/approval/执行集中 | 搜索、Policy、Broker、Executor 仍分层 | P1 |
| Tool permission | OpenCode | allow/deny/ask、tool lifecycle | 最终 verdict 由 ControlPlane 产生 | P1 |
| Extension/MCP host | Cline、Goose | host/runtime 分离、extension/tool approval | 当前以 stdio MCP 为边界，HTTP MCP 不支持 | P1 |
| Tool schema/approval | Agent Framework | typed schema、调用 identity 绑定 | schema hash drift 必须使相关 approval 失效 | P1 |
| 极简 agent loop / 工具解析契约 | mini-swe-agent | 小核心 + 可替换 model/environment adapter；缺调用、坏 JSON、未知工具、缺 command 都有解析测试 | Kiana 保持单一 harness 循环，工具仍经 broker；不采用无限制 `shell=True` | P2 |
| Typed graph | Pydantic AI | Graph、typed state、validate-before-defer | 领域事件和 wire schema 由 Kiana 定义 | P2 |
| Event-driven workflow | CrewAI | Flow 与 Crew 分离、checkpoint | 不吞掉 checkpoint/handler failure | P2 |
| Agent loop 设计检查表（factor 1–8） | 12-factor-agents | 结构化下一步选择、源码自有 prompt/context、统一执行状态、start-pause-resume、human-as-tool | 教育性示例，不是运行时；Kiana 仍需 ControlPlane 状态机和持久事件 | P2 |
| 审批暂停 / 恢复演示 | 12-factor-agents | divide 需审批时返回调用方，`/thread/:id/response` 恢复同一 thread | 示例端点无认证、无幂等、无重启恢复，禁止照搬 | P2 |
| YAML/DAG process | Archon | fresh context、确定性节点、人工 Gate、worktree | YAML 是配置，不是权限和事实源 | P2 |
| Workflow 确定性 / 重放 | grok-build、Temporal Python SDK | journal（req_hash + 稠密 seq + Divergence fail-closed）、history replay、可注入时钟与 logic_version 分支 | 只取本地重放语义，不引入服务端 / 第二运行时；未知版本 fail-closed；重放只做只读投影 | P0/P1 |
| Spec 工件 / validate 门禁 | spec-kit、OpenSpec | 工件模板链、ArtifactGraph（generates / requires）、validate --strict / --json、Constitution Check 门禁 | 工件图由 ControlPlane 拥有；validate 是只读门禁，不新增模型可见工具 | P1 |
| Workflow graph | ChatDev 2 / MacNet | node 可连接 memory/human/tool | 不复用共享 ChatChain 作为状态 | P2 |
| Runtime/team separation | AutoGen | RequestToSpeak、progress ledger、有界 turns | 不采用无限 group chat | P2 |
| Role-based team | MetaGPT | PM/Architect/Engineer/QA 角色与工件 | 不采用全员广播 Environment | P3 |
| Directed handoff | Agency Swarm | communication flows、handoff 与 orchestrator-worker | 使用 WorkPacket/DelegationPacket，不使用自由 SendMessage | P3 |
| 协议 / Task 状态机 | a2a | Task / TaskState / Message 的字段形状与 Kiana 状态映射 | 只取字段形状与状态映射；对外声明 push_notifications=false / streaming=false，不做远程 transport | P1 |
| Fan-out/fan-in | AutoGen、Agency Swarm | 定向分工、结果聚合、max turns | 必须有 partition、budget、TTL、merge 和 retire | P3 |
| Project task state | gpt-pilot | task/command state chain | 文件与事件事务边界必须显式 | P1/P2 |
| 任务图 / 依赖就绪 | beads、claude-task-master | 单一 `ready_packets` 谓词、blocked 不动点、环检测、claim / lease 心跳回收 | 依赖用显式 `WorkPacket.dependencies` 字段，不从 packet 文本解析；`ready_packets` 与 PathLock 写死「规划期检查 + 运行期兜底」；ready 只是查询，许可仍由 ControlPlane 产生 | P0 |
| Code repository map | Aider | 结构化 repo map、相关文件选择 | stale index 必须带 freshness，不能伪装最新 | P1 |
| Client replay | Letta Code、Crush | cursor、seq/epoch、terminal result 必达 | 当前 Web 不宣称 token streaming | P2/P4 |
| Golden lifecycle tests | DeepSeek、Cline、OpenCode | fake model、effect test、dispatch race | 失败/拒绝/Unknown 和成功都要覆盖 | P0 |
| Cost/usage | Agno、DeepSeek | run usage、token/cost/step 计量 | 成本不能抵消安全或证据失败 | P1 |
| Lessons/memory | MemPalace、memorix（仅目录级参考，未做结构化审计） | diary、retro、lessons 写回 | 需要 ACL、purpose、retention 和 provenance | P1/P2 |
| 记忆时间维度 | graphiti、mem0 | valid_from / valid_to / expired_at、新事实失效旧边并保留历史、内容 hash 去重与 ADD/UPDATE/DELETE 生命周期 | 检索分数只进排序、不参与授权；来源服务端派生、模型写默认 candidate；instance-scratch 默认可见、持久层默认不可检索 | P1 |

## 3. 按项目总结

> **审计状态与核验口径**：本节「已审计」严格指 [`reference-agent-audit/`](reference-agent-audit/README.md) 中存在结构化源码审计报告；**目录里有不等于已审计**。逐项目的覆盖状态、最后核验日与基准 commit 以审计 README 的覆盖状态表为准。维护口径（决策记录 #8）：结构化审计 + 最后核验日 + 只核对被引用路径，不做每次全量重审。
>
> - **已核验仍有效**（自审计日 2026-08-25/26 以来 0 提交）：roo-code、aider、continue、claude-code-rust、gpt-pilot、autogen、ChatDev、MetaGPT、12-factor-agents。
> - **边缘漂移，只核对被引用路径**：crush、opencode。
> - **已过时，待重审**：codex、deepseek-harness、adk-python、goose、agent-framework；顺序见审计 README 待办。
> - **目录级参考，未结构化审计**：本节的 grok-build、Temporal、beads、claude-task-master、graphiti、mem0、container-use、gastown、a2a、spec-kit、OpenSpec，只用于提出可验证假设，采用前必须回 Kiana 的 ControlPlane / EventLog / fixture 重新验证。
> - **明确排除**：`promptfoo-full` 的远程地址配置指向本仓自身，无法核对上游，不作为参考来源。

### DeepSeek Harness

适合借鉴：

- 耐久 Session/Run ledger；
- step boundary；
- Central ToolRuntime；
- provider 和工具结果归一化；
- 子进程树清理；
- fake model lifecycle tests。

不直接复制：TypeScript package topology、Provider 实现和与 Kiana 不兼容的默认执行权限。

### Codex

适合借鉴：Core 是唯一执行控制面，CLI/TUI/app-server 只是薄适配器；Thread/Turn/step 与 start/steer/recover/suspend 输入语义清晰；每 step 重捕获 context 并精确回灌工具结果；approval 与 permission/sandbox profile 位于 Session/Core，UI 只提交 decision；CancellationToken 全链传播，rollout reconstruction / rollback / flush 形成恢复边界。

不直接复制：Core/runtime 规模很大，多 transport 背压和 suspend ownership transfer 的兼容演进需要强测试；suspend 不产生 terminal event，消费者不能无限等 TurnComplete。

`coding-pack-matrix.md` §1 的 P0-WRITE / P0-SHELL / P0-SANDBOX / P0-MCP / P1-COMPACT 都标「Codex Apache-2.0 形状可借鉴」，复用形状时保留来源注释。

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

### OpenHands

注意审计边界：`reference/OpenHands` 是 `@openhands/agent-canvas` 前端/适配层，不是 OpenHands Python Agent Server 本体；服务端 model、tool executor、sandbox、persistence 和 approval enforcement 无法从当前源码验证。

适合借鉴：REST tail preload + since WebSocket 的可恢复事件桥、事件 ID 与副作用前去重、stream delta 帧批处理、conversation 切换防串台、UI metadata 与 runtime/authorization state 分开持久化。

不直接复制：Local interrupt / Cloud pause / stop-goal 语义不对称，不能统一解释为「已取消」；前端可见 session key 和前端审批 response 都不是可信执行边界，审批必须在 ControlPlane/Broker 强制。

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

### adk-python

适合借鉴：Event 是对话、工具、审批、状态、checkpoint 和 rewind 的共同事实日志；单消费者 queue 保证 non-partial Event 持久化后才继续；ToolConfirmation 绑定原始 function-call ID/name/args 并去重，审批不能替换工具或参数；temp state 只在 invocation 内共享；rewind 追加新 Event 而不删除历史。

不直接复制：Node Runtime 与旧 BaseAgent 双轨并存，Agent-level final Event 不等于 invocation 完成，InMemorySessionService 不能当生产 backend，自定义 SessionService 必须自己实现 optimistic concurrency。

### openai-agents-python

适合借鉴：流式与非流式共享 turn resolution/tool/guardrail/approval/persistence 语义；版本化 RunState 覆盖 approval、trace、sandbox、max turns、pending input，未知版本 fail fast；immediate 与 after_turn 双取消模式；approval 绑定 namespace、qualified tool key 和 call ID；guardrail 在工具副作用前有竞态保护。

不直接复制：Session 与 RunState 双层持久化的 ownership 和重复写入由宿主负责，cancel 后仍需 drain events，after_turn 不是立即停止（UI 要显示「正在收敛」），tracing 可能包含敏感数据。

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

### mini-swe-agent

适合借鉴：小型协议化架构把 agent 循环、model adapter 和 environment 清晰分离，工厂可替换 adapter 而不改循环；工具调用解析对缺调用、坏 JSON、未知工具和缺 command 都有测试；超时保留部分输出并终止 POSIX 进程组。

不直接复制：没有会话身份、反序列化或恢复路径，轨迹 JSON 只是检查工件不是可恢复会话；轨迹写入非原子；`shell=True` 加继承主机环境是无限制执行，confirm 只是交互式 UX 而不是安全边界。

### Pydantic AI、CrewAI、Archon、ChatDev、AutoGen

适合借鉴：

- typed graph；
- Flow 与 Crew 分离；
- DAG/fresh context；
- 有界 RequestToSpeak；
- 人工 Gate 和确定性节点。

不直接复制：让 LLM 决定全部流程状态、共享 ChatChain、无限 GroupChat 或无边界 Agent 递归。

### 12-factor-agents

适合借鉴：factor 1–8 作为设计检查表——结构化下一步选择、源码自有 prompt/context、统一执行状态、start-pause-resume、human-as-tool；审批门控后返回调用方、再由 response 路由恢复同一 thread 的路径清晰。

不直接复制：final 运行时是内存 Map，无持久化、恢复、取消、流式；`while(true)` 没有迭代预算；未转义字符串插值会破坏上下文完整性；HTTP 审批端点无认证、幂等和重放保护——这些正是 Kiana 要补齐的缺口。

### MetaGPT、Agency Swarm

适合借鉴：

- 角色分工；
- artifact-driven handoff；
- directed communication flow；
- orchestrator-worker 关系。

不直接复制：全员广播、自由私聊和把自然语言消息当正式交接。

### grok-build（目录级参考，未结构化审计）

适合借鉴：xai-workflow 的 journal（req_hash、稠密 seq、Divergence fail-closed 与 replay）、xai-hunk-tracker 的 Agent vs External hunk 归因、CoW / btrfs 快照与 worktree pool、Landlock / Seatbelt + 子进程 seccomp、events.jsonl 与 EventTracker。

不直接复制：其 provider、voice、dashboard、遥测与远程服务。

审计状态：目录级参考；新增 `27-grok-build.md` 结构化审计是待办（S-2，本轮不执行）。最后核验日：2026-09-08（仅确认目录与引用路径，未审计）。

### Temporal Python SDK（目录级参考，未结构化审计）

适合借鉴：本地 history-replay 语义、activity / workflow 边界、确定性限制、Replayer 差分检测、可持久化的 RetryPolicy / TimeoutPolicy。

不直接复制：服务端 / 集群运行时；Kiana 只取本地重放与策略形状。

审计状态：目录级参考；结构化审计待办（S-3，本轮不执行）。最后核验日：2026-09-08（仅确认目录与引用路径，未审计）。

### beads、claude-task-master（目录级参考，未结构化审计）

适合借鉴：beads 的单一 ready 谓词、blocked 不动点、环检测、claim / lease 冲突类型；claude-task-master 的 eligibility = 状态合法 + 全部依赖 done、DFS 找环。

不直接复制：Dolt 后端、模型驱动规划；依赖边必须来自 `WorkPacket.dependencies` 显式字段。

审计状态：目录级参考；结构化审计待办（beads 见 S-3、claude-task-master 见 S-6，本轮不执行）。最后核验日：2026-09-08（仅确认目录与引用路径，未审计）。

### graphiti、mem0（目录级参考，未结构化审计）

适合借鉴：graphiti 的 valid_at / invalid_at / expired_at 双时间与「新事实失效旧边、保留历史」；mem0 的内容 hash 去重与 ADD / UPDATE / DELETE 生命周期。

不直接复制：Neo4j / 向量库依赖、模型驱动的 add / update / delete；检索分数只进排序、来源由服务端派生。

审计状态：目录级参考；结构化审计待办（S-3，本轮不执行）。最后核验日：2026-09-08（仅确认目录与引用路径，未审计）。

### container-use、gastown（目录级参考，未结构化审计）

适合借鉴：container-use 的 environment 状态机与「所有副作用经 environment」；gastown 的 checkpoint 字段与 estop 熔断。

不直接复制：Dagger；gastown 的 mail / nudge / mayor 属于冻结的自由消息总线，只取 checkpoint / estop。

审计状态：目录级参考；结构化审计待办（container-use 见 S-3、gastown 见 S-6，本轮不执行）。最后核验日：2026-09-08（仅确认目录与引用路径，未审计）。

### a2a（目录级参考，未结构化审计）

适合借鉴：Task / TaskState / Message 的字段形状与 Kiana 状态映射。

不直接复制：push notification、streaming、SubscribeToTask 等远程传输层（冻结项）；对外声明 push_notifications=false / streaming=false，取消不写 CANCELED 终态。

审计状态：目录级参考；结构化审计待办（S-3，本轮不执行）。最后核验日：2026-09-08（仅确认目录与引用路径，未审计）。

### spec-kit、OpenSpec（目录级参考，未结构化审计）

适合借鉴：spec-kit 的工件模板链、Constitution Check 门禁；OpenSpec 的 ArtifactGraph（generates / requires）、validate --strict / --json、需求级 diff。

不直接复制：把编排权交给模型；工件图只能由 ControlPlane 拥有，validate 是只读门禁。

审计状态：目录级参考；结构化审计待办（S-3，本轮不执行）。最后核验日：2026-09-08（仅确认目录与引用路径，未审计）。

未单列小节的审计项目：`claude-code-rust`（反面教材，见 [`coding-pack-matrix.md`](coding-pack-matrix.md) §8）与 `gpt-pilot`（见 §2 的 Project task state 行）。

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
- 用参考目录的代码规模推断 CompanyOS 功能完整度；
- 把 `promptfoo-full` 当作参考来源——其远程地址配置指向本仓自身，无法核对上游，明确排除；
- 把「目录里有」当成「已审计」——审计状态只以 [`reference-agent-audit/README.md`](reference-agent-audit/README.md) 的覆盖状态表为准。

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
