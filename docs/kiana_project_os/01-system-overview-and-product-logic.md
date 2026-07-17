# Volume 01: 系统总览与产品逻辑

## 1. 定位

Kiana 的目标不是成为一个更会聊天的 CLI，也不是把现有 coding agent 换皮。它要成为个人项目操作系统，把“项目为什么做、现在做到哪、下一步做什么、怎么验证完成、长期记忆如何继承”这些问题变成软件对象和运行协议。

Kiana 的核心闭环：

```text
Goal
  -> Capture
  -> Context
  -> Plan
  -> Work
  -> Verify
  -> Review
  -> Report
  -> Learn
  -> Next Goal / Next Task
```

这个闭环必须可恢复、可审计、可拆分、可并行、可长期演进。

## 2. 产品层级

Kiana 包含三个产品层级：

| 层级 | 目标用户 | 核心价值 | 不做什么 |
| --- | --- | --- | --- |
| Personal CLI | 单人开发者、研究者、硬件项目操作者 | 本地项目推进、记忆、验证、汇报 | 不依赖云端才能工作 |
| Enterprise Offline | 企业内网/离线团队 | 权限、审计、插件治理、离线交付 | 不默认上传代码 |
| Cloud Workspace | 远程项目空间和 worker pool | 并发执行、团队视图、浏览器控制台 | P0 不优先 |

三个层级共享：

- Runtime event model。
- WorkflowRun。
- Task Card。
- Evidence Ledger。
- PolicyDecision。
- ContextPack。
- VerificationPacket。

差异只在 UI、部署方式、worker 执行位置和权限策略默认值。

## 3. 用户模型

### 3.1 个人持续推进型用户

典型行为：

- 经常说“继续”。
- 同时维护多个项目。
- 需要保留长期目标和当前 blocker。
- 需要让 agent 不丢上下文。
- 需要中文汇报。

Kiana 必须提供：

- 项目级 memory。
- `.kiana/workflows/` 持久状态。
- `/project next`。
- `/report progress`。
- live evidence 检查。

### 3.2 工程交付型用户

典型行为：

- 希望从需求到实现再到验证有闭环。
- 关心测试、构建、发布、回滚。
- 不接受“看起来完成”。

Kiana 必须提供：

- verification gate。
- review gate。
- release proof。
- blocker report。
- check profile。

### 3.3 并行推进型用户

典型行为：

- 希望多个任务同时做。
- 担心并发冲突。
- 希望主 agent 管理集成。

Kiana 必须提供：

- WorkPacket。
- path lock。
- worker budget。
- integration gate。
- touch-set audit。

### 3.4 EDA/硬件项目用户

典型行为：

- 有原理图、BOM、Gerber、CPL、嘉立创打样约束。
- 希望 AI 做审查和计划，而不是盲目画板。
- 需要 bring-up 步骤和风险清单。

Kiana 必须提供：

- `/eda review`。
- BOM risk。
- DFM checklist。
- bring-up plan。
- hardware approval gate。

## 4. 产品模式

### 4.1 `/quick`

用途：

- 快速解释。
- 单命令。
- 单文件轻改。
- 低风险查询。

逻辑：

1. 不创建 WorkflowRun。
2. 可以创建轻量 evidence。
3. 如果发现跨文件修改或验证需求，升级到 `/task`。
4. 如果用户只问概念，保持 L0。

输出：

- 简短回答。
- 命令结果。
- 小 diff。
- 必要时升级说明。

### 4.2 `/task`

用途：

- 单个 bugfix。
- 单个 feature slice。
- 单个 refactor。
- 单个测试补齐。

逻辑：

1. 构建 task-level ContextPack。
2. 定义 scope。
3. 执行修改。
4. 运行验证命令。
5. 生成 ResultPacket 和 VerificationPacket。
6. 根据结果 Done/Blocked/Fix Loop。

### 4.3 `/project`

用途：

- 长期项目。
- 多阶段目标。
- 商业化推进。
- 论文/产品/硬件项目。

逻辑：

1. 创建 WorkflowRun。
2. Capture 目标和约束。
3. 建 WBS。
4. 生成 Kanban。
5. 选择 Ready task。
6. 每次继续都从状态恢复。

### 4.4 `/swarm`

用途：

- 有多个互不冲突的 Ready task。
- 需要高并发推进。

逻辑：

1. 检查 task dependency。
2. 计算 allowed files。
3. 检查 path overlap。
4. 创建 WorkPacket。
5. 派发有限 worker。
6. 收集 ResultPacket。
7. 主 agent 集成。
8. 统一验证。

### 4.5 `/audit`

用途：

- 严格审计。
- 商用化缺口。
- 发布前检查。
- 安全与质量风险。

逻辑：

1. live scan。
2. 查 fake/stub/TODO/hardcoded。
3. 查测试/构建/release/proof。
4. 查 policy/plugin/MCP/hooks。
5. 输出 finding。
6. finding 必须有 evidence。

### 4.6 `/report`

用途：

- 中文汇报。
- 项目进展。
- 当前问题。
- 下一步路线。

逻辑：

1. 只从 evidence ledger、state、live scan、verified memory 取事实。
2. 区分完成、进行中、阻塞、未知。
3. 输出可直接口述的中文。
4. 不把计划当进展。

### 4.7 `/eda`

用途：

- 电路电子设计审查。
- EasyEDA/嘉立创项目推进。
- BOM/DFM/bring-up。

逻辑：

1. 复用 WorkflowRun。
2. 领域规则包替换软件规则包。
3. Evidence 类型扩展到硬件文件。
4. 高风险操作必须 approval。

## 5. 软件边界

Kiana Core 不应直接关心具体 UI。

Core 只输出：

- events。
- state。
- packets。
- reports。
- policy decisions。
- review findings。

CLI/TUI/Web 只是 projection。

## 6. 端到端闭环

### 6.1 从目标到任务

输入：

- 用户自然语言。
- PRD。
- issue。
- roadmap。
- 文档。

输出：

- Goal。
- Success Criteria。
- Constraints。
- NOT_BUILDING。
- WBS。
- Task Cards。

失败：

- 目标不清楚：Clarify Gate。
- 范围过大：Split Goal。
- 高风险：Approval Required。
- 不安全：Cancel/Block。

### 6.2 从任务到执行

输入：

- Task Card。
- ContextPack。
- WorkPacket。

输出：

- changed files。
- command results。
- errors。
- ResultPacket。

失败：

- scope violation。
- dirty state conflict。
- command failure。
- missing dependency。
- worker timeout。

### 6.3 从执行到完成

输入：

- ResultPacket。
- verification profile。
- acceptance criteria。

输出：

- VerificationPacket。
- ReviewPacket。
- consolidated review。
- delivery summary。

完成条件：

- verification pass。
- acceptance satisfied。
- no blocking findings。
- evidence written。

### 6.4 从完成到学习

输入：

- final packets。
- eventlog。
- review findings。

输出：

- learnings.md。
- memory proposal。
- stale memory notes。
- next task。

禁止：

- 自动把低置信结论写进长期 memory。
- 删除用户任务。
- 把失败经验伪装成规则。

## 7. 关键产品判断

### 7.1 为什么不是普通 coding CLI

普通 coding CLI 关注：

- 当前 prompt。
- 当前工具调用。
- 当前 diff。
- 当前回答。

Kiana 关注：

- 项目目标。
- 持久状态。
- 证据账本。
- 任务板。
- 长期记忆。
- 并发协作。
- 完成门禁。
- 汇报和学习。

### 7.2 为什么先做 `/project`

因为 `/project` 是差异化核心：

- `/quick` 和 `/task` 已经是常规 coding agent 能力。
- `/project` 才能形成长期闭环。
- `/swarm` 需要 Project 的 Task Card 和 WorkPacket。
- `/report` 需要 Project 的 evidence ledger。
- `/eda` 需要 Project 的流程和 gate。

### 7.3 为什么不先做 Dashboard

Dashboard 没有核心协议时只会变成展示壳。

正确顺序：

1. WorkflowRun。
2. State/EventLog。
3. Task Card。
4. Evidence Ledger。
5. Context/Recovery。
6. CLI/TUI projection。
7. Web/dashboard。

### 7.4 为什么 EDA P0 不自动画板

硬件错误成本高，且需要专业判断。

P0 更有价值的是：

- 需求捕获。
- 资料完整性。
- BOM 风险。
- DFM 检查。
- Bring-up 计划。
- 风险归档。

自动画板和自动下单应进入更晚阶段，并带强 approval。

## 8. 系统完整性检查

Kiana 的每个核心功能都要能回答：

- 输入是什么。
- 输出是什么。
- 状态存在哪里。
- 失败如何记录。
- 什么时候需要用户。
- 如何恢复。
- 如何验证。
- 如何汇报。
- 如何写入学习。

如果一个能力不能回答这些问题，就不能进入 P0。
