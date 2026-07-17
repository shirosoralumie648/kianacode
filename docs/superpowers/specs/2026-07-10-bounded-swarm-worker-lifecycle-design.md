# Bounded Swarm Worker 生命周期设计

## 1. 设计目标

本切片把已经持久化的 `kiana.swarm-workpacket.v1` 从静态派发产物推进为可运行、可观察、可取消、可恢复的本地 worker 生命周期。

它必须解决五个问题：

1. 一个 dispatch 如何确定性地启动多个 worker。
2. CLI 进程退出或崩溃后，如何重新发现 worker 状态。
3. timeout、输出上限、scope violation 和用户取消如何形成可审计终止事实。
4. worker 退出后如何形成不可变 ResultPacket，而不是只留下日志。
5. 如何保证主工作树不被 worker 直接修改，且不自动 merge、push 或 commit。

## 2. 非目标

本切片明确不实现：

- 自动合并 worker diff。
- 自动解决冲突。
- 自动 push、merge、deploy 或发布。
- 云端 worker 调度。
- 任意数量的无限 swarm。
- 把进程退出码等同于功能完成。
- 在没有证据时填写 commands、tests 或 changed files。
- 对真实模型供应商凭据、额度或在线可用性作保证。

## 3. 方案选择

### 3.1 方案 A：直接调用交互式 AgentTool

优点：

- 已有 agent prompt、后台进程和 worktree 逻辑。
- 与现有交互式 agent 体验一致。

缺点：

- 强依赖 ToolContext、会话状态、权限模式和交互式工具注册表。
- WorkflowRun 无法独立恢复其 worker 状态。
- ResultPacket、预算和 dispatch identity 会被埋在 AgentTool 私有 artifact 中。

结论：不作为 Swarm runtime 的核心依赖。

### 3.2 方案 B：进程内 Tokio task

优点：

- 启动和取消简单。
- 可以直接持有 `Child` handle。

缺点：

- CLI 退出后 handle 丢失。
- crash/resume 无法成立。
- 不能成为长期运行、可审计的商业化执行模型。

结论：拒绝。

### 3.3 方案 C：持久 Workflow 状态 + 外部 runner 适配器

优点：

- runner 与 Workflow 状态机解耦。
- worker PID、日志、退出码、隔离目录和 ResultPacket 都可在 CLI 重启后恢复。
- 默认 runner 可调用 Kiana 自身，测试可注入确定性 executable。
- 后续可以增加本地模型、远程 worker 或 EDA runner，而不改变 Workflow 数据契约。

缺点：

- 需要定义明确的 runner 环境变量和 artifact 协议。
- 跨平台进程组终止需要适配。

结论：采用方案 C。

## 4. 生命周期状态机

每个 worker 的状态固定为：

```text
prepared
  -> starting
  -> running
  -> completed
  -> failed
  -> timeout
  -> budget_exhausted
  -> scope_violation
  -> cancelled
  -> lost
```

规则：

- `prepared`：不可变 launch artifact 已提交，但进程尚未确认。
- `starting`：runner 已 spawn，PID artifact 尚未确认。
- `running`：PID 存在且进程仍存活，exit artifact 不存在。
- `completed`：exit code 为 0，且没有 budget/scope 终止覆盖。
- `failed`：exit code 非 0。
- `timeout`：运行时间超过 `timeout_seconds`。
- `budget_exhausted`：stdout + stderr 超过 `max_output_bytes`，或结构化 telemetry 超过 `max_commands`。
- `scope_violation`：隔离目录的变更超出 packet path locks。
- `cancelled`：用户明确执行 cancel，且终止动作已记录。
- `lost`：PID 不存活、exit artifact 缺失，无法证明正常退出。

终态不可回退到运行态。重试必须创建新的 attempt identity，而不是覆盖旧状态。

## 5. Artifact 布局

WorkflowRun 内新增：

```text
workers/<dispatch_id>/
  manifest.json
  <task_id>/
    launch.json
    prompt.md
    runner.sh
    state.json
    pid
    started_at_ms
    exit_code
    finished_at_ms
    stdout.log
    stderr.log
    telemetry.json
    result.json
```

其中：

- `manifest.json`、`launch.json`、`prompt.md`、`runner.sh` 是不可变 launch artifacts。
- `pid`、时间戳、exit code 和日志是运行事实。
- `state.json` 是可重建 projection，不是唯一事实源。
- `result.json` 是不可变 `kiana.swarm-result-packet.v1`。

## 6. Schema

### 6.1 Execution Manifest

```json
{
  "schema": "kiana.swarm-execution-manifest.v1",
  "dispatch_id": "dispatch-...",
  "workflow_id": "wf-...",
  "run_id": "run-...",
  "runner_mode": "kiana_cli",
  "runner_fingerprint": "sha256:...",
  "worker_ids": ["worker-dispatch-task"],
  "launch_paths": ["workers/.../launch.json"],
  "created_at_ms": 0,
  "next_action": "run_swarm_monitor"
}
```

### 6.2 Worker Launch

```json
{
  "schema": "kiana.swarm-worker-launch.v1",
  "worker_id": "worker-dispatch-task",
  "dispatch_id": "dispatch-...",
  "task_id": "task-a",
  "workpacket_path": "workpackets/.../task-a.json",
  "isolation_mode": "worktree",
  "isolation_strategy": "git_worktree|snapshot_copy",
  "isolation_path": ".kiana/swarm-worktrees/...",
  "budget": {},
  "created_at_ms": 0
}
```

### 6.3 Worker State

```json
{
  "schema": "kiana.swarm-worker-state.v1",
  "worker_id": "worker-dispatch-task",
  "status": "running",
  "pid": 1234,
  "attempt": 1,
  "started_at_ms": 0,
  "finished_at_ms": null,
  "exit_code": null,
  "termination_reason": null,
  "output_bytes": 0,
  "result_path": null,
  "updated_at_ms": 0
}
```

### 6.4 ResultPacket

```json
{
  "schema": "kiana.swarm-result-packet.v1",
  "worker_id": "worker-dispatch-task",
  "dispatch_id": "dispatch-...",
  "task_id": "task-a",
  "status": "completed",
  "termination_reason": "completed",
  "exit_code": 0,
  "changed_files": [],
  "commands_run": [],
  "acceptance_evidence": [],
  "scope_deviations": [],
  "stdout_path": "workers/.../stdout.log",
  "stderr_path": "workers/.../stderr.log",
  "started_at_ms": 0,
  "finished_at_ms": 0,
  "notes": [],
  "next_action": "swarm_integration_review"
}
```

## 7. Runner 协议

默认 runner：

- executable 为当前 `kiana` 二进制。
- 参数为 `-p <prompt>`。
- cwd 为 worker 隔离目录。

测试和受控部署可以通过 `KIANA_SWARM_WORKER_EXECUTABLE` 替换 executable。替换值必须：

- 是已存在的普通文件。
- 在 Unix 上具有 executable 权限。
- 被 canonicalize 后记录 SHA-256 fingerprint。
- 不把原始环境变量内容写入公开 JSON。

runner 接收：

- `KIANA_SWARM_WORKER_ID`
- `KIANA_SWARM_WORKFLOW_ID`
- `KIANA_SWARM_DISPATCH_ID`
- `KIANA_SWARM_TASK_ID`
- `KIANA_SWARM_WORKPACKET_PATH`
- `KIANA_SWARM_RESULT_PATH`
- `KIANA_SWARM_TELEMETRY_PATH`
- `KIANA_SWARM_ALLOWED_ROOTS`

runner 可以写 telemetry，但 controller 不相信 runner 自报成功。最终状态仍由 PID、exit code、budget、scope diff 和 ResultPacket gate 决定。

## 8. 隔离策略

选择规则：

1. 项目是 Git repository、存在 HEAD、且排除 `.kiana/` 后工作树干净：使用 detached `git_worktree`。
2. 工作树有 tracked/untracked 用户改动：使用 `snapshot_copy`，确保 worker 看到当前真实代码，而不是旧 HEAD。
3. 非 Git 项目：使用 `snapshot_copy`。

snapshot 默认排除：

- `.git`
- `.kiana`
- `.claude/worktrees`
- `target`
- `node_modules`
- `.venv`
- `dist`
- `build`

这些目录不得出现在 WorkPacket allowed path 中；若出现则 start fail closed，要求用户调整 task scope 或后续提供专用 isolation adapter。

## 9. 命令契约

```text
kiana tasks swarm start --workflow <run_id> --dispatch <dispatch_id> [--json]
kiana tasks swarm status --workflow <run_id> --dispatch <dispatch_id> [--json]
kiana tasks swarm monitor --workflow <run_id> --dispatch <dispatch_id> [--json]
kiana tasks swarm cancel --workflow <run_id> --dispatch <dispatch_id> [--task <task_id>] [--json]
```

语义：

- `start`：准备 immutable launch artifacts，创建隔离目录并启动尚未启动的 workers；重复调用幂等。
- `status`：只读 projection，不终止进程、不写 ResultPacket。
- `monitor`：执行一次 reconcile；应用 timeout/output/scope gate，并为终态 worker 提交 ResultPacket。
- `cancel`：终止指定 task 或 dispatch 全部运行 worker，记录 cancelled 事实，再由 monitor 收口 ResultPacket。

## 10. 恢复模型

### 10.1 crash after prepare, before spawn

重跑 `start`，发现 launch artifact 存在但 PID 不存在，重新 spawn。

### 10.2 crash after spawn, before WorkerStarted event

runner 第一条动作写 `pid` 和 `started_at_ms`。重跑 `start` 或 `monitor` 检查 PID 存活并补写事件。

### 10.3 PID dead, no exit artifact

标记 `lost`，ResultPacket 使用 `worker_failed`，不得猜测 exit code。

### 10.4 result artifact exists, event missing

使用 Workflow immutable artifact commit 原语补交 `ResultPacketCreated` event。

### 10.5 event exists, result artifact missing

报告 Workflow inconsistent state，fail closed。

## 11. Policy 与安全边界

- start 前重新执行 Workflow state/eventlog consistency gate。
- dispatch 必须已存在且 schema/identity 匹配。
- launch path、task ID、dispatch ID 必须通过安全路径校验。
- runner override 只允许 executable path，不接受 shell command 字符串。
- worker 不获得自动 commit、push、merge、deploy 权限。
- cancel 只作用于记录的 worker PID/process group。
- 所有用户可见 JSON 仅输出相对 artifact/isolation path。
- ResultPacket 记录事实，不把 exit code 0 写成 acceptance criteria pass。

## 12. 验收标准

本切片完成必须满足：

1. 两个独立 WorkPacket 可并行启动确定性 fixture workers。
2. `status` 可在新的 CLI 调用中恢复 PID/exit 状态。
3. 重复 `start` 不重复启动已运行或已终止 worker。
4. `cancel` 可终止一个 worker，不影响另一个 worker。
5. `monitor` 能处理 completed、failed、timeout、output budget、scope violation 和 lost。
6. 每个终态 worker 只产生一个不可变 ResultPacket 和一个唯一 event。
7. 主工作树 Task 文件和源文件在 fixture 冒烟中保持不变。
8. 不创建 commit、push、merge 或 deploy 证据。
9. 全 workspace 测试、格式检查、diff check 和 CLI build 通过。
