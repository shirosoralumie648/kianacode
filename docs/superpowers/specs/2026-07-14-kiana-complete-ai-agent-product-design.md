# Kiana 完整 AI Agent 产品设计

**日期：** 2026-07-14
**状态：** 设计已批准，待书面复核
**适用范围：** Kiana 统一 AI Agent、三个能力包、全部产品入口、官方云与企业自托管
**项目定义：** `.planning/PROJECT.md`

## 1. 设计目标

Kiana 要成为一个功能完整、可公开发行、可商业交付的 AI Agent。它首先以 Claude Code 的公开功能覆盖作为 Coding 基线，同时以更可靠的长任务执行、证据验证和故障恢复形成差异化；在同一个核心上并行交付 Academic Research 和 Daily Work，并将 Claude Desktop 的桌面工作区与连接器体验纳入产品外壳。

最终 `1.0` 不是单一终端工具，而是由以下交付物共同组成：

- 开源、本地优先的个人 AI Agent；
- Coding、Academic Research、Daily Work 三个正式能力包；
- CLI/TUI、Headless SDK/RPC/MCP、IDE、Desktop、Web/App Server；
- 可选的官方云同步、远程执行和团队空间；
- 企业自托管、SSO/RBAC、集中策略、审计和运维能力。

项目允许持续发布 Alpha/Beta，但只有上述范围全部通过共同发布门禁后才能标记为 `1.0`、`完整` 或 `生产就绪`。

## 2. 已批准的产品决策

| 主题 | 决策 |
|------|------|
| 借鉴方式 | 公开行为层独立实现；许可证允许且架构适配的开源代码尽量合法复用 |
| 首要基线 | Claude Code 公开功能覆盖，不要求命令、配置或 UI 直接兼容 |
| 差异化 | 更可靠地完成长任务：多 Agent、证据链、验证、恢复和外部回执 |
| 首批用户 | 高级个人开发者优先，同时按公开发行标准交付 |
| 架构 | 稳定 Kiana Core + Coding/Research/Daily 能力包 + 多端产品外壳 |
| 模型 | 首版即支持 Anthropic、OpenAI、Gemini、OpenRouter、OpenAI-compatible 和本地模型 |
| 平台 | Linux/macOS 原生；Windows 首版通过 WSL |
| 产品入口 | CLI/TUI、Headless、IDE、Desktop、Web/App Server 全部进入完整版本 |
| 数据 | Local-first、无需账户；云同步与远程执行显式启用并加密 |
| 自主权 | 首次启动选择安全、平衡、自治档位，并允许按项目覆盖 |
| 三条产品线 | 并行开发，Coding 投入最高，三者共同达到正式质量后发布 1.0 |
| 商业模式 | Open Core；个人核心开源，官方云和企业自托管商业化 |
| Reference | 38 个目录逐项进入 `Adopt / Adapt / Reject` 能力矩阵 |
| 中间发布 | 持续 Alpha/Beta，1.0 门禁不降级 |
| 完成标准 | 公开发行质量是最高硬门禁，功能存在本身不代表完成 |

## 3. 总体架构

```text
CLI / TUI / SDK / RPC / MCP / IDE / Desktop / Web / App Server
                              |
                Typed Commands and Runtime Events
                              |
        +------------------- Kiana Core -------------------+
        | runtime | session | provider | tools | policy    |
        | memory  | context | workflow | evidence | recovery|
        +--------------------------------------------------+
              |                |                 |
         Coding Pack      Research Pack      Daily Pack
              |                |                 |
        Local Workspace / Optional Cloud / Enterprise Control Plane
```

### 3.1 Kiana Core

Kiana Core 是唯一的 Agent 运行时事实来源，负责：

- turn、stream、tool call、permission request 和 result 的 typed events；
- 可创建、恢复、分叉、导入和导出的 session tree；
- Provider、模型、认证和能力协商；
- Tool、MCP workbench、skill、plugin、hook 和 connector 生命周期；
- ProjectTrust、权限档位、沙箱、网络和外部副作用策略；
- context、repo map、memory、artifact 和 provenance；
- workflow DAG、WorkPacket、evidence、verification、review 和 recovery。

产品入口只负责输入、呈现和入口特有协议。任何入口都不得自行复制模型循环、工具执行、权限判断、会话存储或任务状态机。

### 3.2 能力包

能力包通过稳定的 domain pack contract 注册领域工具、工作流模板、对象 schema、评测集和呈现扩展。能力包可以依赖 Kiana Core，但不能分叉核心状态或发明不兼容的事件协议。

### 3.3 服务与部署

- **Local Personal**：无需账户即可完整运行个人核心和三个能力包。
- **Official Cloud**：提供显式启用的加密同步、远程 worker、团队空间、订阅和计费。
- **Enterprise Self-hosted**：提供 SSO/RBAC、集中策略、审计、数据保留、离线部署和运维工具。

云端与企业端必须使用和本地模式相同的 task、event、policy 和 evidence contracts，不能形成另一套产品语义。

## 4. 统一执行模型

```text
User Intent
    -> Intent Router
    -> Workflow Planner
    -> Context Builder
    -> Agent Runtime <-> Tool/Workbench <-> Policy Gate
    -> Evidence Ledger
    -> Verifier
    -> Complete / Rework / Blocked / Awaiting Approval / Result Unknown
```

### 4.1 请求路由

Intent Router 根据任务目标、输入材料、目标应用和明确命令选择 Coding、Research、Daily 或组合工作流。路由结果包含选择理由、所需权限、预计副作用和验收方式，并允许用户覆盖。

简单、低风险、可立即验证的请求可以直接执行。跨多个步骤、需要外部副作用、需要多 Agent 或可能跨会话的请求必须转为持久化 `WorkflowRun`。

### 4.2 计划与上下文

Workflow Planner 输出带依赖关系的任务图、资源预算、风险等级和可观察验收条件。Context Builder 只装载任务需要的代码、文档、记忆、连接器数据和来源；每个上下文项保留来源、时间、权限和内容哈希。

### 4.3 Agent 与工具循环

Agent Runtime 将不同 Provider 的流式输出转换为统一 events。所有工具调用先经过 schema 校验、能力检查和 policy gate；执行结果包含结构化状态、输出、证据引用和可重试信息。

不同模型能力不一致时，Provider 层必须显式路由、降级或拒绝。禁止把不支持的 tool use、vision、structured output、long context 或 reasoning 能力伪装成支持。

### 4.4 Evidence 与 Verification

Evidence Ledger 记录文件变化、diff、命令、测试、构建、引用、实验、外部回执、审批和产物。Verifier 根据任务开始时定义的验收条件决定：

- `complete`：全部必需证据存在且验证通过；
- `rework`：结果可修正，生成新的受限任务；
- `blocked`：策略、依赖或完整性问题阻止继续；
- `awaiting_approval`：需要用户或组织批准；
- `result_unknown`：外部副作用可能发生但无法确认，禁止盲目重放。

模型的自然语言声明不能单独使任务进入 `complete`。

### 4.5 多 Agent

多 Agent 任务拆成 `WorkPacket`，每个 packet 明确目标、输入、允许文件、工具、预算、依赖、验收和返回 schema。并发写入使用路径锁或隔离工作区；集成 Agent 只能合并已验证结果，并必须再次运行整体门禁。

## 5. 能力包

### 5.1 Coding Pack

Coding Pack 以 Claude Code 的公开核心功能为覆盖基线，并吸收其他 coding references 的优势：

- 仓库探索、repo map、上下文预算与可编辑/只读文件集；
- 精确文件编辑、AI diff、undo、Git 安全和 worktree 工作流；
- Shell、沙箱、网络、浏览器、computer-use、notebook 和 LSP；
- lint、test、build、debug、review 和自动修复循环；
- session、后台任务、remote worker、subagent 和 team orchestration；
- MCP、skills、plugins、hooks、项目 agents/rules/prompts/checks；
- CLI/TUI、Headless、IDE、Desktop 和 Web 中一致的任务状态。

功能覆盖不要求复制 Claude 的专有实现、品牌或像素级界面。Kiana 可以提供自己的命令和配置，只要目标能力和真实用户旅程完整。

### 5.2 Academic Research Pack

Research Pack 覆盖：

- 文献检索、合法下载、去重、PDF/网页解析和引用管理；
- 笔记、证据图谱、问题定义、假设、研究计划和进展追踪；
- 代码、数据集、实验编排、统计分析、图表和消融；
- 论文结构、写作、claim-support 检查、复现包和投稿材料；
- EDA、硬件、机器人等领域扩展包及人工审批动作。

每个结论必须能追溯到文献、数据或实验产物。模型不得伪造 DOI、引用、实验结果、统计显著性或外部审稿状态。

### 5.3 Daily Work Pack

Daily Pack 覆盖：

- 文件、笔记、待办、日历、邮件和提醒；
- 文档、表格、演示、会议、消息和知识库；
- 浏览器和桌面应用控制、表单、下载上传和可恢复 RPA；
- 项目管理、长流程、多 Agent、审批、审计、报告和团队协作。

所有外部写入必须有可识别目标、预览或策略判断、执行回执和审计记录。付款、发布、发送、删除和权限变更使用更严格的硬审批。

## 6. 产品入口

| 入口 | 责任 | 禁止事项 |
|------|------|----------|
| CLI/TUI | 交互式 Agent、任务监控、权限和本地管理 | 不复制 runtime 状态 |
| Headless SDK/RPC/MCP | CI、自动化、第三方嵌入和结构化事件 | 不绕过 policy gate |
| IDE | 编辑器上下文、diff、diagnostics、review 和任务状态 | 不建立独立 session 格式 |
| Desktop | 对话、工作区、artifact、connector、computer-use 和可视化审批 | 不保存独立于 Core 的任务事实 |
| Web/App Server | 远程任务、团队空间、管理和运维 | 不把本地核心变成强制云依赖 |

用户可以在 CLI 创建任务、在 IDE 查看 diff、在 Desktop 批准外部动作、在 Web 观察远程 worker；所有入口看到的是同一个 workflow 和 event stream。

## 7. 数据、信任与安全

### 7.1 Local-first

本地个人模式无需账户。代码、会话、记忆、工作流和证据默认保存在设备；云同步、远程执行和遥测均为显式开启。用户可以查看、导出和删除同步数据。

### 7.2 凭据

API key、OAuth token、Cookie、SSH key 和企业凭据进入操作系统 Keychain、硬件安全模块或服务端 secret vault。凭据不写入项目目录，不进入普通日志，也不自动加入模型上下文。

### 7.3 ProjectTrust 与扩展

未知项目默认不加载项目内 skills、plugins、hooks、MCP 或自动化规则。建立信任时展示将启用的能力和风险；信任记录保存在项目外的用户存储中，并绑定规范化项目身份。

### 7.4 自主权档位

- **安全**：大部分副作用逐项确认；
- **平衡**：只读自动，可逆低风险写入按策略自动，高风险动作确认；
- **自治**：允许在受限目标、预算和作用域内持续执行。

三个档位都不能绕过删除、发布、付款、凭据、外部消息、组织策略和不可逆远程写入等硬边界。

### 7.5 云与企业

云端和企业端必须提供租户隔离、RBAC、审计日志、密钥轮换、数据保留、备份恢复和区域策略。企业自托管必须能在离线或受限网络中完成安装、升级、回滚和诊断。

### 7.6 供应链

发布物需要签名、校验和、SBOM、依赖和许可证扫描。插件、skills、MCP 和连接器需要来源、版本、完整性和权限声明；远程更新失败时必须回滚到已验证版本。

## 8. 故障与恢复

### 8.1 事实来源

长任务以追加式 EventLog 为事实来源，materialized state 是可重建投影。事件包含 schema version、workflow identity、因果顺序和完整性信息。

### 8.2 错误分类

| 类型 | 行为 |
|------|------|
| 可重试 | 按预算和退避策略重试，记录每次尝试 |
| 需要输入 | 暂停并请求用户补充，不丢失已完成工作 |
| 策略拒绝 | fail closed，返回结构化原因和可行替代路径 |
| 外部依赖失败 | 保留回执和状态，支持恢复或人工接管 |
| 完整性损坏 | 阻塞执行，从可信事件和备份恢复 |
| 结果未知 | 禁止自动重放外部副作用，先查询或人工核对 |

### 8.3 幂等与补偿

内部状态变更使用幂等键和原子提交。可补偿动作记录对应撤销方法；不可补偿动作在执行前必须获得更严格批准。恢复流程不能通过“重新执行整个 Agent 回复”来猜测状态。

## 9. Reference 治理

`reference/` 的目标不是逐仓库复制，而是保证每项能力都被审计。能力矩阵至少包含：

- reference repository、上游版本和具体机制；
- 许可证、来源可信度和可复用边界；
- `Adopt / Adapt / Reject` 与理由；
- Kiana owner crate、能力包和产品入口；
- command/tool/event/data/policy contracts；
- 实现状态、自动化测试、真实场景和发布证据；
- 已知风险、stub/mock 警告和上游差异。

只有以下情况可以 `Reject`：许可证不兼容、存在无法接受的安全或隐私风险、与产品核心边界冲突、已有更完整等价能力，或参考本身是无效/废弃/stub 实现。拒绝必须可审查，不能用“以后再说”代替理由。

Claude Code 和 Claude Desktop 作为公开行为参考，不视为可复制源码。对动态上游建立版本快照和差异追踪，避免“功能对齐”成为永远无法冻结的描述。

## 10. 测试与评测

### 10.1 测试层级

1. 单元测试和 schema/contract 测试；
2. Provider、tool、plugin、connector 和跨进程集成测试；
3. Coding、Research、Daily 的真实黄金任务集；
4. CLI、IDE、Desktop、Web、云和企业端到端旅程；
5. Linux、macOS、Windows/WSL 安装、升级、回滚和卸载矩阵；
6. 安全、隐私、许可证、供应链、性能和故障注入测试。

### 10.2 领域证据

- Coding：diff、测试、构建、静态检查、运行结果和 review；
- Research：来源、引用、数据、实验、统计和复现产物；
- Daily：目标状态、外部回执、审批和审计记录。

每个 requirement、reference capability 和发布声明必须能追踪到测试与证据。仅有代码路径、CLI help、页面截图或 mock response 不足以证明完成。

## 11. 发布与商业门禁

### 11.1 Alpha/Beta

Alpha/Beta 可以按垂直切片持续发布。每个版本必须明确支持范围、已知限制、迁移风险和数据兼容性；不能使用模糊的“基本完成”掩盖门禁缺口。

### 11.2 1.0

`1.0` 至少要求：

- Coding、Research、Daily 三个能力包均达到正式质量；
- 全部产品入口完成核心用户旅程并共享一致状态；
- 个人、云端、企业三种交付形态可安装、升级、回滚、恢复和运维；
- 多供应商能力协商、权限档位、本地数据、加密同步和 remote worker 行为通过测试；
- 没有未处置的 P0/P1 缺陷、已知数据损坏路径或可绕过安全策略；
- 所有高风险外部副作用有审批、回执、审计和恢复证明；
- 文档、迁移、管理、API、支持和漏洞响应流程齐全；
- 供应链、安全、隐私、许可证和平台发布矩阵全部通过。

### 11.3 商业产品

- 个人版提供本地完整核心和能力包，不以云账户为使用前提；
- 官方云提供同步、远程执行、团队空间、订阅和计费；
- 企业版提供自托管、SSO/RBAC、集中策略、审计、数据治理和支持服务。

商业层可以增加规模、协作和治理能力，但不能故意破坏个人本地核心的完整性来制造付费依赖。

## 12. 分阶段实施原则

实施必须遵循依赖顺序，而不是按界面可见度排序：

1. runtime、session、provider、tool、policy 和 typed events；
2. workflow、evidence、verification、recovery 和多 Agent 隔离；
3. Coding Pack 的 Claude Code 功能基线；
4. Research 与 Daily 的共用基础及领域工作流；
5. CLI/TUI、Headless 和 IDE；
6. Desktop、Web/App Server 和跨入口连续体验；
7. 官方云和企业控制面；
8. 全量 reference 差异收敛、故障注入和 1.0 发布证明。

三条能力线可以并行，但低层契约必须先冻结。任何并行实现如果需要复制 session、tool、policy 或 event model，应停止并回到核心契约设计。

## 13. 明确边界

- 不复制专有源码、品牌或受保护素材；
- 不把所有参考实现机械拼接成一个不可维护系统；
- 不允许 Research 伪造来源、数据或实验；
- 不允许 Daily 自动化在结果未知时盲目重放；
- 不允许任何自主权档位绕过硬策略；
- 不以强制云账户替代 Local-first；
- 不在首个完整版本承诺原生 Windows，Windows 通过 WSL 支持；
- 不用模块、命令、页面或测试数量替代真实完成证据。

## 14. 设计验收结论

本设计已经覆盖产品目标、架构、组件边界、数据流、多 Agent、能力包、产品入口、商业交付、安全、错误处理、恢复、Reference 治理、测试和发布门禁。各项决策与 `.planning/PROJECT.md` 一致，没有保留会改变总体路线的未决项。
