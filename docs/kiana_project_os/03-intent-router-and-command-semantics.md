# Volume 03: Intent Router 与命令语义

## 1. Router 目标

Intent Router 的职责不是“猜用户想要什么”这么简单。它要把用户输入映射到合适的执行档位、命令语义、风险策略和证据要求。

Router 必须同时做到：

- 小事不重流程。
- 大事不轻率。
- 高风险不绕过审批。
- 可以升级。
- 可以降级。
- 可以解释决策原因。

## 2. 输入信号

Router 使用以下信号：

| 信号 | 来源 | 用途 |
| --- | --- | --- |
| explicit command | 用户输入 `/project`、`/audit` 等 | 作为 mode hint |
| natural language intent | 用户自然语言 | 分类任务类型 |
| repo state | git、文件、脚本、测试 | 估计复杂度 |
| workflow state | `.kiana/workflows` | 判断是否 resume |
| memory state | source/confidence/stale | 辅助恢复 |
| risk terms | deploy、delete、secret、PCB order | 升级到 L5 |
| artifact presence | PRD、BOM、Gerber、issue | 选择领域模式 |
| dirty state | git dirty/untracked | 决定是否隔离 |
| task board | ready tasks | 决定是否可 swarm |

## 3. 输出

Router 输出 `RouteDecision`：

```json
{
  "schema_version": "kiana.route_decision.v1",
  "input_id": "turn_001",
  "requested_mode": "auto",
  "selected_mode": "project",
  "level": "L3",
  "domain": "software",
  "reasons": [
    "用户目标影响多个模块",
    "需要长期状态和验证门槛",
    "存在可拆分任务"
  ],
  "upgraded_from": "task",
  "downgraded_from": null,
  "requires_approval": false,
  "next_command": "/project plan",
  "evidence_requirement": "workflow_artifact"
}
```

## 4. L0-L5 档位

### 4.1 L0 Answer

触发：

- 解释概念。
- 纯建议。
- 不需要工具。
- 不需要写文件。

输出：

- 直接回答。
- 可选引用现有文档。

禁止：

- 创建 WorkflowRun。
- 写文件。
- 宣称验证过。

### 4.2 L1 Quick Action

触发：

- 单命令。
- 单文件小改。
- 低风险查询。

输出：

- command result。
- small diff。
- minimal evidence。

升级条件：

- 涉及多文件。
- 需要测试。
- 发现目标比输入复杂。
- 需要长期状态。

### 4.3 L2 Task Loop

触发：

- bugfix。
- feature slice。
- refactor。
- test addition。

输出：

- Task Card。
- ResultPacket。
- VerificationPacket。

升级条件：

- 任务依赖多个阶段。
- 需要 WBS。
- 需要跨会话继续。
- 需要多个 worker。

### 4.4 L3 Project OS

触发：

- “继续”。
- PRD/roadmap。
- 多模块目标。
- 商业化推进。
- 论文/硬件长期项目。

输出：

- WorkflowRun。
- WBS。
- Kanban。
- Task Cards。
- Evidence Ledger。

降级条件：

- 用户只是问只读状态。
- 没有可执行动作。
- 任务实际很小。

### 4.5 L4 Bounded Swarm

触发：

- Project 中有多个 Ready task。
- 文件边界清楚。
- 用户允许并行。
- 验证可以统一集成。

输出：

- WorkPackets。
- worker assignments。
- path lock table。
- integration report。

禁止：

- 自动派发冲突文件。
- worker 自由扩大 scope。
- worker 直接 merge。

### 4.6 L5 High-Risk Governance

触发：

- push/merge/deploy/release。
- 删除/重写大量文件。
- secrets/credentials。
- network upload。
- plugin/MCP/hook trust change。
- hardware order。
- high-voltage/battery/safety advice execution。

输出：

- Approval request。
- risk report。
- rollback plan。
- audit event。

禁止：

- 静默执行。
- 用用户过去的偏好替代本次 approval。

## 5. 显式命令语义

### 5.1 `/quick <prompt>`

语义：

- 用户要求快速处理。
- Router 尽量保持 L0/L1。
- 如果必须升级，必须说明原因。

错误：

- 需要写多文件：返回升级建议或直接升级。
- 需要审批：进入 L5。

### 5.2 `/task <goal>`

语义：

- 创建或复用单任务执行上下文。
- 不默认创建完整 Project WBS。

动作：

1. Build task ContextPack。
2. Define scope。
3. Execute。
4. Verify。
5. Report。

### 5.3 `/project plan <goal>`

语义：

- 创建 WorkflowRun。
- Capture goal。
- 生成 WBS/Kanban。

输出：

- workflow_id。
- task_plan.md。
- state.json。
- board summary。

### 5.4 `/project board`

语义：

- 展示当前项目任务板。

输出：

- Ready。
- Doing。
- Review。
- Blocked。
- Done。
- 每个任务的 evidence 状态。

### 5.5 `/project next`

语义：

- 选择下一步最应该做的 task。

选择规则：

1. Ready 状态。
2. 依赖满足。
3. 最高优先级。
4. 最低阻塞风险。
5. 能产生最大推进证据。

### 5.6 `/project split <task_id>`

语义：

- 把过大的 task 拆小。

输出：

- child tasks。
- dependency edges。
- parent task 状态更新。

### 5.7 `/project resume <workflow_id>`

语义：

- 恢复指定 WorkflowRun。

动作：

1. Load workflow。
2. Replay eventlog。
3. Check dirty state。
4. Refresh ContextPack。
5. Select next node。

### 5.8 `/swarm dispatch --max-workers N`

语义：

- 派发最多 N 个 worker。

前置条件：

- Project mode。
- 至少 2 个 Ready task。
- path lock 无冲突。
- verification profile 存在。

### 5.9 `/audit strict`

语义：

- 执行严格审计。

检查：

- fake/stub/TODO。
- hardcoded。
- test gap。
- release proof。
- schema drift。
- policy/trust gap。
- security issue。

### 5.10 `/report progress`

语义：

- 生成中文进度报告。

来源：

- Evidence Ledger。
- state.json。
- VerificationPacket。
- ReviewPacket。
- live scan。

禁止：

- 用 stale memory 直接生成当前状态。

### 5.11 `/eda review`

语义：

- 硬件项目审查。

输入：

- EasyEDA project。
- schematic export。
- BOM。
- Gerber。
- CPL。
- constraints。

输出：

- eda_review.json。
- bom_risk.md。
- bringup-plan.md。

## 6. 升级规则

| 从 | 到 | 条件 |
| --- | --- | --- |
| L0 | L1 | 需要读取/运行命令 |
| L1 | L2 | 需要改文件并验证 |
| L2 | L3 | 涉及多阶段、多模块、长期状态 |
| L3 | L4 | 多个 Ready task 可安全并行 |
| any | L5 | 高风险操作 |
| any | EDA domain | 硬件/PCB/BOM/Gerber/EasyEDA 信号 |

## 7. 降级规则

| 从 | 到 | 条件 |
| --- | --- | --- |
| L3 | L0 | 用户只是问状态/解释 |
| L3 | L2 | 只有一个清晰 task |
| L4 | L3 | path lock 冲突或依赖不满足 |
| L5 | lower | 用户取消高风险动作，仅保留只读分析 |

## 8. 冲突优先级

当信号冲突：

1. Safety/Policy wins。
2. Explicit user command wins over auto classification。
3. Live repo state wins over memory。
4. Project state wins over single-turn guess。
5. Lower-risk mode wins when evidence is insufficient。

## 9. Router 自解释

每次非显式 obvious 的路由都要可解释：

- selected mode。
- reasons。
- risk flags。
- why not lower。
- why not higher。
- next artifact。

示例：

```text
选择 /project：
- 目标是长期商业化推进，不是单次修复。
- 需要恢复 blocker 和验证命令。
- 涉及 docs、scripts、schema、release 多个面。
- 当前没有足够安全边界进入 /swarm。
```

## 10. Router 失败模式

### 10.1 误升级

表现：

- 小问题进入完整 WorkflowRun。
- 用户体验变慢。

修正：

- 增加降级。
- 快速输出。
- 只创建 minimal evidence。

### 10.2 误降级

表现：

- 长期目标只做了局部回答。
- 没有状态。

修正：

- 对“继续”“完整”“商用化”“项目”设复杂度下限。

### 10.3 风险漏判

表现：

- 执行了高风险操作。

修正：

- policy gate 在 tool 层再次检查。
- Router 决策不能成为最终授权。

### 10.4 EDA 误判

表现：

- 把普通软件任务当硬件。
- 把硬件任务当普通项目。

修正：

- EDA domain 需要关键词 + artifact 或用户显式命令之一。
