# Volume 07: Bounded Swarm 与 Worker 模型

## 1. Bounded Swarm 定义

Bounded Swarm 是有界并行执行，不是无限多 agent 群聊。它的目标是提高吞吐，同时控制冲突、成本、风险和验证复杂度。

核心原则：

- 只有 Ready task 才能派发。
- 每个 worker 只拿一个 WorkPacket。
- WorkPacket 有明确文件边界。
- 主 agent 负责集成。
- worker 不直接 merge。
- 并行后必须统一验证。

## 2. Swarm 触发条件

必须满足：

- WorkflowRun 存在。
- 至少 2 个 Ready task。
- tasks dependencies satisfied。
- allowed files 不重叠。
- verification profile 存在。
- user 或 policy 允许并行。

禁止触发：

- dirty state 未分类。
- path overlap。
- shared schema 高风险未隔离。
- release/deploy 任务。
- secrets/policy 任务。

## 3. WorkPacket

WorkPacket 是 worker 的唯一输入。

`tasks swarm plan` 当前只生成 `kiana.swarm-workpacket-preview.v1`。preview 用于用户审阅派发边界，不是 worker 可消费的持久 WorkPacket，也不能写入 dispatch event。后续 dispatch 必须补齐 `task_id`、`workflow_id`、context summary、budget、最终 path locks 和不可变 artifact identity 后，才能生成正式 WorkPacket。

字段：

- workpacket_id。
- task_id。
- goal。
- context summary。
- allowed files。
- forbidden files。
- commands。
- acceptance criteria。
- rollback。
- review focus。
- budget。
- retry policy。

Worker 不应读取整个项目上下文，除非 WorkPacket 允许。

## 4. Worker 类型

| 类型 | 用途 |
| --- | --- |
| builder | 实现任务 |
| tester | 补测试/验证 |
| reviewer | 只读审查 |
| researcher | 查资料/代码模式 |
| eda_reviewer | 硬件审查 |

P0 可先只支持 builder/reviewer 的本地模拟模型。

## 5. Path Lock

Path Lock 目标：

- 避免 worker 同时写同一文件。
- 避免写共享高风险文件。
- 提前发现冲突。

Lock 类型：

- read。
- write。
- exclusive。
- forbidden。

冲突规则：

- write/write 同路径冲突。
- write/exclusive 父子路径冲突。
- shared schema 文件默认 high risk。
- Cargo.lock/package lock 需要主 agent 集成。

## 6. Dispatch Algorithm

步骤：

1. Load board。
2. Select Ready tasks。
3. Sort by priority。
4. Compute file scope。
5. Build path lock table。
   - 同时保存逻辑 scope 与解析 symlink 后的真实相对路径。
   - 真实路径逃逸项目根或遇到 broken symlink 时拒绝任务。
6. Drop conflicting tasks。
   - 未批准、高风险、release/deploy/secrets/policy 任务先由 policy gate 拒绝。
7. Limit by max_workers。
8. `plan` 创建 WorkPacket previews；`dispatch` 创建正式 WorkPackets 与 manifest。
9. `dispatch` 在 Workflow writer lease 内提交不可变 artifacts，最后写唯一 dispatch event。
10. `start` 创建隔离目录并启动 workers；`monitor` 收口预算、scope 和 ResultPacket。

输出：

- dispatched。
- skipped with reason。
- path lock table。

当前真实命令：

```text
kiana tasks swarm plan [--json] [--max-workers <2..32>] [task_list_id]
kiana tasks swarm dispatch --workflow <run_id> [--json]
  [--max-workers <2..32>]
  [--max-attempts <1..3>]
  [--max-commands <1..100>]
  [--timeout-seconds <1..86400>]
  [--max-output-bytes <1024..104857600>]
  [task_list_id]
kiana tasks swarm start --workflow <run_id> --dispatch <dispatch_id> [--json]
kiana tasks swarm status --workflow <run_id> --dispatch <dispatch_id> [--json]
kiana tasks swarm monitor --workflow <run_id> --dispatch <dispatch_id> [--json]
kiana tasks swarm cancel --workflow <run_id> --dispatch <dispatch_id> [--task <task_id>] [--json]
```

`dispatch` 使用确定性 SHA-256 identity，写入 `workpackets/<dispatch_id>/`。相同输入重试时复用原 `WorkPacketCreated` 事件；内容不一致时 fail closed。`dispatch` 自身只持久化，下一动作是 `run_swarm_start`。

`start` 提交 `kiana.swarm-execution-manifest.v1`、worker launch/prompt/runner artifacts 和唯一 prepared/start events。干净 Git 项目使用 detached worktree；脏工作树或非 Git 项目使用 snapshot copy。`status` 只读；`monitor` 生成不可变 `kiana.swarm-result-packet.v1`；`cancel` 支持单 worker 或整个 dispatch。

## 7. Worker 执行约束

Worker 必须：

- 读取 WorkPacket。
- 声明理解 scope。
- 只改 allowed files。
- 运行指定命令或说明无法运行。
- 输出 ResultPacket。

Worker 禁止：

- 改 forbidden files。
- 扩大目标。
- commit。
- push。
- merge。
- 安装未批准依赖。
- 修改 policy。

## 8. ResultPacket

ResultPacket 包含：

- changed files。
- commands run。
- pass/fail。
- errors。
- scope deviations。
- notes。
- suggested next。

ResultPacket 是事实包，不是成功声明。

## 9. Integration

主 agent 集成步骤：

1. 收集 ResultPackets。
2. 检查 path lock 是否被违反。
3. 检查 dirty state。
4. 合并非冲突 diff。
5. 对冲突 task 进入 triage。
6. 运行统一 verification。
7. 生成 integration summary。

## 10. Conflict 类型

| 冲突 | 处理 |
| --- | --- |
| same file write | stop integration, triage |
| lock file changed | main agent review |
| schema changed | run broader tests |
| behavior conflict | review synthesis |
| test conflict | fix loop |
| policy conflict | approval gate |

## 11. Worker Budget

Budget 包括：

- max time。
- max tool calls。
- max changed files。
- max retries。
- max output size。

超预算：

- stop worker。
- collect partial result。
- mark blocked or retry。

## 12. Termination

Worker 终止条件：

- completed。
- failed。
- blocked。
- timeout。
- scope violation。
- user stop。

每个终止必须写 event。

## 13. Review After Swarm

并行结果必须经过：

- integration review。
- verification gate。
- review synthesis。
- evidence ledger。

不能：

- worker 自报成功就 Done。
- 单个 worker 测试通过就整体通过。

## 14. Scaling Path

P0：

- 单进程模拟 worker。
- WorkPacket 文件。
- path overlap 检查。

P1：

- typed local worker。
- worktree isolation。
- budget/termination。

P2：

- remote worker pool。
- cloud workspace。
- dashboard。

## 15. 当前第一版实现：Pre-Dispatch Planner

当前已实现入口：

```text
kiana tasks swarm plan [--json] [--max-workers <1..32>] [task_list_id]
```

输出 schema：`kiana.swarm-plan.v1`。

第一版执行顺序：

1. 读取现有 Task files。
2. 使用 Project Board 规则投影 Ready task。
3. 按 priority 降序、created_at 升序、task_id 升序稳定排序。
4. 校验 `allowed_paths`。
5. 将 glob 收缩为保守 path root。
6. 计算 write/exclusive path locks。
7. 跳过父子路径重叠的任务。
8. 应用 `max_workers`。
9. 为选中任务生成 inline `kiana.swarm-workpacket-preview.v1`。
10. 输出 assignments、skipped、path locks、summary 和 next action。

Typed skip reason：

| reason | 含义 |
| --- | --- |
| `missing_allowed_paths` | Task 没有声明写入边界 |
| `invalid_allowed_path` | Task 使用绝对路径、父目录逃逸或空 scope |
| `path_conflict` | 与已选择任务的 path lock 冲突 |
| `approval_required` | 任务要求批准但尚无 approved 记录 |
| `restricted_task_type` | release/deploy/secrets/policy 禁止并行派发 |
| `high_risk_task` | high/critical 风险任务禁止并行派发 |
| `worker_limit` | 超出本次 worker 上限 |
| `insufficient_parallelism` | 最终不足两个安全候选，不能称为 swarm |

高风险 lock file 使用 exclusive mode：

- `Cargo.lock`。
- `package-lock.json`。
- `pnpm-lock.yaml`。
- `yarn.lock`。
- `uv.lock`。

Plan 当前边界：

- `execution_mode = plan_only`。
- 不启动 worker。
- 不创建 worktree。
- 不修改 Task status/owner。
- 不写 WorkflowRun event。
- 不持久化正式 WorkPacket。
- 不 merge、commit、push。

Dispatch 持久化当前边界：

- `execution_mode = persist_only`。
- 写入 `kiana.swarm-workpacket.v1` 与 `kiana.swarm-dispatch-manifest.v1`。
- 使用 Workflow writer lease 和唯一 `WorkPacketCreated` event 提交。
- 相同 dispatch 幂等重试，不增长 event sequence。
- 正式 packet 包含 worker budget 与 termination policy。
- `dispatch` 命令本身不启动 worker、不创建 worktree、不修改 Task、不集成 diff。

Worker lifecycle 当前边界：

- 支持 start/status/monitor/cancel。
- 支持 detached worktree 与 snapshot copy。
- 支持 PID/exit/log/state crash recovery projection。
- 支持 timeout、输出字节、command telemetry 和 scope violation gate。
- 支持幂等 ResultPacket/EventLog 提交。
- 不自动集成 diff，不修改主工作树，不 commit/push/merge/deploy。

因此当前已经证明“任务可安全并行、dispatch 可恢复持久化、worker 可受控执行并形成事实包”。下一切片是 serial integration gate、统一 verification 和隔离目录清理策略。
