# Volume 04: Project Orchestrator、WBS 与 Kanban

## 1. Project Orchestrator 职责

Project Orchestrator 是 `/project` 的核心。它把长期目标变成可执行、可恢复、可验证的项目状态。

职责：

- 维护 WorkflowRun。
- 抽取 goal/success criteria。
- 建 WBS。
- 建 Task Card。
- 维护 Kanban。
- 选择 next task。
- 控制 scope。
- 生成 progress report。

不负责：

- 直接执行所有工具。
- 自己绕过 policy。
- 替 worker 写代码。
- 把失败隐藏成进展。

## 2. WBS 层级

Kiana 使用三层 WBS：

```text
Project Goal
  -> Workstream
    -> Milestone
      -> Task Card
```

### 2.1 Project Goal

字段：

- title。
- objective。
- why。
- success criteria。
- constraints。
- NOT_BUILDING。
- risk profile。

### 2.2 Workstream

用于区分方向：

- Runtime。
- Project OS。
- Memory。
- Repo Intelligence。
- Swarm。
- Policy。
- EDA。
- UI。
- Docs。

字段：

- id。
- name。
- purpose。
- owner mode。
- priority。
- dependencies。

### 2.3 Milestone

Milestone 是可验收阶段。

字段：

- id。
- workstream。
- output。
- acceptance。
- verification。
- risks。

### 2.4 Task Card

Task Card 是最小执行单位。

字段：

- task_id。
- title。
- objective。
- scope。
- dependencies。
- allowed_paths。
- forbidden_paths。
- acceptance_criteria。
- verification_commands。
- status。
- evidence。

## 3. Kanban 状态

状态：

- Draft。
- Ready。
- Doing。
- Review。
- Blocked。
- Done。
- Cancelled。

### 3.1 Draft

含义：

- 已识别，但不可执行。

进入条件：

- WBS 初步拆分。
- 缺少验收标准。
- 缺少 scope。

离开条件：

- 补齐 dependencies。
- 补齐 verification。
- 补齐 allowed/forbidden paths。

### 3.2 Ready

含义：

- 可以被执行或派发。

进入条件：

- dependencies done。
- scope 清楚。
- verification 存在。
- 无 approval blocker。

### 3.3 Doing

含义：

- 正在执行。

进入条件：

- 被主 agent 或 worker claim。
- path lock 成功。

### 3.4 Review

含义：

- 执行完成，等待审查/验证/集成。

进入条件：

- ResultPacket 存在。
- 初步 checks 完成。

### 3.5 Blocked

含义：

- 需要外部输入或风险无法继续。

blocked reasons：

- user decision。
- missing dependency。
- failing test unclear。
- dirty git conflict。
- external service unavailable。
- approval required。

### 3.6 Done

含义：

- 验收完成。

进入条件：

- VerificationPacket pass。
- no blocking ReviewPacket。
- Evidence Ledger 写入。

## 4. `/project plan`

输入：

- goal text。
- optional PRD path。
- optional issue id。
- optional constraints。

动作：

1. Capture。
2. Build goal object。
3. Extract success criteria。
4. Extract NOT_BUILDING。
5. Identify workstreams。
6. Create milestones。
7. Create initial tasks。
8. Write task_plan.md。
9. Write state.json。

输出：

- workflow_id。
- board summary。
- first Ready task。
- risks。
- next command。

失败：

- goal unclear -> clarify。
- goal too large -> split。
- high risk -> approval。

## 5. `/project board`

输出应包含：

- workflow_id。
- current goal。
- current node。
- each status column。
- task id/title。
- priority。
- blocker。
- last evidence。

示例输出结构：

```text
Workflow: wf_20260709_001
Goal: Kiana 个人项目 OS

Ready
  task_001 WorkflowRun schema
  task_002 Evidence Ledger

Doing
  task_003 Router rules

Blocked
  task_004 Plugin trust model - needs policy decision

Done
  task_000 Reference audit
```

## 6. `/project next`

选择算法：

1. 过滤 Ready task。
2. 排除 blocked dependencies。
3. 排除 path lock 冲突。
4. 按 priority 排序。
5. 按 critical path 排序。
6. 按 evidence value 排序。
7. 选择最小可交付 slice。

输出：

- selected task。
- why selected。
- why not others。
- required context。
- suggested command。

## 7. `/project split`

触发：

- task 太大。
- task scope 不清。
- task 不能在一次验证中完成。
- task 涉及多个 owners。

拆分规则：

- 每个 child task 有独立验收。
- child task 可以独立验证。
- child task 文件范围尽量不重叠。
- parent task 状态转为 blocked 或 decomposed。

## 8. Scope 管理

Task Card 必须区分：

- in_scope。
- out_of_scope。
- NOT_BUILDING。
- allowed_paths。
- forbidden_paths。

区别：

- `out_of_scope` 是需求边界。
- `NOT_BUILDING` 是明确不做的产品承诺。
- `allowed_paths` 是执行边界。
- `forbidden_paths` 是工具/worker 禁止触碰的边界。

## 9. 依赖模型

依赖类型：

- blocks。
- requires_decision。
- requires_artifact。
- requires_verification。
- requires_approval。
- related_but_not_blocking。

依赖必须可解释：

- 谁依赖谁。
- 为什么依赖。
- 完成条件是什么。

## 10. 优先级模型

优先级信号：

- 用户显式优先级。
- unblock value。
- risk reduction。
- critical path。
- verification availability。
- implementation size。
- dependency fanout。

优先级不应只按“看起来重要”。

## 11. Board 与 WorkflowRun 的一致性

Board 是 state projection，不是独立 source of truth。

更新路径：

```text
eventlog.jsonl -> state.json -> board render
```

禁止：

- 手动只改 board 不写 event。
- Done column 没有 evidence。
- Blocked column 没有 reason。

## 12. Project Report

报告结构：

- 背景。
- 当前目标。
- 已完成。
- 正在做。
- 阻塞。
- 风险。
- 下一步。
- 需要用户决策。

事实来源：

- state.json。
- Evidence Ledger。
- ReviewPacket。
- VerificationPacket。
- live scan。

禁止：

- 用 roadmap 替代进度。
- 用旧 memory 替代当前状态。
