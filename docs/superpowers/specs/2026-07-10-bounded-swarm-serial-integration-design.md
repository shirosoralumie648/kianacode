# Kiana Bounded Swarm 串行集成设计

## 1. 文档目的

本文定义 Bounded Swarm worker 完成后的串行集成闭环。它解决的不是“如何并行执行”，而是“如何把多个不可信 worker 的结果安全、可回滚、可审计地带回当前项目工作树”。

本设计是 `docs/kiana_project_os/27-worker-integration-and-conflict-resolution.md` 的可实现规格，复用现有 WorkflowRun、WorkPacket、ResultPacket、Evidence Ledger、VerificationPacket 与 checkpoint 能力。

## 2. 产品目标

用户运行多个 worker 后，可以显式执行：

```text
kiana tasks swarm integrate plan --workflow <run_id> --dispatch <dispatch_id> [--json]
kiana tasks swarm integrate apply --workflow <run_id> --dispatch <dispatch_id> [--json]
kiana tasks swarm integrate status --workflow <run_id> --dispatch <dispatch_id> [--json]
kiana tasks swarm cleanup --workflow <run_id> --dispatch <dispatch_id> [--json]
```

系统必须做到：

1. 不信任 worker 自报成功，只接受可验证的 ResultPacket。
2. 在修改主工作树前完成 packet、scope、路径冲突和 baseline 漂移预检。
3. 以确定顺序逐个应用 worker patch。
4. 每个 worker patch 应用后运行该 WorkPacket 的 verification commands。
5. 任一步失败时恢复到集成前的精确工作树状态，包括用户原有 dirty changes。
6. 持久化 IntegrationPlan、IntegrationState、IntegrationPacket 和 workflow events。
7. 不自动 commit、push、merge、deploy。
8. cleanup 仅回收已有终态事实包的隔离目录，不删除未取证结果。

## 3. 非目标

P0 不实现：

- 自动解决语义冲突。
- 自动修改 worker patch。
- 自动提交 Git commit。
- 自动创建或合并 PR。
- 自动发布或部署。
- 跨仓库原子事务。
- 未经显式命令自动集成。
- 将 exit code 0 等同于 acceptance criteria 完成。

## 4. 信任边界

### 4.1 不可信输入

- worker 进程退出码。
- worker stdout/stderr。
- worker notes。
- worker 自报 commands_run。
- worker 隔离目录中的任意文件。

### 4.2 可验证输入

- dispatch manifest。
- execution manifest。
- WorkPacket。
- monitor 生成并持久化的 ResultPacket。
- worker 启动前 baseline manifest。
- worker 隔离目录当前 manifest。
- Workflow eventlog。
- 主工作树当前 Git 与文件指纹。

### 4.3 fail-closed 原则

任何身份不一致、文件缺失、schema 不匹配、hash 不一致、未知终态、scope deviation、路径冲突或主树漂移，均不得进入 apply。

## 5. 状态机

IntegrationState 使用以下状态：

```text
not_planned
  -> planned
  -> blocked

planned
  -> applying
  -> stale
  -> blocked

applying
  -> verifying
  -> rolling_back

verifying
  -> integrated
  -> rolling_back

rolling_back
  -> rolled_back
  -> rollback_failed
```

终态：

- `blocked`：预检不允许应用。
- `stale`：plan 生成后主工作树或 worker 结果发生变化。
- `integrated`：所有 patch 和验证通过。
- `rolled_back`：应用或验证失败，主工作树已恢复。
- `rollback_failed`：恢复无法证明完成，必须人工介入。

## 6. 命令语义

### 6.1 integrate plan

完全只读于项目源文件，只允许在 Workflow artifact directory 写入计划和事件。

步骤：

1. 恢复 WorkflowRun 并验证 ready。
2. 读取 dispatch manifest 与 execution manifest。
3. 读取每个 WorkPacket、launch、worker state、ResultPacket。
4. 只接受 `ResultPacket.status == completed`。
5. 拒绝 `scope_deviations` 非空的 packet。
6. 验证 result packet 的 workflow_id、run_id、dispatch_id、task_id、worker_id。
7. 从 worker baseline manifest 和隔离目录生成 changed file set。
8. 验证 changed file set 与 ResultPacket、path locks、allowed files 一致。
9. 生成每个 worker 的 binary patch artifact。
10. 检测 worker 间 path overlap。
11. 对每个 patch 执行 `git apply --check --binary`，但不修改当前工作树。
12. 捕获当前主工作树 baseline fingerprint。
13. 持久化 IntegrationPlan。

计划结果：

- `ready`：全部可串行应用。
- `blocked`：至少存在一个阻断项。

### 6.2 integrate apply

必须已有 `ready` IntegrationPlan。

步骤：

1. 重新计算主工作树 baseline fingerprint。
2. 重新计算所有 patch SHA-256 与 ResultPacket SHA-256。
3. 任一变化则标记 `stale`，不修改源文件。
4. 创建集成前 checkpoint，保存 tracked、staged、unstaged、untracked 文件状态。
5. 按 IntegrationPlan 固定顺序执行 `git apply --check --binary`。
6. 执行 `git apply --binary`。
7. 运行该 worker WorkPacket 的 verification commands。
8. 记录命令、exit code、stdout/stderr 有界尾部和 Evidence events。
9. 所有 worker 完成后运行统一 verification profile。
10. 生成 VerificationPacket 与 IntegrationPacket。
11. 任一步失败则进入 rollback。

apply 不允许：

- 计划外 patch。
- 修改 `.git/**`、`.kiana/policy.json` 或 forbidden files。
- 自动 commit、push、merge、deploy。
- 在验证失败后保留半集成状态。

### 6.3 integrate status

只读返回：

- 当前 IntegrationState。
- plan/packet 路径。
- ready、blocked、applied、verified worker 数量。
- 阻断原因。
- rollback 状态。
- next_action。

重复调用不得新增 workflow event。

### 6.4 cleanup

cleanup 删除 worker 隔离目录和可安全移除的 Git worktree registration，但保留：

- dispatch manifest。
- execution manifest。
- WorkPackets。
- ResultPackets。
- IntegrationPlan。
- IntegrationPacket。
- VerificationPacket。
- eventlog。
- worker stdout/stderr 摘要与 hash。

允许 cleanup 的 worker 必须满足：

1. worker state 为终态。
2. ResultPacket 已持久化。
3. dispatch 已 `integrated`、`blocked`、`rolled_back`，或显式无可集成结果。
4. 隔离路径严格位于 `.kiana/swarm-worktrees/<dispatch>/<task>`。

若缺少事实包，cleanup 必须拒绝。

## 7. 数据契约

### 7.1 IntegrationPlan

```json
{
  "schema": "kiana.swarm-integration-plan.v1",
  "integration_id": "integration-...",
  "workflow_id": "...",
  "run_id": "...",
  "dispatch_id": "dispatch-...",
  "status": "ready",
  "baseline": {
    "git_head": "...",
    "working_tree_fingerprint": "sha256:...",
    "captured_at_ms": 0
  },
  "workers": [
    {
      "task_id": "api",
      "worker_id": "worker-...-api",
      "result_packet_path": "results/.../api.json",
      "result_packet_sha256": "sha256:...",
      "patch_path": "integrations/.../patches/api.patch",
      "patch_sha256": "sha256:...",
      "changed_files": ["src/api/mod.rs"],
      "verification_commands": ["cargo test -p api"],
      "status": "ready",
      "blockers": []
    }
  ],
  "conflicts": [],
  "blockers": [],
  "created_at_ms": 0,
  "next_action": "run_swarm_integrate_apply"
}
```

### 7.2 IntegrationState

```json
{
  "schema": "kiana.swarm-integration-state.v1",
  "integration_id": "integration-...",
  "dispatch_id": "dispatch-...",
  "status": "applying",
  "plan_sha256": "sha256:...",
  "checkpoint_path": "integrations/.../checkpoint",
  "applied_task_ids": ["api"],
  "verified_task_ids": ["api"],
  "current_task_id": "docs",
  "failure": null,
  "updated_at_ms": 0,
  "next_action": "continue_integration"
}
```

### 7.3 IntegrationPacket

```json
{
  "schema": "kiana.swarm-integration-packet.v1",
  "integration_id": "integration-...",
  "workflow_id": "...",
  "run_id": "...",
  "dispatch_id": "dispatch-...",
  "status": "integrated",
  "integrated_task_ids": ["api", "docs"],
  "rejected_task_ids": [],
  "commands": [],
  "verification_packet_path": "verification/...json",
  "rollback": null,
  "automatic_commit": false,
  "automatic_push": false,
  "automatic_merge": false,
  "automatic_deploy": false,
  "created_at_ms": 0
}
```

## 8. Baseline 与一致性

### 8.1 主工作树 baseline

fingerprint 必须覆盖：

- Git HEAD，非 Git 项目使用 `none`。
- tracked staged diff。
- tracked unstaged diff。
- untracked 文件相对路径与内容 hash。
- 删除文件状态。

忽略：

- `.git/**`。
- 当前 Workflow artifact directory 的新 integration 状态文件。
- worker isolation roots。
- 构建缓存目录。

该规则允许用户在 dirty tree 上集成，同时能发现 plan 后新增的外部修改。

### 8.2 worker patch

- `git_worktree`：相对 worker 启动 HEAD 生成 binary patch，并纳入 untracked files。
- `snapshot_copy`：相对 worker baseline manifest 生成 no-index patch。
- patch 必须能在当前 baseline 上通过 `git apply --check --binary`。
- empty patch 不是集成成功；标记 `blocked: empty_result`。

### 8.3 冲突

P0 阻断以下冲突：

- 两个 worker 修改同一路径。
- 父子路径锁重叠。
- patch 不能 apply。
- lock file、policy、release、security surface 变化。
- ResultPacket changed_files 与实际 patch 不一致。

P0 不做三方自动合并。

## 9. 回滚

回滚不能使用 `git reset --hard` 或 `git checkout --`。

推荐实现：

1. apply 前把集成计划涉及的每个路径保存到 checkpoint：存在性、类型、权限、内容 bytes、symlink target。
2. 同时保存主工作树 staged/unstaged 状态摘要。
3. 失败时按 checkpoint 恢复涉及路径。
4. 删除集成新增但 baseline 不存在的路径。
5. 恢复后重新计算 baseline fingerprint。
6. 只有 fingerprint 与 plan baseline 相同才能标记 `rolled_back`。
7. 不同则标记 `rollback_failed` 并保留全部诊断 artifact。

## 10. 幂等性

- 相同输入重复 `integrate plan` 返回原 plan，不重复写事件。
- `integrated` 后重复 apply 返回原 IntegrationPacket。
- `blocked` 或 `stale` 不自动重算；用户必须重新运行 plan。
- `rolling_back` 恢复时可根据 state 继续回滚。
- cleanup 重复运行返回 removed=0，不报错。

## 11. Workflow events

新增事件：

- `SwarmIntegrationPlanned`
- `SwarmIntegrationStarted`
- `SwarmWorkerIntegrated`
- `SwarmIntegrationBlocked`
- `SwarmIntegrationRolledBack`
- `SwarmIntegrationCompleted`
- `SwarmIsolationCleaned`

事件数据必须包含 dispatch_id 与 integration_id，唯一性由二者共同约束。

## 12. 错误与 next_action

| 状态 | next_action |
| --- | --- |
| ready | run_swarm_integrate_apply |
| blocked | inspect_integration_blockers |
| stale | rerun_swarm_integrate_plan |
| applying | resume_or_inspect_integration |
| rolled_back | fix_and_redispatch |
| rollback_failed | manual_recovery_required |
| integrated | run_swarm_cleanup_or_review |

## 13. P0 测试矩阵

必须覆盖：

1. 两个 non-overlap completed worker 生成 ready plan。
2. 非 completed worker 被拒绝。
3. scope deviation 被拒绝。
4. ResultPacket 身份不一致被拒绝。
5. 同一路径修改产生 path_conflict。
6. dirty 主树可生成 baseline 并保持不变。
7. plan 后主树改变，apply 返回 stale 且不修改其他文件。
8. 两个 patch 串行应用成功。
9. 第一 worker 验证失败时恢复所有路径。
10. 第二 worker 验证失败时撤销第一和第二 worker 修改。
11. 重复 plan/apply/status 幂等。
12. cleanup 拒绝缺少 ResultPacket 的 worker。
13. cleanup 删除 snapshot 与 Git worktree，但保留事实包。
14. 无自动 commit/push/merge/deploy。

## 14. P0 验收标准

- 用户原有 dirty diff 在成功集成后仍存在且内容不变。
- 失败回滚后主工作树 fingerprint 与 plan baseline 完全一致。
- 所有集成决策都有 artifact 与 event 证据。
- worker 结果只有通过 packet、scope、conflict、apply-check 和 verification 后才进入 integrated。
- 任一阻断不会留下半应用源文件。
- 所有命令支持 JSON 输出并有 CLI 集成测试。
