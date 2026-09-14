# Harness 运行时设计调研（2026-09-12）

> 文档性质：研究与实现建议；产品设计和可派发步骤见 [roadmap 的 Harness 专项](roadmap.md#harness-runtime-plan)。
> 本次任务只更新文档。源码核对基线为 `db77c2485bcafecbb1da17ec57ee509ad2ee32b4` 加当日 WIP；不提升任何产品能力或证明等级。

## 1. 结论与覆盖口径

Kiana 应继续拥有自己的单 Agent 运行时：用明确的状态和输入推进模型轮次，以版本化上下文调用 Provider，把工具意图交回 ControlPlane，把获准执行结果配对放回上下文。需要补齐的是这些环节之间的时序、身份、失败和恢复契约。

本次枚举了 `reference/` **72 个非隐藏项目目录**，逐项核对可枚举文件、根 README、独立 git 身份和许可证文件位置；对直接影响 Harness 的代表项目继续阅读源码/包设计。下表明确区分重点源码和目录/README 调查，**不声称逐行审计了所有项目，也没有运行参考项目**。隐藏的 `.claude-flow/` 与 `agentdb.rvf`、其 lock、`ruvector.db` 是运行数据；`COMPANYOS-REFERENCES.md` 是旧索引，均不计为项目实现。

相同提交的大小写/重复目录不算独立佐证。README 定位变化以实际内容记录，不能沿用旧审计的路径和结论。尤其是 OpenHands/Archon/Letta，当前本地树与过去常见的同名项目描述不完全一致。

下面的设计是对 Kiana 的建议，不是“参考项目都已经证明了这个方案”。其他项目内置的直接工具执行、自动提交、自改权限或宽松恢复策略，不因它们存在就适用于 Kiana。

## 2. 关键一手源码与可借鉴机制

| 来源与已读位置 | 观察到的机制 | Kiana 的落点与调整 |
|---|---|---|
| [DeepSeek agent-loop](../reference/deepseek-harness/packages/core/agent-loop/README.md)、[tool-calls.ts](../reference/deepseek-harness/packages/core/agent-loop/src/tool-calls.ts)、[inbox.ts](../reference/deepseek-harness/packages/core/agent-loop/src/inbox.ts) | turn/step 分层，两类待输入；工具 exclusive 屏障与有界并行池；取消后合成未派发结果 | Harness 管输入和工具意图；排队、批次身份、结果记录由 ControlPlane 接线，不能直接采用参考项目的工具 executor |
| [Codex turn.rs](../reference/codex/codex-rs/core/src/session/turn.rs)、[orchestrator.rs](../reference/codex/codex-rs/core/src/tools/orchestrator.rs)、[parallel.rs](../reference/codex/codex-rs/core/src/tools/parallel.rs) | 检查待输入再决定结束；保留工具被公布时的 StepContext；审批/沙箱/尝试集中管理，独占工具与可并行工具分开 | 每 step 固定上下文/工具目录/模型路由；实际 dispatch 再检验有效授权。参考项目的 sandbox retry 缓存不能替代 Kiana 的审批绑定 |
| [Pi agent-loop.ts](../reference/pi/packages/agent/src/agent-loop.ts)、[runtime/reducer.ts](../reference/pi/packages/agent/src/harness/runtime/reducer.ts) | follow-up 外循环、steering 内循环；到模型边界才转消息；截断的工具批次不执行 | 明确 Continue/Steer/Inject；完整模型响应校验在工具执行前；同一个状态驱动器服务所有入口 |
| [Pi tool-durability 设计](../reference/pi/packages/agent/docs/tool-durability.md) | 提议把外部结果已就绪与按模型顺序放置分开；说明后续调用先完成时的崩溃窗口 | 采用该问题分析：结果按真实完成顺序立即落账；上下文按声明顺序投影。此处引用的是设计，不把其中全部目标标为已实现 |
| [Goose state_machine](../reference/goose/crates/goose/src/agents/state_machine/mod.rs) | 将 LLM、retry、steer、approval、compaction、stop hook 拆成有序 operation；有显式环境开关 | 用小型职责模块拆解 Harness，避免万能 middleware；不把参考的可选状态机误当唯一默认入口 |
| [OpenCode processor](../reference/opencode/packages/opencode/src/session/processor.ts)、[compaction](../reference/opencode/packages/opencode/src/session/compaction.ts) | 处理器返回 stop/continue/compact；维护工具 part 状态、结果结束和 snapshot | 把“模型轮次完成”与“任务验收完成”分开；截断/审批/等待不可压成普通完成 |
| [Grok Agent](../reference/grok-build/crates/codegen/xai-grok-agent/src/agent.rs) | 构造后的定义、PromptContext 与 ToolBridge 分工；completion requirement 和 compaction policy 是明确对象 | PromptBundle、模型能力和完成要求版本化；业务验收交还 CompanyOS，不在 Harness 自行批准 |
| [Crush agent.go](../reference/crush/internal/agent/agent.go) | accepted sequence 区分取消前后入队；被取消的排队 RunID 也有结束通知 | 输入 receipt 与 turn identity；用户不应永远等待一个已被队列清除的请求。该树 LICENSE 是 FSL-1.1-MIT，不按 MIT 简称处理 |
| [Roo Task.ts](../reference/roo-code/src/core/task/Task.ts) | history 持久消息和 API 合并视图分开；signature 与所属消息关联；避免孤立工具结果 | 保存结构化 item 和正确配对；Kiana 应从事实派生兼容视图，不用补造“工具已执行”来修历史 |
| [Aider history.py](../reference/aider/aider/history.py)、[base_coder.py](../reference/aider/aider/coders/base_coder.py) | 真实摘要、保留近端历史；lint/test 反馈和有界修复循环 | 压缩必须保留目标/进度/证据；验证命令仍走 Broker；不复制默认自动 git 行为 |
| [Mini-SWE default.py](../reference/mini-swe-agent/src/minisweagent/agents/default.py) | 小型模型—行动—观测循环；格式错误、调用次数和成本分开计数，保存 trajectory | 保持内核简单；普通工具失败可反馈，协议破损与权限拒绝有更严格的分类 |
| [Agents SDK run_loop](../reference/openai-agents-python/src/agents/run_internal/run_loop.py)、[approvals](../reference/openai-agents-python/src/agents/run_internal/approvals.py)、[RunState](../reference/openai-agents-python/src/agents/run_state.py) | NextStepRunAgain/FinalOutput/Interruption/Handoff；审批占位不当工具结果提交 | 等审批是暂停，恢复带回原调用；最终输出单独校验；Kiana 的 Handoff 仍受 WorkPacket/Cell 管理 |
| [Pydantic AI agent graph](../reference/pydantic-ai/pydantic_ai_slim/pydantic_ai/_agent_graph.py) | 节点化输入/模型/工具处理，deferred tool result，输出校验重试预算 | Provider、工具、结构化输出分别拥有错误分类和重试额度；重试不重跑先前副作用 |
| [LangGraph _loop](../reference/langgraph/libs/langgraph/langgraph/pregel/_loop.py)、[_retry](../reference/langgraph/libs/langgraph/langgraph/pregel/_retry.py) | checkpoint 与执行任务分离；attempt 有独立超时、进度和取消边界 | checkpoint 只保存可序列化状态；ID、时钟和调度输入可注入，便于确定性故障测试 |
| [Temporal SDK README](../reference/temporal-sdk-python/README.md) | 确定性 workflow 与外部 activity 分离；恢复依赖已记录的历史 | 重放不可重新请求模型、执行 shell 或调用 MCP；借鉴边界，不为单机 Harness 引入完整 Temporal 服务 |
| [12-factor 章节](../reference/12-factor-agents/content/) | 显式掌握上下文、控制流、暂停恢复与错误反馈 | 作为实现取舍的检查方法；不是产品完成证明 |

## 3. 补充调查的外部项目和官方资料

以下页面于 2026-09-12 实际检索/打开。在线文档和 `main` 源码会变化；正式实现前应固定版本并执行本仓库测试。

| 资料 | 已核实的要点 | 本计划中的使用范围 |
|---|---|---|
| [Codex App Server](https://learn.chatgpt.com/docs/app-server) | thread/turn/item 分层；`turn/steer` 绑定 active turn，不能同时修改 model/cwd/sandbox；interrupt 与 turn 终态分开 | 运行中输入和 UI 投影契约；不引入 Codex server 作为另一个执行器 |
| [OpenAI Compaction](https://developers.openai.com/api/docs/guides/compaction) | provider compact item 是不透明状态；显式 compact 返回窗口按协议整体接续 | 预留 provider 原生 compact adapter；本地结构化摘要仍是通用实现，不能把不透明 item 当权限或可读摘要 |
| [Gemini CLI scheduler types](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/scheduler/types.ts)（读取 [raw 源码](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/scheduler/types.ts)） | 工具状态区分 validating/scheduled/executing/awaiting_approval 与 success/error/cancelled；request/response/live output 分开 | 补齐 Kiana 工具调度状态；不是对整个 Gemini CLI 的源码审计。旧 architecture URL 已失效/重定向，未据其下结论 |
| [OpenHands SDK Agent architecture](https://docs.openhands.dev/sdk/arch/agent) | 按 step 读取事件视图，处理 pending actions、condensation、模型调用、确认与 observation | 验证可恢复状态与 Agent 推进逻辑分层；它的直接工具执行模式不直接采用 |
| [Deep Agents context engineering](https://docs.langchain.com/oss/python/deepagents/context-engineering) | 大型工具输入/输出可移到文件后引用；旧历史做真正的摘要；工具 schema 也占上下文 | 先限制/外置结果，再做摘要；取回结果使用 Kiana 的受控能力 |
| [LangGraph persistence](https://docs.langchain.com/oss/python/langgraph/persistence) | checkpoint、thread、pending writes 与 replay 支撑暂停恢复 | 将已完成工具结果的落盘和上下文展示顺序分开；不声称任意外部副作用 exactly once |
| [Anthropic：Effective harnesses for long-running agents](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents)（2025-11-26） | 单靠 compaction 不足；增量任务、结构化进度和真实功能验证改善跨窗口接续 | Harness 输出可引用的进度和验证证据；自动提交示例不作为 Kiana 的授权依据 |
| [Anthropic：Harness design for long-running application development](https://www.anthropic.com/engineering/harness-design-long-running-apps)（2026-03-24） | planner/evaluator 有不同职责；模型能力变化后应重新评估额外流程的收益 | 把验证门做成可配置契约，用固定评测集衡量收益；不强制每个简单任务都额外运行评审模型 |

## 4. Kiana 源码事实与待验证问题

以下是冻结到临时副本后的 source 观察，**不是运行失败复现**。实施 agent 应先在其最新代码上补聚焦用例；已修复的项只补缺失验收，不重做实现。

| 观察 | 代码位置 | 后续处理 |
|---|---|---|
| `ActiveRun` 中存消息、pending_tools、steps、Instant；模型 await 期间移出 runs，in_flight 仅保留取消状态 | `harness.rs::model_step_in_flight`、`take_run`、`insert_next_step` | 在模型流期间接收 steering 的产品用例；稳定 mailbox 不能依赖 ActiveRun 在 map 中 |
| `Continue` 复用 RunId，重置步数和 wall-time；Completed 后仍保留对象 | `continue_run`、`store_unless_terminal` | 与 ControlPlane 专项对齐：新语义 Continue 创建 Turn/Run，Resume 保持原 Run；旧 v1 同 RunId 行为作为显式兼容合同 |
| `ModelOutput.stop_reason` 是字符串元数据；无工具且无 next-step 时直接 Completed | `model.rs`、`harness.rs::model_step` | 区分 end/length/refusal/cancel/error；文本增量不能直接证明完成 |
| compact 保留 system、最近 user，再写 `(no summary available)`；旧 Assistant/Tool 被移除 | `compact.rs::build_compacted_history` | 替换成带来源的真摘要并保留近期完整工具组，验证恢复任务能力 |
| 预检查已构造 CapabilityRequest，emit 阶段重新映射并更新首个 request ID | `model_step`、`emit_tool_request` | 构造一次并固定 invocation/request 身份；队列、checkpoint、账本共享同一身份 |
| live 结果只按 pending_tools 首项处理；checkpoint 会校验重复 call ID，但正常模型输出路径尚需独立核验 | `on_capability_result`、`restore` | 补全重复/空/跨 step call ID、乱序/重复结果和不会破坏合法 pending 的断言 |
| wall-time 在模型请求前检查；Provider 自身另有 timeout/retry | `model_step`、`model_client.rs` | 统一 deadline 传播至 silent stream、重试等待、工具、MCP；不能仅测两次模型调用之间超时 |
| WIP 已有 PromptBundle、TokenBudget、ModelTurn 记录、HarnessCheckpoint、core recovery、ToolCancelled | 本次快照列出的 domain/runner/core 文件 | 当作可复用起点；端到端接线、事务边界、缺失字段与 crash window 逐个验收 |
| checkpoint 未保存 Inbox；restore 构造空 inbox | `HarnessCheckpoint`、`restore` | 输入落账和 claim 游标进快照，证明崩溃前接收的消息不会丢失/重复消费 |
| `RunnerEvent` 的同步 sink 和返回 Vec 并存；core 聚合 delta 后记账，stream wrapper 另外转发展示 | `EventEmitter`、`core::drive_run`、`run_stream.rs` | 兼容模式共用一个 reducer，明确事实提交、临时 delta 和最终投影的顺序/去重 |

文档冲突也需要保留记录：入门 overview 和部分源注释仍写“live provider 不支持”，但 `CURRENT_STATUS.md` 已有 2026-09-09 的单 DeepSeek 模型 live 证据；roadmap 现有部分冻结表与用户本轮“冲突旧限制忽略”的授权不一致。追加设计按当前授权规划，不据旧冻结把工具目录演进等目标删除，也不把局部 live 证据扩大到三入口、多 Provider 或 durable。

## 5. 全部 reference 目录覆盖表

`源码` 表示上文列出了实际阅读的实现入口；其他行只承担目录/README 调查结论。提交是本地检出的版本，不声称是上游最新；版本缺失明示，不使用 Kiana 父仓库 HEAD 冒充。

### 运行时

| 项目 | 本地提交 | 调查结果与用途 |
|---|---|---|
| [codex](../reference/codex/) | `d6489472f3c1` | 重点源码：Turn/Step、工具 orchestrator、并行屏障、上下文投影。 |
| [deepseek-harness](../reference/deepseek-harness/) | `c389f96bf3a9` | 重点源码和包文档：turn/step、两级 inbox、调用成对记录、有界工具池。 |
| [pi](../reference/pi/) | `96617628e852` | 重点源码与设计文档：内外循环、消息转换、lane 投影、结果完成与上下文放置分离。 |
| [goose](../reference/goose/) | `5e90925962f0` | 重点源码：有序 operation 管线；state_machine 本身受环境开关控制，不能当全局默认行为。 |
| [opencode](../reference/opencode/) | `d6855b6b47a8` | 重点源码：processor、stop/continue/compact、工具状态与 snapshot。 |
| [grok-build](../reference/grok-build/) | `72a61251fcff` | 重点源码：不可变 AgentDefinition/PromptContext 与运行时 ToolBridge 分工。 |
| [crush](../reference/crush/) | `563d658bccb5` | 重点源码：accepted sequence、队列取消、每个已接收 RunID 的结束通知。LICENSE 当前为 FSL-1.1-MIT。 |
| [cline](../reference/cline/) | `fc28a5fe3331` | 目录与 README 核对；当前树已含 apps/sdk，旧 src/core/task 路径不适用，不引用旧路径作源码证据。 |
| [roo-code](../reference/roo-code/) | `b867ec914575` | 重点源码：工具配对、history 存储与 API 转换分离、thought signature 关联。 |
| [Roo-Code](../reference/Roo-Code/) | `b867ec914575` | 与 roo-code 同一提交，重复目录不算第二份独立实现证据。 |
| [aider](../reference/aider/) | `5dc9490bb35f` | 重点源码：repo map、真实摘要、受限 lint/test 反馈循环。 |
| [mini-swe-agent](../reference/mini-swe-agent/) | `04d809ceab9d` | 重点源码：最小 query/execute/observe 循环、格式错误次数、trajectory。 |
| [letta-code](../reference/letta-code/) | `6bc41be9f4a9` | README/目录：状态型 Agent、记忆与技能；模型自改运行配置不直接移植。 |
| [continue](../reference/continue/) | `5522c6f44ca0` | README 标记仓库只读维护状态；作为历史上下文/编辑器接线材料。 |
| [strix](../reference/strix/) | `52b19233477a` | 目录/README：专项安全任务 Agent；只借鉴隔离和验证需求，不扩展 Kiana 任务域。 |

### 框架与编排

| 项目 | 本地提交 | 调查结果与用途 |
|---|---|---|
| [openai-agents-python](../reference/openai-agents-python/) | `f355af660416` | 重点源码：NextStep 分类、RunState、审批 interruption 与输出校验。 |
| [pydantic-ai](../reference/pydantic-ai/) | `62f1e8302a35` | 重点源码：UserPrompt/ModelRequest 节点、deferred result、输出重试预算。 |
| [langgraph](../reference/langgraph/) | `81bf17b23123` | 重点源码：Pregel 调度、checkpoint、attempt timeout/重试边界。 |
| [langchain](../reference/langchain/) | `e670c7a03ba3` | 目录/README：模型、消息与 middleware 接口；Deep Agents 另查官方文档。 |
| [adk-python](../reference/adk-python/) | `b0180620f4c2` | 目录/README：Runner、Session、Event 分层；编排留在 CompanyOS。 |
| [agent-framework](../reference/agent-framework/) | `aea4dc221e97` | 目录/README：Agent 与 workflow 的职责划分、类型化事件接口。 |
| [autogen](../reference/autogen/) | `027ecf0a379b` | 目录/README：消息驱动与终止条件；README 指向维护模式/后继框架。 |
| [agno](../reference/agno/) | `f974c175c6f5` | 目录/README：Agent/session/storage 抽象；不引入第二套状态权威。 |
| [crewAI](../reference/crewAI/) | `34199c21b724` | 目录/README：role/task/process；对应 CompanyOS，不塞进单 Agent 内循环。 |
| [agency-swarm](../reference/agency-swarm/) | `5cd5a0d9c4ad` | 目录/README：在 Agents SDK 上组织角色和通信；对应受控子任务接缝。 |
| [MetaGPT](../reference/MetaGPT/) | `11cdf466d042` | 目录/README：角色、产物和交接流程。 |
| [ChatDev](../reference/ChatDev/) | `4fb2db0ea903` | 目录/README：角色协作与流程定义；不等于 Harness 调度器。 |
| [gpt-pilot](../reference/gpt-pilot/) | `9b763fdaf002` | 目录/README：增量开发/验证方法；未运行仓库代码或安装依赖。 |

### 运行环境与任务协调

| 项目 | 本地提交 | 调查结果与用途 |
|---|---|---|
| [OpenHands](../reference/OpenHands/) | `f7fb0c4b21f5` | 当前 README 是 Agent Canvas；真正循环另查 software-agent-sdk 官方资料。 |
| [Archon-Knowledge](../reference/Archon-Knowledge/) | `fa2740050f18` | 当前 README 是 YAML coding workflow engine，不能再按旧知识库定位。 |
| [container-use](../reference/container-use/) | `2e43e625e952` | 目录/README：独立执行环境、工作区与环境生命周期。 |
| [orca](../reference/orca/) | `12f53da542d0` | 目录/README：多 agent/worktree 的 UI 与调度。 |
| [emdash](../reference/emdash/) | `c811c072b342` | 目录/README：桌面协调与工作区选择。 |
| [herdr](../reference/herdr/) | `9e01168b140c` | 目录/README：多 agent 管理；用于入口与隔离需求对照。 |
| [gastown](../reference/gastown/) | `649b832b7672` | 目录/README：持久任务跟踪、职责分配和进度交接。 |
| [architect-loop](../reference/architect-loop/) | `164d32c36eeb` | 目录/README：外层开发流程；Kiana 用现有 CompanyOS/Workflow 承接。 |
| [gsd-core](../reference/gsd-core/) | `c6df4e1e463c` | 目录与 loop-resolver 文件定位；计划执行属于外层任务协调。 |
| [ruflo](../reference/ruflo/) | `a295c6870315` | 目录/README 与模块定位：meta-harness、memory/hooks/swarm；未启动其工具或服务。 |
| [beads](../reference/beads/) | `c0d8da42de5f` | 目录/README：任务依赖图和持久工作跟踪。 |
| [claude-task-master](../reference/claude-task-master/) | `c0c98d367c55` | 目录/README：任务拆分与依赖，不是模型循环实现。 |

### 检索与记忆

| 项目 | 本地提交 | 调查结果与用途 |
|---|---|---|
| [Archon](../reference/Archon/) | `55ef3bc7bff2` | 当前 README 为代码依赖/影响分析 CLI，与 Archon-Knowledge 不能混同。 |
| [GitNexus](../reference/GitNexus/) | `b1d87c1f33d7` | 目录/README：代码图与影响分析，供 ContextPlan 按需取材。 |
| [graphify](../reference/graphify/) | `67f99bd0059d` | 目录/README：知识图抽取与检索。 |
| [graphiti](../reference/graphiti/) | `b943c9e8486c` | 目录/README：带时间的知识关系。 |
| [llama-index](../reference/llama-index/) | `d2ac544a27c7` | 目录/README：文档索引与检索；返回值需保留来源/权限。 |
| [mem0](../reference/mem0/) | `dae67f74f5cc` | 目录/README：长时记忆；不采用 README 的效果数字作为 Kiana 证据。 |
| [memorix](../reference/memorix/) | `3a5a3c700e4d` | 目录/README：跨工具项目记忆；保留项目身份和数据准入边界。 |
| [MemPalace](../reference/MemPalace/) | `000524b111e7` | 目录/README：本地记忆和 MCP；大规模记忆通过检索进入上下文。 |
| [claude-memory](../reference/claude-memory/) | `1f1c13c981a7` | 目录/README：跨 session 提取/索引/回忆。 |
| [claude-mem-candidate](../reference/claude-mem-candidate/) | `1f1c13c981a7` | 与 claude-memory 同一提交，视作同源材料。 |
| [letta](../reference/letta/) | `4511fa0bc91f` | 当前仅 12 个可枚举文件；不据此推断包含完整旧版 Letta 服务端。 |
| [letta-oss](../reference/letta-oss/) | `4511fa0bc91f` | 与 letta 同一提交且同样为小型树，不能当独立服务端源码证据。 |

### 方法与扩展资源

| 项目 | 本地提交 | 调查结果与用途 |
|---|---|---|
| [12-factor-agents](../reference/12-factor-agents/) | `d20c728368bf` | 目录与章节：显式控制流、暂停恢复、错误反馈、reducer 方法。 |
| [ECC](../reference/ECC/) | `5064474d4d76` | 目录/README：扩展、技能、hooks 配置集合。 |
| [everything-claude-code](../reference/everything-claude-code/) | `432485ba6b92` | 较早配置集合；与 ECC 提交不同，不混称同一版本。 |
| [get-shit-done](../reference/get-shit-done/) | `bdcaab2c752d` | 目录/README：任务切片、验证、交接，映射到外层计划。 |
| [OpenSpec](../reference/OpenSpec/) | `e062b9572be9` | 目录/README：需求变更与验收产物。 |
| [spec-kit](../reference/spec-kit/) | `4a7341a93d94` | 目录/README：规格、计划、任务和验证链。 |
| [superpowers](../reference/superpowers/) | `b36e0829c6d0` | 目录/README：开发方法与组合技能，不能获得运行时授权。 |
| [planning-with-files](../reference/planning-with-files/) | `0d21b6c4aa5f` | 目录/README：文件式计划/进度/发现；Kiana 由事件派生这些视图。 |
| [gstack](../reference/gstack/) | `0530392821c2` | 目录/README：角色技能与评测工作方法。 |
| [pm-skills](../reference/pm-skills/) | `a5115727700b` | 目录/README：规划材料；只进入对应角色的上下文。 |
| [skills](../reference/skills/) | `3cca18b368ae` | 目录/README：可组合技能，不是工具权限来源。 |
| [awesome-agent-skills](../reference/awesome-agent-skills/) | `8873794bcb26` | 技能链接目录；不是可执行 Harness。 |
| [ai-coding-guide](../reference/ai-coding-guide/) | `d187dbdb83fa` | 教程材料，发现线索后回到一手代码/文档核验。 |

### 协议、耐久执行与评测

| 项目 | 本地提交 | 调查结果与用途 |
|---|---|---|
| [a2a](../reference/a2a/) | `98853be376c8` | 目录/README：跨 agent 的 task/message/artifact 协议；不直接扩展内部权限。 |
| [mcp-servers](../reference/mcp-servers/) | `d73f99efbfd4` | 目录/README：server/tool/result 参考；具体 server 仍需独立信任和测试。 |
| [temporal-sdk-python](../reference/temporal-sdk-python/) | `22a9e41fd857` | README 与模块定位：确定性 workflow 与外部 activity 分离；不要求引入 Temporal 服务。 |
| [promptfoo-full](../reference/promptfoo-full/) | 无独立提交证据 | 本次 rg 无可枚举项目文件；git 顶层回退到宿主仓库，不能使用宿主 HEAD 冒充其版本。 |

### 还原树与来源待核材料

| 项目 | 本地提交 | 调查结果与用途 |
|---|---|---|
| [claude-code-main (2)](../reference/claude-code-main%20%282%29/) | 无独立提交证据 | 目录登记；无独立 git/根许可证证据，只用于材料索引。 |
| [claude-code-rev-main](../reference/claude-code-rev-main/) | 无独立提交证据 | README 自述还原树；无独立 git/根许可证证据，不复制源码。 |
| [claude-code-rust](../reference/claude-code-rust/) | `4b87a363fd20` | README 自述包含 TS 还原源码；未把整个树当作来源清晰的可复用实现。 |

## 6. 本次研究证据与限制

```text
source_snapshot: db77c2485bcafecbb1da17ec57ee509ad2ee32b4 + WIP source copies captured 2026-09-12T08:44:17Z; file digests below
worktree_status: existing multi-crate WIP and modified roadmap before this task; this task adds this report and appends roadmap only
command_argv:
  git rev-parse HEAD
  git status --short
  rg --files (scoped per reference project; no project commands executed)
  git -C reference/<project> rev-parse --show-toplevel
  git -C reference/<project> rev-parse HEAD
  rg -n / sed -n / cat (source locations identified above)
  python3 (capture source digests, inventory 72 directories, validate documentation)
  python3 /tmp/kiana-harness-research-20260912/validate_docs.py --preview
  python3 /tmp/kiana-harness-research-20260912/validate_docs.py --written
  git diff --check -- docs/roadmap.md docs/harness-runtime-research.md
  web search/open/find (official pages listed in section 3)
cwd/environment: Kiana workspace; Linux; repository research read-only; temporary snapshots under /tmp/kiana-harness-research-20260912
fixture/cassette: none; planned test fixtures in roadmap are not executed evidence
exit_code: successful source reads/inventory=0; missing/relocated old paths and empty searches reported then corrected or marked unavailable; online architecture redirect not counted as source evidence
status change: none; all new H steps are planned work, not implemented claims
proof-level change: none; research only
limitations: no Cargo tests/build, no upstream checkout update, no execution of reference code, no CI or live-provider validation; WIP may continue changing; whole-directory survey is not a complete line-by-line audit; proposed APIs/test names must be reconciled with implementation before acceptance
reviewer: Codex primary agent self-review; no independent runtime or human acceptance
```

文档校验范围：72 个非隐藏项目目录、36 个 H 步骤、120 个拟定验收名、113 条步骤依赖无环、56 个关联的现有 P 单元、18 个端到端场景，以及本地引用和追加前正文的逐字节保留。原 roadmap 的 74 个 P 单元与已追加的 ControlPlane 专项保留；这些静态检查不代表拟定验收测试已运行。一次性校验脚本与回执保存在上述 `/tmp` 研究目录，属于本次会话材料，不是仓库的永久工具。

关键本地文件的 SHA-256（本表固定研究时的 WIP，不要求后续实现保持相同内容）：

| 文件 | SHA-256 |
|---|---|
| [docs/roadmap.md](../docs/roadmap.md) | `ab564fd2e416fc8f9d352a23f001fca1cd8e4cc76d220aeb779af8683987fedb` |
| [docs/module-map.md](../docs/module-map.md) | `2799a66be6ff6cf444dd8deb45fe039b5a3b0ba31565ad37d5e6d8e166985c8b` |
| [CURRENT_STATUS.md](../CURRENT_STATUS.md) | `e8e38a06319851500945d4f838cc164a29c5b0266cb0fcad0940ff1042d94df5` |
| [kiana-runner/src/harness.rs](../kiana-runner/src/harness.rs) | `3e8e2950d07c1fc8095e55ed6766fb70900991e11ed15736587d596028dcb720` |
| [kiana-runner/src/model.rs](../kiana-runner/src/model.rs) | `aa7dee5053343968450849795c4b0a35e61ddbec6836f4bb93ff9e2f3b4cd53f` |
| [kiana-runner/src/compact.rs](../kiana-runner/src/compact.rs) | `bd021351b20331da36762ffcb55348856d5c1429833c3e703593ab5017325a4d` |
| [kiana-runner/src/inbox.rs](../kiana-runner/src/inbox.rs) | `403f97d22be0254a22c2b1f2dc10066a934858bbb7e64b4345735056eff0b1eb` |
| [kiana-runner/src/tools.rs](../kiana-runner/src/tools.rs) | `c9ca1dac33eeaff0ea72be958fa398e4238ccc535f45efb8d6ec25b8e6de34e8` |
| [kiana-runner-protocol/src/lib.rs](../kiana-runner-protocol/src/lib.rs) | `e721ef528e9e30989dc99d20b653ca747a3b6f77a15bda1b50021c8f63f448b6` |
| [kiana-domain/src/prompts.rs](../kiana-domain/src/prompts.rs) | `ad8202c2e2a417603043e8db2ac9eca7fb3fb97d72658fde61493da227fec1dc` |
| [kiana-domain/src/tool_catalog.rs](../kiana-domain/src/tool_catalog.rs) | `5bbe51a140c465601608339f97981260dfde824dac40164ea120cfa50edec011` |
| [kiana-core/src/lifecycle.rs](../kiana-core/src/lifecycle.rs) | `4708f4187693da2f9a91eff9938a63f87318fdd15396eb8fea7e32063d22d93c` |
| [kiana-core/src/recovery.rs](../kiana-core/src/recovery.rs) | `3b28eccde42bc1ea20b3c3b4ddafceced0f377db5c88da660a76c27a27ef4152` |
| [kiana-core/src/history.rs](../kiana-core/src/history.rs) | `a7f581fac2be14201be868be58afe953479be3c0c1692eb75e5a4a3a331922ca` |
| [kiana-core/src/capabilities.rs](../kiana-core/src/capabilities.rs) | `6e02c3e4e62a92b944dbaa26c2270785fd645d29968f84ed4d298b4bafef4869` |
| [kiana-daemon/src/model_client.rs](../kiana-daemon/src/model_client.rs) | `a025eb26c1104b571117a55b685c1efe344973f15e3df4b90c309b6cb4d9cb0e` |
| [kiana-daemon/src/run_stream.rs](../kiana-daemon/src/run_stream.rs) | `dcdfaa87f54b42c6c0cece688640ae9b0aab1d25b837cef4c3718ef669ce02dd` |
| [kiana-ports/src/lib.rs](../kiana-ports/src/lib.rs) | `bf25b2c3120977cc6a79deada3f16845edc3f41ebf374974fb214b442740434c` |

详细实施顺序、接口设计、拒绝与成功验收见 [roadmap 追加区](roadmap.md#harness-runtime-plan)。
