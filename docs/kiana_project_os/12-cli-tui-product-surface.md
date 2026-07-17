# Volume 12: CLI/TUI 产品面

## 1. 产品面目标

CLI/TUI 是 Kiana P0 的主要用户界面。它不是底层逻辑本身，而是 WorkflowRun、Task Board、Evidence Ledger、Policy Gate 的投影。

目标：

- 快速输入。
- 清楚显示当前模式。
- 清楚显示任务状态。
- 清楚显示验证证据。
- 清楚显示阻塞和风险。
- 避免用户被内部协议淹没。

## 2. CLI 原则

### 2.1 默认输出短

默认只展示：

- 当前做了什么。
- 结果是什么。
- evidence 在哪里。
- 下一步是什么。

详细内容通过 flags：

- `--json`
- `--verbose`
- `--show-evidence`
- `--show-events`
- `--show-risk`

### 2.2 所有命令可脚本化

每个核心命令应支持 JSON 输出。

用途：

- CI。
- smoke。
- tests。
- app-server。
- dashboard。

### 2.3 人类摘要和机器输出分离

示例：

```text
/project next
```

人类输出：

```text
Next: task_003 Router decision reasons
Why: dependencies clear, highest unblock value, verification available
Run: /task task_003
```

机器输出：

```json
{
  "selected_task": "task_003",
  "reasons": ["dependencies_clear", "highest_unblock_value"],
  "next_command": "/task task_003"
}
```

## 3. 命令族

### 3.1 Project commands

- `/project plan <goal>`
- `/project status`
- `/project board`
- `/project next`
- `/project split <task_id>`
- `/project resume <workflow_id>`
- `/project report`

### 3.2 Context commands

- `/context packet`
- `/context search <query>`
- `/context repo-map`
- `/context impact <path-or-symbol>`
- `/context ingest --source <path>`

### 3.3 Swarm commands

- `/swarm dispatch --max-workers N`
- `/swarm status`
- `/swarm integrate`
- `/swarm stop <worker_id>`

### 3.4 Audit/Verify commands

- `/audit strict`
- `kiana validate`
- `kiana checks`
- `kiana review`

### 3.5 Report commands

- `/report progress`
- `/report blockers`
- `/report handoff`
- `/report teacher`

### 3.6 EDA commands

- `/eda review`
- `/eda bom`
- `/eda dfm`
- `/eda bringup-plan`

### 3.7 Trust commands

- `/policy status`
- `/policy explain <event_id>`
- `/plugin status`
- `/mcp status`
- `/hooks status`

## 4. TUI 信息架构

TUI 只展示状态投影：

```text
┌─────────────────────────────────────────────┐
│ Kiana Project: Personal Project OS          │
│ Mode: /project L3     Workflow: wf_001      │
├─────────────────────────────────────────────┤
│ Board                                       │
│ Ready: 3  Doing: 1  Review: 0  Blocked: 2  │
├─────────────────────────────────────────────┤
│ Current Task                                │
│ task_003 Intent Router Decision Rules       │
├─────────────────────────────────────────────┤
│ Evidence                                    │
│ Last check: pass  Command: cargo test ...   │
├─────────────────────────────────────────────┤
│ Risks                                       │
│ policy: ok  dirty: user_dirty caution       │
└─────────────────────────────────────────────┘
```

## 5. 状态条

状态条字段：

- mode。
- workflow_id。
- current task。
- git dirty。
- blockers count。
- verification status。
- policy risk。
- worker count。

状态条不能隐藏 blocker。

## 6. Tool Cards

工具调用显示：

- tool name。
- status。
- target。
- duration。
- risk。
- output summary。

失败显示：

- exit code。
- failure class。
- suggested next。

## 7. Evidence View

Evidence View 展示：

- latest command。
- checks。
- review findings。
- approvals。
- blockers。

每条 evidence 可展开：

- event id。
- timestamp。
- payload summary。
- source。

## 8. Board View

Board View 列：

- Ready。
- Doing。
- Review。
- Blocked。
- Done。

Task Card 显示：

- id。
- title。
- priority。
- risk。
- evidence status。

## 9. Prompt 入口

用户可以输入：

- 自然语言。
- slash command。
- task id。
- workflow id。
- file path。

Prompt 入口要显示 router 决策：

```text
Route: /task -> /project
Reason: long-running goal with multiple milestones
```

## 10. Approval UI

Approval 必须清楚：

- 将执行什么。
- 为什么需要。
- 风险。
- 回滚。
- exact operation。
- scope。

选项：

- approve once。
- approve for workflow。
- reject。
- ask for more info。

## 11. Report UI

报告模式：

- short。
- detailed。
- teacher。
- manager。
- handoff。

报告必须标注：

- evidence-based。
- unknown。
- blocked。
- memory-derived。

## 12. EDA UI

EDA 显示：

- artifact list。
- schematic issues。
- BOM risk。
- DFM issues。
- bring-up steps。
- approval required。

硬件风险必须高亮。

## 13. 错误体验

错误输出包括：

- what failed。
- why it matters。
- evidence。
- next action。
- whether user input is required。

不要只输出 stack trace。

## 14. CLI/TUI 验收

P0 验收：

- `/project board` 可读。
- `/project next` 可解释。
- `/report progress` 可直接汇报。
- `/audit strict` finding 可追溯。
- approval prompt 不模糊。
- JSON 输出可被测试。
