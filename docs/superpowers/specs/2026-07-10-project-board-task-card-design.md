# Project Board 与 Task Card P0 聚焦设计

## 1. 设计依据

本设计落实以下已批准规格：

- `docs/kiana_project_os/10-p0-p1-p2-delivery-slices.md` 的 P0-M2。
- `docs/kiana_project_os/31-project-board-data-model-and-queries.md` 的 Board 投影与查询语义。
- `docs/kiana-product-functional-and-implementation-design.md` 的 F03。

本切片直接承接已经实现的 WorkflowRun 初始化、列表和安全恢复能力。

## 2. 产品目标

用户可以通过统一的 `/project` 入口查看现有任务板，并让 Kiana 选择下一项真正可执行的任务。

必须回答：

- 当前有哪些 Backlog、Spec、Ready、In Progress、Review、Blocked、Done 和 Archived 任务。
- 每个 Blocked 任务为什么被阻塞，以及解除条件是什么。
- 每个 Done 状态是否存在 Evidence。
- 下一项任务为什么被选择，其他候选为什么没有被选择。

## 3. 非目标

本切片不实现：

- 自动把自然语言目标拆成完整 WBS。
- `/project split` 和子任务创建。
- 自动执行选中的任务。
- Evidence 命令执行和 VerificationPacket 生成。
- 独立于现有 TaskCreate 的第二套任务数据库。
- Dashboard、Web UI 或 TUI Kanban 拖拽。

## 4. 方案比较

### 4.1 方案 A：在 `kiana-commands` 中直接拼 Board JSON

优点：实现最短。

缺点：状态规则、Evidence 规则和 next 排序无法被 TUI、App、Router 或后续 Swarm 复用。

结论：不采用。

### 4.2 方案 B：在 `kiana-tasks` 中建立纯领域投影，命令层只负责读取和展示

优点：

- TaskCreate 文件仍是唯一事实源。
- BoardProjection、状态规则和 next 查询可复用。
- 领域测试不依赖 CLI 或真实文件系统。
- 后续 `/swarm`、Dashboard 和 Router 可以使用同一结果。

缺点：需要新增一个小型领域模块。

结论：采用。

### 4.3 方案 C：把 `kiana-tools::Task` 整体迁移到共享 crate

优点：类型完全统一。

缺点：会扩大到工具协议、序列化兼容和大量测试，超出 P0-M2 最小切片。

结论：暂不采用；后续在 Task schema v2 时单独迁移。

## 5. 架构边界

```text
TaskCreate / TaskUpdate JSON
  -> kiana-commands task store loader
  -> kiana-tasks ProjectBoard projection
  -> Board policy validation
  -> next_task ranking
  -> ProjectCommand JSON / text output
```

职责分配：

- `kiana-tools`：创建和更新原始 Task JSON。
- `kiana-commands`：解析 cwd、task_list_id，合并磁盘与运行时任务。
- `kiana-tasks`：把原始 Task 投影为 Task Card、Board 和 next 查询结果。
- `ProjectCommand`：提供 `/project board` 与 `/project next`。

## 6. Task Card 输入契约

继续兼容现有字段：

- `id` / `task_id` / `taskId`。
- `title` / `subject` / `description`。
- `status`。
- `owner`。
- `blocks`。
- `blockedBy` / `blocked_by`。
- `created_at`。
- `updated_at`。
- `metadata`。

P0 Board 从 `metadata` 读取以下可选字段：

- `board_status` / `boardStatus`。
- `workstream_id` / `workstreamId`。
- `milestone_id` / `milestoneId`。
- `priority`，整数，默认 `0`。
- `verification_commands` / `verificationCommands`，字符串数组。
- `evidence` / `evidence_refs` / `evidenceRefs`，字符串数组。
- `blocker_reason` / `blockerReason`。
- `unblock_condition` / `unblockCondition`。
- `allowed_paths` / `allowedPaths`，字符串数组。

缺失 `id`、标题或合法状态时返回显式错误，不静默忽略损坏任务。

## 7. Board 状态模型

固定列顺序：

1. `backlog`
2. `spec`
3. `ready`
4. `in_progress`
5. `review`
6. `blocked`
7. `done`
8. `archived`

映射规则按安全优先级执行：

1. `completed` 且 Evidence 非空，投影为 `done`。
2. `completed` 且 Evidence 为空，投影为 `blocked`，记录 `completed_without_evidence`。
3. 存在未完成的 `blocked_by`，投影为 `blocked`，原因是 `dependency_blocked`。
4. 存在 `blocker_reason`，投影为 `blocked`。
5. `failed` 投影为 `blocked`，必须生成失败原因。
6. `cancelled` 或 `killed` 投影为 `archived`。
7. metadata 明确声明 `review` 时投影为 `review`。
8. `in_progress` 或 `running` 投影为 `in_progress`。
9. `pending` 且有 verification commands，投影为 `ready`。
10. `pending` 且无 verification commands，投影为 `backlog` 并记录 `ready_missing_verification` 提示。

`board_status=spec` 和 `board_status=backlog` 可以覆盖普通 pending 投影，但不能覆盖 Evidence、dependency 或 blocker 安全规则。

## 8. 数据对象

### 8.1 `ProjectTaskCard`

最小字段：

- `schema`：`kiana.project-task-card.v1`。
- `task_id`。
- `title`。
- `source_status`。
- `board_status`。
- `owner`。
- `workstream_id`。
- `milestone_id`。
- `priority`。
- `blocks`。
- `blocked_by`。
- `verification_commands`。
- `evidence`。
- `blocker_reason`。
- `unblock_condition`。
- `allowed_paths`。
- `created_at`。
- `updated_at`。
- `policy_findings`。

### 8.2 `ProjectBoardProjection`

- `schema`：`kiana.project-board.v1`。
- `task_list_id`。
- `columns`：固定顺序的列及 Task Card。
- `counts`：每列数量。
- `policy_findings`：整个任务板的违规摘要。
- `ready_task_ids`。
- `blocked_task_ids`。
- `done_task_ids`。

Board 不保存独立状态文件；每次从当前 Task 事实源重新投影。

领域 API：

```rust
pub fn build_project_board(
    task_list_id: impl Into<String>,
    tasks: &[serde_json::Value],
) -> ProjectBoardResult<ProjectBoardProjection>;

pub fn select_next_project_task(
    board: &ProjectBoardProjection,
) -> ProjectNextTaskReport;
```

### 8.3 `ProjectNextTaskReport`

- `schema`：`kiana.project-next.v1`。
- `task_list_id`。
- `mode`：P0 固定为 `balanced`。
- `selected_task`：可为空。
- `why`：选择理由。
- `alternatives`：其他 Ready 候选及未选原因。
- `blocked_summary`。
- `primary_blockers`：按 blocker severity、数量和 code 稳定排序的主要阻塞原因。

## 9. `next_task` 排序规则

只允许 `ready` Task 进入候选集。

排序键依次为：

1. `priority` 降序。
2. `blocks` 数量降序，优先解除更多下游任务。
3. `updated_at` 升序，防止长期饥饿。
4. `task_id` 字典序，保证确定性。

选择理由至少包含：

- `ready`。
- `verification commands available`。
- `priority=<n>`。
- `unblocks=<n>`。
- `updated_at=<n>`。
- `task_id=<id>`。

其他候选的 `why_not` 必须指出第一个实际决胜条件：`lower_priority`、`lower_unblocks`、`newer_update` 或 `task_id_tiebreak`。

如果没有 Ready Task，`selected_task` 为 `null`，输出当前 blocked/backlog 数量和按严重度稳定排序的 `primary_blockers`，不把 Backlog 伪装成可执行任务。

## 10. 命令契约

### 10.1 `/project board`

```text
kiana project board [--json] [task_list_id]
```

JSON 输出：`kiana.project-board.v1`。

文本输出包含：

- task list id。
- 各列数量。
- Ready 和 Blocked 的前若干 Task。
- policy finding 数量。

### 10.2 `/project next`

```text
kiana project next [--json] [task_list_id]
```

JSON 输出：`kiana.project-next.v1`。

文本输出包含：

- selected task。
- why。
- alternatives 数量。
- 没有 Ready Task 时的明确说明。

### 10.3 `/project status`

本切片暂时不新增独立实现。用户可以继续使用 `kiana tasks workflow list/continue/show`；后续 F01 聚合切片再把 Workflow 与 Board 合并成一个项目状态摘要。

## 11. 错误与一致性

- 损坏的 Task JSON 由现有 loader 返回文件路径和解析错误。
- 缺失关键 Task 字段由 Board 领域层返回 task index 和原因。
- 已出现但类型错误的 `board_status`、`priority`、时间戳和字符串 metadata 字段必须返回显式错误，不得静默降级。
- `blocked_by` 引用不存在的 Task 时，Task 保持 blocked，并记录 `missing_dependency_reference`。
- `blocked_by` 指向已经 completed 且有 Evidence 的 Task 时，该依赖视为已解决。
- 重复 task id 返回错误，不选择任意一条。
- task list id 使用与磁盘目录相同的规范化身份，避免读取路径与输出 ID 不一致。
- Board 和 Task Card 的 policy findings 使用稳定排序，输入任务顺序不能改变结果。
- Board 投影不得修改原始 Task 文件。

## 12. 验收标准

- `/project` 在默认命令注册表中可发现。
- `/project board --json` 能投影现有 TaskCreate 文件。
- pending + verification 的任务进入 Ready。
- 未完成依赖使任务进入 Blocked。
- completed 无 Evidence 不能进入 Done。
- completed 有 Evidence 才能进入 Done。
- `/project next --json` 只选择 Ready，并给出确定性理由。
- 没有 Ready 时返回可解释的 `selected_task: null`。
- 领域测试、命令测试、完整 workspace 离线测试、格式检查和 diff 检查全部通过。

## 13. 后续扩展点

- `/project split` 写入 child Task 和 dependency edges。
- Board 与 WorkflowRun 的 project_id/workflow_id 关联。
- Evidence Ledger 将 VerificationPacket 引用写入 Task metadata。
- `ready_parallel` 增加 allowed path 冲突评分。
- Dashboard 直接消费 `ProjectBoardProjection`。
