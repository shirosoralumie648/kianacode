# CompanyOS 组织与业务：参考调研记录

> 日期：2026-09-12。性质：研究输入，不是 Kiana 实现证明。
> 配套：[组织与业务代码设计](../company-os-organization-business-design.md)、[roadmap 追加步骤](../roadmap.md#companyos-business-steps)。

## 1. 范围与取证方式

本次遍历 `reference/` 的 **72 个非隐藏项目目录**，核对各目录自己的 Git 根、HEAD、README、许可证文件是否存在，并建立文件索引。69 个目录能取得自身 HEAD；这不意味着有 69 个不同项目，也不意味着其工作树经过运行验证。

采用两层阅读：所有目录做定位与相关性筛选；组织、业务交接、任务图、会议和恢复相关项目进一步读取下表列出的源码或设计文件。没有逐行审计全部参考源码，没有运行、安装或更新参考项目。`reference/.claude-flow/`、`agentdb.rvf*`、`ruvector.db` 是本地工具状态，不作为产品设计证据。在线补充只采用项目官方文档、官方仓库；访问日期均为 2026-09-12。

结论的证据粒度：源码可证明“这个快照包含这种处理方式”；README/设计文档只证明项目描述了该机制。参考项目的性能数字、自治能力、可靠性承诺均不直接转写为 Kiana 的能力。

## 2. 必须纠正的旧引用

| 旧信息或容易混淆的地方 | 本次核对 | 对后续设计的影响 |
|---|---|---|
| `reference/Archon` 被当成 coleam00 的工作流引擎 | [Archon README](../../reference/Archon/README.md) 的 origin 是 `Schr0d/Archon`，定位是依赖与冲击半径分析；[Archon-Knowledge](../../reference/Archon-Knowledge/README.md) 才是 `coleam00/Archon` 的工作流项目 | 工作流引用后者，变更影响分析引用前者 |
| ChatDev 等于 CEO/CTO 共享聊天室 | 当前 [ChatDev](../../reference/ChatDev/README.md) 是 2.0 DevAll，具有图配置、DAG executor 和节点运行时 | 旧公司角色隐喻可作历史背景；当前实现参考必须指向 2.0 文件 |
| Letta、letta-oss 是两份可移植的后端实现 | 两目录 HEAD 相同，当前 README 指向 [letta-code](../../reference/letta-code/README.md)；根目录主要是项目说明和政策文件 | 不为两个迁移页各记一份“后端恢复能力” |
| Roo-Code / roo-code、claude-mem-candidate / claude-memory 是不同证据 | 每组 HEAD 和 README 摘要一致 | 目录覆盖计数保留，机制证据去重 |
| pm-skills 会议合同仍位于旧 `docs/reference/...` | 实际在 [site/src/content/docs/reference/skill-families/meeting-skills-contract.md](../../reference/pm-skills/site/src/content/docs/reference/skill-families/meeting-skills-contract.md) | 使用实际路径；它以文件名关联会议产物，Kiana 仍需要稳定对象 ID |
| `promptfoo-full` 已有源码 | 本次目录只有无有效 checkout 的 `.git` 残留，无 README/源码 | 不能写成本地评测实现已调研；评测任务须另行提供真实 fixture |
| `architect-loop` 的旧角色与审批规则仍是现状 | 当前 [DESIGN.md](../../reference/architect-loop/DESIGN.md) 描述 v5.1、GitHub issue 协调、关闭时 cohesion review，并明确存在无需审批的 timed-ruling 策略 | 只吸收上下文隔离和冻结检查；不据此推导等待超时就是人类同意 |
| `get-shit-done` README 仍是当前开发入口 | README 指向 [gsd-core](../../reference/gsd-core/README.md) | 历史目录保留；当前流程模板优先核对 gsd-core |

## 3. 与本任务直接相关的机制

| 参考及实际阅读落点 | 观察到的机制 | Kiana 的具体采用方式 | 适用限制 |
|---|---|---|---|
| [12-factor-agents：small focused agents](../../reference/12-factor-agents/content/factor-10-small-focused-agents.md) | 小 Agent 嵌入确定性软件流程 | 业务推进由 process manager 计算；每个角色 Run 有单一输入/输出合同 | 这是设计原则，不是 Kiana 运行证据 |
| [architect-loop DESIGN](../../reference/architect-loop/DESIGN.md) | fresh worker、冻结检查、原始报告、独立收束评审 | 分离 Plan、Build、Review；Evidence 包带输入与检查版本 | 不采用自动批准、自动发布或 issue 状态代替业务验收 |
| [pm-skills 会议合同](../../reference/pm-skills/site/src/content/docs/reference/skill-families/meeting-skills-contract.md)、[验收模板](../../reference/pm-skills/skills/deliver-acceptance-criteria/references/TEMPLATE.md) | 议程、主席准备、纪要、跨会综合分开；标准可表述为具体场景 | 区分私有 brief 与共享 blackboard；Criterion 关联输入、证据与结论 | 模板内容是提案，不授予角色权限，也不自动成为事实 |
| [MetaGPT environment](../../reference/MetaGPT/metagpt/environment/base_env.py)、[role](../../reference/MetaGPT/metagpt/roles/role.py) | 角色观察消息并按动作推进；`publish_message` 会按地址检查投递 | 借鉴专业角色和结构化产物接续 | 不能把当前 MetaGPT 简化为“所有消息必定广播给所有人”；其环境消息机制也不等于 Kiana 的授权交接 |
| [ChatDev DAG executor](../../reference/ChatDev/workflow/executor/dag_executor.py)、[图配置](../../reference/ChatDev/entity/configs/graph.py) | 拓扑分层、层内并行、节点触发判断 | WorkGraph 显式依赖；就绪计算、执行调度、人工节点分离 | 图节点存在不证明跨重启恢复或副作用幂等 |
| [Agency Swarm communication flows](../../reference/agency-swarm/docs/core-framework/agencies/communication-flows.mdx) | Handoff 与 orchestrator-worker 的控制返回语义不同；边显式定义 | `Handoff` 转责任，`DelegationPacket` 委派工作，分别建模 | 其 Handoff 可带全历史；Kiana 只传冻结输入和授权上下文 |
| [AutoGen Magentic-One orchestrator](../../reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/teams/_group_chat/_magentic_one/_magentic_one_orchestrator.py)、[发言示例](../../reference/autogen/python/samples/core_distributed-group-chat/_agents.py) | progress ledger、max_stalls/max_turns、定向 RequestToSpeak | Symposium 有轮次、时间、消息和费用上限；监控 stalled 后升级或重新规划 | 模型判断“完成”只作为建议；实际 Gate 读取证据 |
| [CrewAI Flow](../../reference/crewAI/lib/crewai/src/crewai/flow/flow.py)、[FlowPersistence](../../reference/crewAI/lib/crewai/src/crewai/flow/persistence/base.py) | Flow 与角色执行分层；状态持久化和 pending feedback 是独立接口 | HumanTask 保存目标版本与续接信息；业务流程等待可重建 | base adapter 的 pending feedback 默认实现并不保存完整等待上下文，不能仅凭接口声称恢复完成 |
| [Archon-Knowledge DAG schemas](../../reference/Archon-Knowledge/packages/workflows/src/schemas/dag-node.ts)、[executor](../../reference/Archon-Knowledge/packages/workflows/src/dag-executor.ts) | fresh/shared/resume 上下文、显式 approval decisions、某些节点拒绝可能遗弃子运行的 retry | 版本化 ProcessTemplate；Retry、Resume、Rework 三者不同；等待节点独立保存 | 不移植其 SDK 执行路径，Kiana 继续通过自己的 DaemonHost |
| [beads Claimer](../../reference/beads/issueops/claimer.go)、[ready](../../reference/beads/cmd/bd/ready.go) | 原子 compare-and-set claim；同一认领者重试可无写入返回；认领失败返回冲突 | PacketClaim、attempt、lease epoch 与 server-owned assignee；就绪查询不取得执行权 | Claimer 注释明确 Actor 为调用方声明，不能借此证明认证；不确定提交后应先查账 |
| [Task Master dependencies](../../reference/claude-task-master/scripts/modules/dependency-manager.js) | 增加依赖时检查成环，依赖 ID 规范化 | 批准 Plan 时验证整个 DAG，输出稳定缺失/环路径 | 单个 helper 对缺失节点的处理不代替 Kiana 的严格引用完整性 |
| [Gas Town convoy](../../reference/gastown/docs/concepts/convoy.md)、[scheduler](../../reference/gastown/docs/design/scheduler.md) | 持久批次与短期工人分离；调度元数据独立于工作描述；容量控制 | 业务 Packet 不随 Cell 消亡；调度使用 DispatchIntent、QueueEntry 和状态投影 | “所有 issue 关闭”不等于 Acceptance、Delivery 或 Outcome |
| [Ruflo swarm 模块说明](../../reference/ruflo/v3/@claude-flow/swarm/README.md) | 文档声明领域路由、拓扑、协调与监督职责 | 参考角色路由和监督职责切分，明确 root/parent/merge owner | 此处为模块文档阅读；没有验证其性能、共识或大规模自治声明，不因此引入新运行时 |
| [OpenSpec schema](../../reference/OpenSpec/schemas/spec-driven/schema.yaml)、[artifact graph](../../reference/OpenSpec/src/core/artifact-graph/graph.ts) | proposal/spec/design/tasks 的产物依赖显式配置 | Charter/Plan/Packet 的输入输出合同和缺件诊断；变更产出新 baseline | 文件存在、勾选 task 都不能代替经授权的业务事件 |
| [Temporal SDK README](../../reference/temporal-sdk-python/README.md)、[replayer](../../reference/temporal-sdk-python/temporalio/worker/_replayer.py) | 确定性 workflow 与 I/O activity 分离；等待 signal；历史重放与可控时钟测试 | 纯 process reducer + ControlPlane 发出指令；事件重放与恢复执行分开 | 不引入 Temporal 服务为前提，也不声称任意外部副作用 exactly-once |
| [LangGraph checkpoint](../../reference/langgraph/libs/checkpoint/README.md) | thread/checkpoint 身份、superstep pending writes，恢复时保留已完成节点 | ProjectProcess 保留节点完成事实；只恢复未决动作；成功 sibling 不重跑 | checkpoint 不是 Artifact 实物确认；节点恢复不等于外部调用不重复 |

其余项目用于范围筛选：运行时类约束执行接口；记忆类约束来源与检索；工作台类启发查看任务/实例/变更的方式；技能与方法论类只提供角色工艺。逐项目定位见 §5，未深读源码的目录不标成深度实现审计。

## 4. 目录外补充项目

| 官方来源 | 与本次设计相关的事实 | 采用决策 |
|---|---|---|
| [Paperclip 项目说明](https://github.com/paperclipai/paperclip)、[任务工作流](https://github.com/paperclipai/paperclip/blob/master/docs/guides/agent-developer/task-workflow.md)、[issues service](https://raw.githubusercontent.com/paperclipai/paperclip/master/server/src/services/issues.ts) | 组织、目标、任务与 agent run 分离；任务有单认领与执行锁；源码 checkout 检查 agent 可分配性、阻塞依赖和当前锁，使用条件更新 | 借鉴目标 ancestry、任务认领与 attention 看板；权限、EventLog 与 Harness 保持 Kiana 自己的权威。读取的是在线浮动 master，不作为可复现 release 测试 |
| [OpenProject 工作包关系](https://www.openproject.org/docs/user-guide/work-packages/work-package-relations-hierarchies/)、[角色工作流](https://www.openproject.org/docs/system-admin-guide/manage-work-packages/work-package-workflows/) | 父子层级与依赖关系分开；按 work package type、role 配置状态转移矩阵 | 组织层级、任务分解、阻塞依赖采用不同边类型；业务命令用角色×对象×状态矩阵 |
| [Plane Project API](https://developers.plane.so/api-reference/project/overview) | Workspace 内 Project 包含 work items、cycles、modules；成员和展示配置单列 | Project 是业务交付单位；排期视图和代码模块视图不改变工单的身份与权限 |
| [ERPNext Workflows](https://docs.frappe.io/erpnext/workflows)、[Task](https://docs.frappe.io/erpnext/tasks) | 审批流程包含状态、转移、角色和条件；取消必须有显式转移；任务具有依赖与模板 | 所有非终态都审查暂停/拒绝/取消/返工的出口；模板只是版本化过程配置 |
| [Restate Workflows](https://docs.restate.dev/tour/workflows)、[External Events](https://docs.restate.dev/develop/go/external-events) | workflow 可保存可查询状态，通过 durable promise 等待外部响应并在重启后继续等待 | HumanTask 用持久 wait key 关联具体目标与版本；回复是信号，恢复前仍校验当前权限与状态 |

这些是机制参考，不是“选择某个框架替代 Kiana”。本次推荐的 Rust 模块、状态细节、审批语义和步骤顺序，是结合 Kiana 源码后的设计结论。

## 5. 全目录清单

以下是初筛记录。HEAD 是本地目录快照，不声称为上游最新；`无独立 checkout` 的目录不得用父仓库 HEAD 冒充。分类描述来自 README 和目录索引，不含运行验证。

| 目录/入口 | 本地 HEAD | 类别 | 本次定位 | 阅读层级 |
|---|---|---|---|---|
| [12-factor-agents](../../reference/12-factor-agents/README.md) | `d20c728368bf` | 业务过程/任务 | 小任务嵌入确定性系统 | 机制核对（见 §3） |
| [Archon](../../reference/Archon/README.md) | `55ef3bc7bff2` | 上下文/知识 | Schr0d；本地依赖和冲击半径工具 | README/目录初筛 |
| [Archon-Knowledge](../../reference/Archon-Knowledge/README.md) | `fa2740050f18` | 编排/工作流 | coleam00；DAG、角色上下文、人工等待 | 机制核对（见 §3） |
| [ChatDev](../../reference/ChatDev/README.md) | `4fb2db0ea903` | 编排/工作流 | 2.0 图节点与分层执行，不沿用 1.0 假设 | 机制核对（见 §3） |
| [ECC](../../reference/ECC/README.md) | `5064474d4d76` | 技能/工程方法 | 工程 agent/skill/hook 配置包；不作权限源 | README/目录初筛 |
| [GitNexus](../../reference/GitNexus/README.md) | `b1d87c1f33d7` | 上下文/知识 | 代码知识图谱与依赖查询 | README/目录初筛 |
| [MemPalace](../../reference/MemPalace/README.md) | `000524b111e7` | 上下文/知识 | 本地分区记忆；非业务状态机 | README/目录初筛 |
| [MetaGPT](../../reference/MetaGPT/README.md) | `11cdf466d042` | 编排/工作流 | 角色动作与按地址投递的环境消息 | 机制核对（见 §3） |
| [OpenHands](../../reference/OpenHands/README.md) | `f7fb0c4b21f5` | 执行运行时 | 当前 README 定位 Agent Canvas 与多后端控制台 | README/目录初筛 |
| [OpenSpec](../../reference/OpenSpec/README.md) | `e062b9572be9` | 业务过程/任务 | 显式产物依赖与变更规范 | 机制核对（见 §3） |
| [Roo-Code](../../reference/Roo-Code/README.md) | `b867ec914575` | 执行运行时 | 与 roo-code 同一 HEAD；模式/任务界面参考 | README/目录初筛 |
| [a2a](../../reference/a2a/README.md) | `98853be376c8` | 协议/验证 | Agent 互操作协议；组织授权仍需另建 | README/目录初筛 |
| [adk-python](../../reference/adk-python/README.md) | `b0180620f4c2` | 编排/工作流 | Agent/工作流 SDK 的接口分层 | README/目录初筛 |
| [agency-swarm](../../reference/agency-swarm/README.md) | `5cd5a0d9c4ad` | 编排/工作流 | 定向委派与 Handoff 返回语义 | 机制核对（见 §3） |
| [agent-framework](../../reference/agent-framework/README.md) | `aea4dc221e97` | 编排/工作流 | Agent 与多步骤 workflow 框架 | README/目录初筛 |
| [agno](../../reference/agno/README.md) | `f974c175c6f5` | 编排/工作流 | Agent/运行服务/管理界面分层 | README/目录初筛 |
| [ai-coding-guide](../../reference/ai-coding-guide/README.md) | `d187dbdb83fa` | 技能/工程方法 | 中文使用教程；不作为核心实现依据 | README/目录初筛 |
| [aider](../../reference/aider/README.md) | `5dc9490bb35f` | 执行运行时 | 代码上下文与编辑执行参考 | README/目录初筛 |
| [architect-loop](../../reference/architect-loop/README.md) | `164d32c36eeb` | 业务过程/任务 | fresh worker、冻结检查、独立收束评审 | 机制核对（见 §3） |
| [autogen](../../reference/autogen/README.md) | `027ecf0a379b` | 编排/工作流 | 进度 ledger、发言调度、stall/turn 上限 | 机制核对（见 §3） |
| [awesome-agent-skills](../../reference/awesome-agent-skills/README.md) | `8873794bcb26` | 技能/工程方法 | 技能目录；发现用，不作实现证明 | README/目录初筛 |
| [beads](../../reference/beads/README.md) | `c0d8da42de5f` | 业务过程/任务 | 任务图、原子认领与冲突返回 | 机制核对（见 §3） |
| [claude-code-main (2)](<../../reference/claude-code-main (2)/claude-code-main/README.md>) | `无独立 checkout` | 历史/重建材料 | 嵌套官方分发/插件资料，只做公开行为定位 | 目录/公开说明 |
| [claude-code-rev-main](../../reference/claude-code-rev-main/README.md) | `无独立 checkout` | 历史/重建材料 | 重建材料，无独立 Git 快照；不移植源码 | 目录/公开说明 |
| [claude-code-rust](../../reference/claude-code-rust/README.md) | `4b87a363fd20` | 历史/重建材料 | 含重建/转换相关说明；不作组织设计代码来源 | 目录/公开说明 |
| [claude-mem-candidate](../../reference/claude-mem-candidate/README.md) | `1f1c13c981a7` | 上下文/知识 | 与 claude-memory 同一 HEAD；会话记忆 | README/目录初筛 |
| [claude-memory](../../reference/claude-memory/README.md) | `1f1c13c981a7` | 上下文/知识 | 会话抽取、索引、召回 | README/目录初筛 |
| [claude-task-master](../../reference/claude-task-master/README.md) | `c0c98d367c55` | 业务过程/任务 | 显式依赖与成环检查 | 机制核对（见 §3） |
| [cline](../../reference/cline/README.md) | `fc28a5fe3331` | 执行运行时 | CLI/IDE 与并行工作界面 | README/目录初筛 |
| [codex](../../reference/codex/README.md) | `d6489472f3c1` | 执行运行时 | 受控 Agent 运行与协议参考；非公司业务域 | README/目录初筛 |
| [container-use](../../reference/container-use/README.md) | `2e43e625e952` | 工作区/操作面 | 隔离执行环境与并行工作区 | README/目录初筛 |
| [continue](../../reference/continue/README.md) | `5522c6f44ca0` | 执行运行时 | 上下文/跨入口工具；按本地快照定位 | README/目录初筛 |
| [crewAI](../../reference/crewAI/README.md) | `34199c21b724` | 编排/工作流 | Flow/角色执行分层与持久等待接口 | 机制核对（见 §3） |
| [crush](../../reference/crush/README.md) | `563d658bccb5` | 执行运行时 | 终端运行/session 界面 | README/目录初筛 |
| [deepseek-harness](../../reference/deepseek-harness/README.md) | `c389f96bf3a9` | 执行运行时 | 单 Agent 工具循环与插件接口 | README/目录初筛 |
| [emdash](../../reference/emdash/README.md) | `c811c072b342` | 工作区/操作面 | 多个运行/工作区的桌面操作面 | README/目录初筛 |
| [everything-claude-code](../../reference/everything-claude-code/README.md) | `432485ba6b92` | 技能/工程方法 | 角色/技能/hook 工艺包；不同于 ECC 快照 | README/目录初筛 |
| [gastown](../../reference/gastown/README.md) | `649b832b7672` | 业务过程/任务 | 持久工作批次、临时工人与容量调度 | 机制核对（见 §3） |
| [get-shit-done](../../reference/get-shit-done/README.md) | `bdcaab2c752d` | 业务过程/任务 | README 迁移通知；当前入口转 gsd-core | README/目录初筛 |
| [goose](../../reference/goose/README.md) | `5e90925962f0` | 执行运行时 | Agent 运行与跨入口工作流接口 | README/目录初筛 |
| [gpt-pilot](../../reference/gpt-pilot/README.md) | `9b763fdaf002` | 执行运行时 | 开发过程 Agent；本次只读筛选，未运行 | README/目录初筛 |
| [graphify](../../reference/graphify/README.md) | `67f99bd0059d` | 上下文/知识 | 仓库图与项目知识查询 | README/目录初筛 |
| [graphiti](../../reference/graphiti/README.md) | `b943c9e8486c` | 上下文/知识 | 带时间关系的知识图谱方向 | README/目录初筛 |
| [grok-build](../../reference/grok-build/README.md) | `72a61251fcff` | 执行运行时 | 终端 coding runtime；不是组织模型 | README/目录初筛 |
| [gsd-core](../../reference/gsd-core/README.md) | `c6df4e1e463c` | 业务过程/任务 | 阶段、任务计划、执行/验收工艺 | README/目录初筛 |
| [gstack](../../reference/gstack/README.md) | `0530392821c2` | 技能/工程方法 | 专业角色工作方法与质量检查 | README/目录初筛 |
| [herdr](../../reference/herdr/README.md) | `9e01168b140c` | 工作区/操作面 | 运行会话托管与操作面 | README/目录初筛 |
| [langchain](../../reference/langchain/README.md) | `e670c7a03ba3` | 编排/工作流 | 通用 Agent/模型组件，避免重复运行时 | README/目录初筛 |
| [langgraph](../../reference/langgraph/README.md) | `81bf17b23123` | 编排/工作流 | checkpoint 身份与 pending writes | 机制核对（见 §3） |
| [letta](../../reference/letta/README.md) | `4511fa0bc91f` | 上下文/知识 | 迁移页；同 letta-oss，源码入口指向 letta-code | README/目录初筛 |
| [letta-code](../../reference/letta-code/README.md) | `6bc41be9f4a9` | 上下文/知识 | 有状态 Agent、记忆与身份接口 | README/目录初筛 |
| [letta-oss](../../reference/letta-oss/README.md) | `4511fa0bc91f` | 上下文/知识 | 同 letta HEAD；不视为第二份后端源码 | README/目录初筛 |
| [llama-index](../../reference/llama-index/README.md) | `d2ac544a27c7` | 上下文/知识 | RAG/索引与来源组织 | README/目录初筛 |
| [mcp-servers](../../reference/mcp-servers/README.md) | `d73f99efbfd4` | 协议/验证 | 参考工具服务；不是组织命令授权源 | README/目录初筛 |
| [mem0](../../reference/mem0/README.md) | `dae67f74f5cc` | 上下文/知识 | 记忆抽取与存取；不采信 README 性能为 Kiana 证据 | README/目录初筛 |
| [memorix](../../reference/memorix/README.md) | `3a5a3c700e4d` | 上下文/知识 | 跨会话/跨工具项目记忆 | README/目录初筛 |
| [mini-swe-agent](../../reference/mini-swe-agent/README.md) | `04d809ceab9d` | 执行运行时 | 小型执行循环与评测工艺 | README/目录初筛 |
| [openai-agents-python](../../reference/openai-agents-python/README.md) | `f355af660416` | 编排/工作流 | handoff、guardrail、session SDK 参考 | README/目录初筛 |
| [opencode](../../reference/opencode/README.md) | `d6855b6b47a8` | 执行运行时 | 运行/session/界面接口 | README/目录初筛 |
| [orca](../../reference/orca/README.md) | `12f53da542d0` | 工作区/操作面 | 多 Agent 工作区与比较视图 | README/目录初筛 |
| [pi](../../reference/pi/README.md) | `96617628e852` | 执行运行时 | Agent runtime 与 coding session 分层 | README/目录初筛 |
| [planning-with-files](../../reference/planning-with-files/README.md) | `0d21b6c4aa5f` | 业务过程/任务 | 计划/发现/进度的持久文件分工 | README/目录初筛 |
| [pm-skills](../../reference/pm-skills/README.md) | `a5115727700b` | 业务过程/任务 | 会议工件合同与可验证验收场景 | 机制核对（见 §3） |
| [promptfoo-full](../../reference/promptfoo-full) | `无独立 checkout` | 协议/验证 | 空内容/无有效 checkout；没有可审源码 | 目录/公开说明 |
| [pydantic-ai](../../reference/pydantic-ai/README.md) | `62f1e8302a35` | 编排/工作流 | 类型化 Agent/graph 及接口边界 | README/目录初筛 |
| [roo-code](../../reference/roo-code/README.md) | `b867ec914575` | 执行运行时 | 与 Roo-Code 同一 HEAD；重复证据去重 | README/目录初筛 |
| [ruflo](../../reference/ruflo/README.md) | `a295c6870315` | 编排/工作流 | 领域路由/监督模块文档；未验证性能/共识声明 | 机制核对（见 §3） |
| [skills](../../reference/skills/README.md) | `3cca18b368ae` | 技能/工程方法 | 可组合工程工艺，按岗位按需使用 | README/目录初筛 |
| [spec-kit](../../reference/spec-kit/README.md) | `4a7341a93d94` | 业务过程/任务 | 需求/计划/任务的模板和工作流 | README/目录初筛 |
| [strix](../../reference/strix/README.md) | `52b19233477a` | 协议/验证 | 独立安全验证方向；未运行扫描 | README/目录初筛 |
| [superpowers](../../reference/superpowers/README.md) | `b36e0829c6d0` | 技能/工程方法 | 设计、任务与代码审查工艺 | README/目录初筛 |
| [temporal-sdk-python](../../reference/temporal-sdk-python/README.md) | `22a9e41fd857` | 编排/工作流 | 确定性 workflow、signal、activity 与 replay | 机制核对（见 §3） |

## 6. Kiana 源码取证范围

基线 HEAD 为 `db77c2485bcafecbb1da17ec57ee509ad2ee32b4`，相关源码存在未提交 WIP。重点核对：`kiana-domain/src/{company,roles,work_packets,symposiums,states}.rs`、`kiana-core/src/{company,collaboration,cell_registry,artifacts}.rs`、`kiana-workflow/src/lib.rs`、`docs/company-command-api.md` 和 `CURRENT_STATUS.md`。

本次初读 CompanyCommand 为 40 个 variant；追加前复核为 46 个，增加了 Handoff ACK、PacketReview、预算配置和 claim/renew/reclaim。DepartmentSpec 有五部门目录、RoleSpec 有六个角色目录；WorkPacket 的通用校验仍限制为 Builder；CompanyState 的 Run 表仍以 packet ID 为键；RequestAcceptance 仍收集全项目标准并要求全项目 packet 完成；Milestone 的启动又可以依赖其他 Milestone 的 Accepted/Closed。最后两项组合会形成分阶段交付的等待环，需通过 roadmap 的 CO-27 复现和修复。

复核还发现 [workflow durable planner](../../kiana-workflow/src/durable.rs) 与 [core automation](../../kiana-core/src/automation.rs) 已出现在 WIP：纯 plan_command 返回 effect，core 先登记再通过 Company StartRun 或现有 authorize_and_execute 执行；具有人工等待、信号、触发和显式推进命令。配套 [scheduling-command-api.md](../scheduling-command-api.md) 仅声明编译，明确未运行测试。本次将这些列为待复用、待行为验证的实现，CO-18–CO-23 不另起调度/流程运行时。

以上是源码阅读结论，不是测试失败回执，也不改变任何 `feature_status` 或 `proof_level`。当前源码继续变化，执行步骤开始时须重新核对；新功能是否通过，仍由绑定快照的实际测试和状态账本回答。

### 读取基线的相关文件摘要

采集时间：`2026-09-12T08:39:54.951413+00:00`。下表固定本次开始时的相关 WIP 内容；不是完整仓库归档，也不替代 CO-01 的最新核验。SHA-256 完整值如下。

| 文件 | SHA-256 |
|---|---|
| `docs/roadmap.md` | `ab564fd2e416fc8f9d352a23f001fca1cd8e4cc76d220aeb779af8683987fedb` |
| `docs/module-map.md` | `2799a66be6ff6cf444dd8deb45fe039b5a3b0ba31565ad37d5e6d8e166985c8b` |
| `CURRENT_STATUS.md` | `e8e38a06319851500945d4f838cc164a29c5b0266cb0fcad0940ff1042d94df5` |
| `COMPANY.md` | `2864623ef82aa8d85f1e151bda7f0f93add6d42e603fc8e924843eab16a8886d` |
| `kiana-domain/src/company.rs` | `a843baa4984361337489d3391fa31a168bf8786f6f921d22af39f86a0c11661e` |
| `kiana-core/src/company.rs` | `0a1f37be7a41341c50e871d7dbc09afb62ad377cf5f0a138db5c08cb798a51ef` |
| `kiana-domain/src/roles.rs` | `a2beb2104d751ee6c9e6b9ef510f85e65ec3da727973e110f44209b4c402a5c6` |
| `kiana-domain/src/lib.rs` | `250d18c84c95e6477f260a5c983481f7c29370bc86b2a5afc862c3693b9524d0` |
| `kiana-core/src/collaboration.rs` | `574e46803fc057b7c17f020a4409cdc6353e795fe083e1f23ed3b44c7f55e032` |
| `kiana-core/src/cell_registry.rs` | `89043a871605bca43acd4201a359580d958e7ff3418d7ff7d30a802f0f973afe` |
| `kiana-workflow/src/lib.rs` | `b1613cfeb56413ac46519e56d0960cb15677f5a92a24323c9cdc69bf88d57b31` |
| `docs/company-command-api.md` | `404f85e30e750820321853514722743c7748873daae5799e1249b3d3a07d2816` |

### 追加前共享 WIP 复核

采集时间：`2026-09-12T09:19:29.576830+00:00`。本轮研究期间源码与 roadmap 持续变化，初读快照不能覆盖新增代码。上文当前观察已按下列版本复核；该表记录源码身份，不是编译或测试回执。roadmap 摘要是保留另一专项追加后的原文件，尚未包含本次 CO 追加。

| 文件 | SHA-256 |
|---|---|
| `docs/roadmap.md` | `d62f532b63c64f250224265c2ce8a9348735868c443c83bf99a51050afd3eefb` |
| `kiana-domain/src/company.rs` | `e744a374ed9216bf1f071636e26ba818b0726496226bdae648217d3a0f2d1ce8` |
| `kiana-core/src/company.rs` | `4de7860c84929d8d10caf59cb4a28f7a0b24c102d4bf1c1293b2d0995a3b8e60` |
| `kiana-domain/src/work_packets.rs` | `2cc8b27f3710512b50bef970715dade5d76f27d010c77e14e3c22823d2b8b1e6` |
| `kiana-domain/src/packet_graph.rs` | `8f56ce5fb8ba80f50aabff3e6fb6221718289437b1a511123821c0d98509c8a0` |
| `kiana-domain/src/handoff.rs` | `0bfaa1fecb222641a495947ca0c495c4e85bfc326ca162e3e10a3d90496670ef` |
| `kiana-core/src/cell_registry.rs` | `aa3a22bf6d87e568bcfb4438ed6c424516fd0a7419d3131d3f1932a191911351` |
| `kiana-workflow/src/lib.rs` | `c7467406e66da57476a73e3dc9a5eb9f808685098336113f4439fd49b495ca06` |
| `kiana-workflow/src/durable.rs` | `11691944613762c8a782f37b5ebce2ba9ce24d92a1a189f475f3acb776b7ac9c` |
| `kiana-core/src/automation.rs` | `f3106359dabe17479c35fc60186be6b30d69ee47f67c3b7d436b424da4c8961d` |
| `kiana-core/src/swarm.rs` | `a5a765f28b3757a22659e6e05ab5ba9b63d54fe091a28ef0e4123cb13329d98d` |
| `docs/scheduling-command-api.md` | `15c1a36124b99df8af98ddd9b9910d7a58c4f5ffad155a5f374cbaa9888b9479` |

## 7. 本次文档校验回执

本次只写入组织业务设计、本文和 roadmap 末尾的 CO 追加区。追加前 roadmap 已含 ControlPlane、Harness、Provider 专项；这些既有内容逐字节保留。

```text
source_snapshot: db77c2485bcafecbb1da17ec57ee509ad2ee32b4 + WIP；相关文件 hash 见 §6
worktree_status: 本专项新增两份文档，追加 docs/roadmap.md；未改动产品源码
command_argv: python3 /tmp/kiana-companyos-research-20260912/check_docs.py --appended
              git diff --check -- docs/roadmap.md
                docs/company-os-organization-business-design.md
                docs/reference-agent-audit/company-os-organization-business-survey-2026-09-12.md
cwd / environment: 仓库根目录；Linux / bash / Python 3
fixture / cassette: 无产品 fixture；检查本次 Markdown 与追加前保存的 roadmap 字节
exit_code: 上述两项均为 0
status change: 无；48 个 CO 步骤全部保持待实施/待核验
proof-level change: 无
limitations: 仅文档结构、依赖、引用和保留性检查；无 Rust 测试、live provider 或 CI 回执
reviewer: Codex 自检；未独立评审
```

检查结果：48 张步骤卡，145 条直接依赖边，依赖均指向先前步骤且无环；127 个本地文件/锚点引用有效；参考目录表 72 行。未来测试名、接口和代码落点均按目标描述，不计为已有实现。

追加前保留的 roadmap 前缀：268642 bytes，SHA-256 `d62f532b63c64f250224265c2ce8a9348735868c443c83bf99a51050afd3eefb`。本次 CO 内容位于第 17–19 节，从第 2723 行追加分隔符起；原 P/CP/Harness/Provider 内容及完成状态未被本专项覆盖。
