# Bounded Swarm Dispatch 持久化实施计划

> 本计划承接已验证的 `kiana.swarm-plan.v1` 预派发切片。目标是提交可恢复、可审计、不可覆盖的 dispatch artifacts；本切片仍不启动 worker、不创建 worktree、不修改 Task 状态、不执行 merge/commit。

## 1. 目标

新增命令：

```text
kiana tasks swarm dispatch --workflow <run_id> [--json]
  [--max-workers <2..32>]
  [--max-attempts <1..3>]
  [--max-commands <1..100>]
  [--timeout-seconds <1..86400>]
  [--max-output-bytes <1024..104857600>]
  [task_list_id]
```

命令必须：

1. 恢复并校验指定 WorkflowRun，拒绝 eventlog/state 不一致的运行。
2. 从 Project Board 重新生成最新 SwarmPlan，不接受调用方上传旧 plan。
3. 只在 `status=ready` 时创建 dispatch。
4. 为每个 assignment 生成自包含 `kiana.swarm-workpacket.v1`。
5. 生成一个 `kiana.swarm-dispatch-manifest.v1`。
6. 在 Workflow writer lease 内提交不可变 artifacts，最后追加唯一 `WorkPacketCreated` 事件。
7. 相同输入重试时返回同一 dispatch/event，不追加重复事件。
8. 输出 `execution_mode=persist_only` 和 `next_action=run_swarm_start`。

## 2. 不做范围

- 不启动 Agent/Bash worker。
- 不创建或删除 worktree。
- 不修改 Task status/owner。
- 不写 ResultPacket、VerificationPacket 或 ReviewPacket。
- 不自动集成 diff。
- 不 commit、push、merge 或 deploy。
- 不把 preview packet 当成正式 packet 复用。

## 3. 正式 WorkPacket 契约

每个 `workpackets/<dispatch_id>/<task_id>.json` 至少包含：

- `schema = kiana.swarm-workpacket.v1`
- `workpacket_id`
- `dispatch_id`
- `workflow_id`
- `run_id`
- `task_id`
- `goal`
- `context_summary`
- `allowed_files`
- `forbidden_files`
- `path_locks`，同时含逻辑路径与真实路径
- `verification_commands`
- `acceptance_criteria`
- `evidence_requirements`
- `rollback_plan`
- `review_focus`
- `budget`
- `termination_policy`
- `retry_policy`
- `approval_required`
- `created_at_ms`

预算默认值：

```json
{
  "max_attempts": 1,
  "max_commands": 20,
  "timeout_seconds": 1800,
  "max_output_bytes": 10485760
}
```

终止原因固定为：

- `completed`
- `budget_exhausted`
- `timeout`
- `scope_violation`
- `policy_denied`
- `approval_required`
- `cancelled`
- `worker_failed`

## 4. Dispatch Manifest 契约

路径：

```text
workpackets/<dispatch_id>/manifest.json
```

字段：

- `schema = kiana.swarm-dispatch-manifest.v1`
- `dispatch_id`
- `workflow_id`
- `run_id`
- `task_list_id`
- `plan_schema`
- `plan_fingerprint`
- `execution_mode = persist_only`
- `status = persisted`
- `workpacket_paths`
- `task_ids`
- `path_locks`
- `budget`
- `created_at_ms`
- `next_action = run_swarm_start`

## 5. 确定性身份与幂等性

`dispatch_id` 使用以下稳定 JSON 的 SHA-256 前 24 个十六进制字符：

- workflow_id/run_id
- task_list_id
- 有序 task IDs
- 每个 task 的正式 path locks
- verification commands
- budget

格式：

```text
dispatch-<24 hex>
```

同一输入重复执行：

- artifact 已存在且字节一致：复用。
- artifact 已存在但内容不同：返回 `ArtifactConflict`。
- 唯一 dispatch event 已存在：验证 artifacts 后返回原事件，不追加新 seq。
- artifacts 已写但事件缺失：补写事件，完成 crash recovery。

## 6. Workflow 原子提交原语

在 `kiana-tasks/src/workflow.rs` 新增通用提交函数：

```text
commit_immutable_artifacts_with_unique_event(...)
```

顺序：

1. 获取 `.eventlog.lock`。
2. 读取并校验 eventlog/state sequence 一致。
3. 检查唯一 event key。
4. 对所有 artifact 做路径穿越校验和内容冲突预检。
5. 使用 `create_new` 写临时文件、`sync_data`，再在同目录 rename 到目标路径。
6. 所有 artifact 成功后追加单个事件并 `sync_data`。
7. 更新 `state.json`。
8. 释放 lease。

该原语提供可恢复提交，不承诺跨文件系统的真正 ACID。崩溃最多留下内容完整但尚未被事件引用的 orphan artifact；相同 dispatch 重试会验证并补交事件。

## 7. 实施与验证任务

### Task 1：Workflow 提交原语

**测试文件：** `kiana-tasks/tests/workflow_runtime.rs`

实现后验证：

- 一次提交两个 artifact，只追加一个唯一事件。
- 重复相同提交返回原事件，event seq 不增加。
- 同路径不同内容返回 artifact conflict。
- `../`、绝对路径和 symlink 逃逸被拒绝。
- 预写 artifacts、缺事件时重试可补交事件。

实现：

- `WorkflowArtifactInput`
- `WorkflowArtifactCommit`
- `commit_immutable_artifacts_with_unique_event`
- `read_workflow_state`
- 必要的 `WorkflowError` 变体

### Task 2：正式 Dispatch 数据模型

**测试文件：** `kiana-tasks/tests/swarm_dispatch.rs`

实现后验证：

- 正式 packet 包含 workflow/task/budget/path-lock 身份。
- manifest 与 packets 使用确定性 dispatch_id。
- preview schema 不会被写入 artifact。
- blocked plan 不产生任何文件。
- 非法预算被拒绝。

实现：

- `SwarmWorkerBudget`
- `SwarmTerminationPolicy`
- `SwarmWorkPacket`
- `SwarmDispatchManifest`
- `SwarmDispatchResult`
- `persist_swarm_dispatch`

### Task 3：CLI 契约

**测试文件：** `kiana-commands/tests/swarm_command.rs`

实现后验证：

- 缺少或重复 `--workflow` 明确报错。
- 不一致 WorkflowRun 被拒绝且零 artifact。
- 成功 dispatch 写 packet/manifest/event。
- 重复命令幂等，event 数量不增长。
- budget 参数越界明确报错。
- JSON 输出不包含绝对本地路径。

实现：

- `tasks swarm dispatch` 路由。
- 参数解析与预算校验。
- `resume_workflow_run` gate。
- 调用 `persist_swarm_dispatch`。
- 中文文本输出和 JSON 输出。

### Task 4：文档与验证

更新：

- `USAGE.md`
- `docs/kiana_project_os/07-bounded-swarm-worker-model.md`

定向验证：

```bash
cargo test -p kiana-tasks --test workflow_runtime --locked --offline
cargo test -p kiana-tasks --test swarm_dispatch --locked --offline
cargo test -p kiana-commands --test swarm_command --locked --offline
```

全量验证：

```bash
cargo test --workspace --locked --offline --no-fail-fast
cargo fmt --all --check
git diff --check
```

真实 CLI 冒烟必须证明：

- packet 与 manifest 存在且 schema 正确。
- EventLog 只增加一个 `work_packet_created` 事件。
- 第二次相同 dispatch 不增加 event seq。
- Task 文件 hash 前后不变。
- 没有 worker 进程、worktree 或 git mutation。

## 8. 验收门禁

- 先实现批准的 dispatch persistence 合同，再运行 focused、adversarial、integration 和 regression verification。
- artifact 路径只能位于 WorkflowRun `workpackets/` 下。
- 正式 packet 不引用绝对项目路径。
- event 是 dispatch 的唯一事实提交点。
- 相同输入重复执行幂等。
- 内容冲突 fail closed。
- 预算和终止策略进入正式 packet。
- CLI 明确报告本切片仅 `persist_only`。
