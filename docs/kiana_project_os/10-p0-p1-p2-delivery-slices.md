# Volume 10: P0/P1/P2 交付切片

## 1. 总体原则

P0 不是功能愿望清单，而是可实现、可测试、可验收的切片。

每个切片必须有：

- 命令。
- schema。
- 状态。
- 测试。
- 验收输出。
- failure path。

## 2. P0-M1 WorkflowRun + Data Contracts

### 2.1 目标

建立项目 OS 的持久运行单元。

### 2.2 命令

- `/project plan <goal>`
- `/project status`
- `/project resume <workflow_id>`

### 2.3 Schema

- workflow.yaml。
- state.json。
- eventlog.jsonl。
- task_card.json。

### 2.4 行为

`/project plan`：

1. 创建 workflow id。
2. 创建 artifact directory。
3. 写 workflow.yaml。
4. 写 state.json。
5. 写 eventlog。
6. 输出 workflow summary。

`/project status`：

1. 读取 state。
2. 校验 last event。
3. 渲染当前节点。

`/project resume`：

1. replay eventlog。
2. rebuild state。
3. check dirty state。
4. choose next node。

### 2.5 测试

- 创建 workflow。
- 重复 resume 幂等。
- eventlog 可回放。
- state 不一致可修复。
- dirty git 会 block 或 caution。

### 2.6 验收

- `.kiana/workflows/<id>/` 存在。
- `/project status` 能解释 current node。
- crash 后可恢复。

## 3. P0-M2 Project Board + Task Card

### 3.1 目标

把长期目标拆成可执行任务板。

### 3.2 命令

- `/project board`
- `/project next`
- `/project split <task_id>`

### 3.3 Schema

- TaskCard。
- Kanban state。
- Dependency edge。

### 3.4 行为

`/project board`：

- 展示列。
- 展示 blockers。
- 展示 evidence 状态。

`/project next`：

- 选择 Ready task。
- 解释原因。

`/project split`：

- 创建 child tasks。
- 维护 dependencies。

### 3.5 测试

- Ready selection。
- dependency blocking。
- blocked reason required。
- Done requires evidence。

### 3.6 验收

- 一个目标可拆 WBS。
- board 可恢复。
- next task 可解释。

## 4. P0-M3 Evidence Ledger + Verification Gate

### 4.1 目标

让 Done/Blocked 有证据。

### 4.2 命令

- `kiana validate`
- `/audit strict`
- `/report progress`

### 4.3 Schema

- EvidenceEvent。
- ResultPacket。
- VerificationPacket。
- ReviewPacket。

### 4.4 行为

`kiana validate`：

- 运行 profile。
- 捕获 pass/fail。
- 写 evidence。

`/audit strict`：

- 扫 fake/stub/TODO/hardcoded。
- 查 tests/release/security/policy。

`/report progress`：

- 从 evidence 生成中文报告。

### 4.5 测试

- pass command 记录。
- fail command 记录。
- blocked report。
- skipped check requires reason。
- report 不读取 stale memory 当事实。

### 4.6 验收

- Done 必须有 evidence。
- Blocked 必须有 reason。
- 完成声明可追溯。

## 5. P0-M4 Context Pack + Recovery

### 5.1 目标

让“继续”真正可用。

### 5.2 命令

- `/context packet`
- `/context search <query>`
- `/project resume <workflow_id>`

### 5.3 Schema

- ContextPack。
- Memory source/confidence。
- Project fingerprint。

### 5.4 行为

`/context packet`：

- 收集 live evidence。
- 收集 relevant memory。
- 标注 stale。
- 生成上下文包。

`/project resume`：

- 检查 state。
- 检查 git。
- 检查 memory。
- 选择下一步。

### 5.5 测试

- stale memory 降权。
- dirty git 分类。
- crash 恢复。
- fork 生成新 workflow。

### 5.6 验收

- 用户说“继续”能恢复。
- 不覆盖用户修改。
- next task 有理由。

## 6. P1 切片

### 6.1 Bounded Swarm

命令：

- `/swarm dispatch --max-workers N`
- `/swarm status`
- `/swarm integrate`

验收：

- path lock 防冲突。
- worker ResultPacket。
- integration gate。

### 6.2 Repo Intelligence

命令：

- `/context repo-map`
- `/context impact <symbol-or-path>`

验收：

- ranked files。
- impacted tests。
- confidence。

### 6.3 Memory Ingest

命令：

- `/context ingest --source <path>`
- `/memory propose`

验收：

- source/confidence。
- stale handling。
- no low-confidence auto inject。

### 6.4 Rules/Skills Injection

命令：

- `/rules status`
- `/skills active`

验收：

- mode/path/task 条件注入。
- conflict explain。

### 6.5 `/eda review`

命令：

- `/eda review`
- `/eda bom`
- `/eda bringup-plan`

验收：

- BOM risk。
- DFM checklist。
- bring-up plan。
- approval gate。

## 7. P2 切片

### 7.1 Dashboard

功能：

- workflow board。
- evidence timeline。
- memory graph。
- worker status。

### 7.2 Cloud Workspace

功能：

- remote session。
- worker pool。
- artifact sync。
- web console。

### 7.3 Plugin Marketplace Trust

功能：

- install receipt。
- signature/hash。
- policy review。
- disabled plugin enforcement。

### 7.4 Cost and Usage

功能：

- token estimate。
- worker cost。
- command duration。
- report。

## 8. 里程碑验收顺序

推荐顺序：

1. P0-M1。
2. P0-M2。
3. P0-M3。
4. P0-M4。
5. P1 swarm。
6. P1 repo intelligence。
7. P1 memory。
8. P1 EDA。
9. P2 dashboard。

不要先做 P2 UI。

## 9. Definition of Done

一个 milestone Done 需要：

- schema landed。
- commands visible。
- tests pass。
- failure path exists。
- docs updated。
- evidence generated。
- no stale claim。
