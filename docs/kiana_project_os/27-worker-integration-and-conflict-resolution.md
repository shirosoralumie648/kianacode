# Volume 27: Worker 集成与冲突解决

## 1. 目标

worker 并行只解决执行吞吐，不解决集成责任。Kiana 必须把集成收回主 agent 或 orchestrator。

## 2. Integration 输入

- WorkPackets。
- ResultPackets。
- path lock table。
- git diff。
- command outputs。
- worker notes。
- verification profile。

## 3. Postflight

每个 worker 完成后：

1. 检查 ResultPacket。
2. 检查 changed files。
3. 检查 forbidden files。
4. 检查 commands。
5. 检查 output schema。
6. 标记 ready_for_integration 或 rejected。

## 4. Touch-set Audit

比较：

- allowed files。
- actual changed files。
- generated files。
- deleted files。

结果：

- clean。
- extra_files。
- forbidden_touched。
- deletion_detected。
- unknown。

## 5. Conflict 分类

| 类型 | 说明 |
| --- | --- |
| path_conflict | 同一文件 |
| semantic_conflict | 行为冲突 |
| schema_conflict | 契约变化 |
| test_conflict | 测试互相影响 |
| dependency_conflict | 依赖版本冲突 |
| policy_conflict | 权限冲突 |
| artifact_conflict | 生成物冲突 |

## 6. Merge Policy

自动集成允许：

- non-overlap。
- tests independent。
- no high-risk files。
- no policy changes。

必须人工/主 agent 审查：

- schema。
- lock files。
- release scripts。
- policy。
- security。
- EDA order files。

## 7. Integration Steps

1. Load packets。
2. Validate packets。
3. Audit touch set。
4. Check conflicts。
5. Apply clean diffs。
6. Run verification。
7. Synthesize review。
8. Update board。
9. Write evidence。

## 8. Rejected Result

Reject 原因：

- scope violation。
- missing ResultPacket。
- command missing。
- forbidden file touched。
- evidence missing。
- output invalid。

Rejected 不等于失败，可以重新派发。

## 9. Fix Wave

Review 后发现问题：

- 创建 fix tasks。
- 可并行则 dispatch。
- 不可并行则 serial fix。

Fix wave 必须继承原 finding。

## 10. Worker Trust

worker 输出不可信直到：

- packet validates。
- touch set clean。
- verification pass。
- review pass。

## 11. Integration Evidence

记录：

- integrated packets。
- rejected packets。
- conflicts。
- commands run。
- final status。

## 12. 验收

P1 验收：

- forbidden file touch 被拒。
- non-overlap 可以集成。
- conflict 会 block。
- integration 后统一 verification。
- worker 自报成功不直接 Done。

## 13. 已实现命令面

```text
kiana tasks swarm integrate plan --workflow <run_id> --dispatch <dispatch_id> [--json]
kiana tasks swarm integrate apply --workflow <run_id> --dispatch <dispatch_id> [--json]
kiana tasks swarm integrate status --workflow <run_id> --dispatch <dispatch_id> [--json]
kiana tasks swarm cleanup --workflow <run_id> --dispatch <dispatch_id> [--json]
```

命令职责严格拆分：

- `plan`：只读预检项目源文件，仅在 Workflow artifact directory 写计划与 patch。
- `apply`：显式串行应用，逐 worker 运行 verification commands。
- `status`：只读投影最新 plan/state/packet。
- `cleanup`：只回收已有终态事实包的隔离目录。

## 14. Integration 状态机

```text
not_planned -> planned -> applying -> verifying -> integrated
                    |          |          |
                    v          v          v
                 blocked   rolling_back -> rolled_back
                    |
                    v
                  stale

rolling_back -> rollback_failed
```

状态定义：

- `ready/planned`：所有 worker 通过 packet、scope、hash、path conflict 与 apply-check。
- `blocked`：至少一个 worker 不可安全集成。
- `stale`：plan 之后主工作树或 artifact 发生变化。
- `applying`：已创建 checkpoint，正在按固定顺序应用。
- `integrated`：所有 patch 与 verification commands 通过。
- `rolled_back`：失败后已恢复到 plan baseline。
- `rollback_failed`：无法证明恢复完成，必须人工处理。

## 15. 持久化 Artifacts

每个 dispatch 使用：

```text
integrations/<dispatch_id>/
  plan.json
  state.json
  packet.json
  verification.json
  cleanup.json
  patches/<task_id>.patch
  checkpoint/manifest.json
  checkpoint/files/**
```

核心 schema：

- `kiana.swarm-integration-plan.v1`
- `kiana.swarm-integration-state.v1`
- `kiana.swarm-integration-packet.v1`
- `kiana.swarm-integration-verification.v1`
- `kiana.swarm-integration-checkpoint.v1`
- `kiana.swarm-cleanup-result.v1`

## 16. Baseline 与 Artifact 完整性

plan 记录当前 Git HEAD 或 `none`，并对排除 `.git`、`.kiana` 和构建缓存后的项目文件 manifest 计算 SHA-256 fingerprint。apply 前必须重新计算并完全匹配。

每个 worker entry 记录：

- ResultPacket 相对路径和 SHA-256。
- binary patch 相对路径和 SHA-256。
- changed files。
- verification commands。
- worker blockers。

任何 artifact hash 改变都会拒绝 apply。

## 17. 串行应用协议

1. 读取 ready IntegrationPlan。
2. 复核主树 baseline。
3. 复核每个 ResultPacket 和 patch hash。
4. 对全部 changed paths 创建路径级 checkpoint。
5. 按 dispatch task 顺序执行 `git apply --check --binary`。
6. 执行 `git apply --binary`。
7. 在项目根目录运行该 worker 的 verification commands。
8. 写 `SwarmWorkerIntegrated` event。
9. 全部通过后写 verification、state 与 IntegrationPacket。

协议永远不会自动 commit、push、merge 或 deploy。

## 18. 回滚协议

回滚不得使用 `git reset --hard` 或 `git checkout --`。checkpoint 保存本次集成涉及路径的存在性、文件内容或 symlink target。失败后：

1. 删除 apply 新增的路径。
2. 恢复 baseline 已存在的文件与 symlink。
3. 重新计算工作树 fingerprint。
4. fingerprint 相同才写 `rolled_back`。
5. fingerprint 不同写 `rollback_failed` 并保留全部诊断 artifact。

用户原有、与集成路径无关的 dirty changes 不会被清理。

## 19. Cleanup 安全边界

cleanup 必须同时满足：

- integration 为 `integrated`、`blocked`、`rolled_back` 或 `rollback_failed`。
- worker state 是 terminal。
- worker ResultPacket 存在。
- isolation path 与 `.kiana/swarm-worktrees/<dispatch>/<task>` 精确一致。
- canonical path 不逃逸项目根目录。

cleanup 保留所有事实包和 EventLog。重复调用不重复删除，也不重复写事件。

## 20. 当前剩余边界

本地 P0 已覆盖 non-overlap 串行应用、stale 检测、verification failure 全量回滚、snapshot/Git worktree 隔离和安全清理。仍不自动解决：

- semantic conflict。
- schema migration conflict。
- dependency/lockfile 合并。
- policy/security/release surface 的自动批准。
- 跨仓库事务。
- commit/PR/merge/deploy。

完整数据契约与测试矩阵见 `docs/superpowers/specs/2026-07-10-bounded-swarm-serial-integration-design.md`。
