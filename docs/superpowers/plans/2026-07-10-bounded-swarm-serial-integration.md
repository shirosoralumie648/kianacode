# Bounded Swarm 串行集成实施计划

## 目标

实现 `integrate plan/apply/status` 与 `cleanup`，把已有 worker ResultPacket 转换为可预检、可回滚、可验证的主工作树集成闭环。

## Task 1：Integration 数据模型和只读 plan

文件：

- `kiana-tasks/src/swarm.rs`
- `kiana-tasks/src/lib.rs`
- `kiana-tasks/src/workflow.rs`
- `kiana-tasks/tests/swarm_integration.rs`

步骤：

1. 定义 IntegrationPlan、worker entry、baseline、conflict、state、packet schema。
2. 实现 artifact 读取与 fail-closed 校验。
3. 实现稳定 integration_id 和幂等持久化。
4. 运行定向测试，覆盖 completed/non-overlap、非 completed、scope deviation、身份错误和 path_conflict。

## Task 2：CLI plan/status

文件：

- `kiana-commands/src/tasks.rs`
- `kiana-commands/tests/swarm_command.rs`
- `USAGE.md`

步骤：

1. 实现参数解析与 JSON/human 输出。
2. 捕获主工作树 baseline fingerprint。
3. 生成 worker patch artifact 并执行 apply-check。
4. 实现只读 status。
5. 验证 plan 不改变项目源文件。

## Task 3：受控 apply 与逐 worker 验证

文件：

- `kiana-commands/src/tasks.rs`
- `kiana-commands/tests/swarm_command.rs`

步骤：

1. 重新验证 plan、patch 与 packet fingerprint。
2. 保存集成涉及路径 checkpoint。
3. 逐 worker `git apply --check`、`git apply`。
4. 逐 worker 运行 WorkPacket verification commands。
5. 持久化 state、events 与 IntegrationPacket。

## Task 4：失败回滚与 cleanup

文件：

- `kiana-commands/src/tasks.rs`
- `kiana-commands/tests/swarm_command.rs`

步骤：

1. 实现路径级 checkpoint 恢复，不使用 destructive Git reset。
2. 比对恢复后 baseline fingerprint。
3. 实现 cleanup 事实包前置条件与验证。
4. 实现 snapshot/Git worktree 回收与幂等输出。

## Task 5：证据、文档和全量验证

步骤：

1. 更新 Project OS Volume 27、USAGE 和 feature matrix。
2. 运行 `cargo test -p kiana-tasks --test swarm_integration --locked --offline`。
3. 运行 `cargo test -p kiana-commands --test swarm_command --locked --offline`。
4. 运行 `cargo test --workspace --locked --offline --no-fail-fast`。
5. 运行 `cargo fmt --all --check`、`git diff --check`、release build。
6. 运行真实 CLI 冒烟，证明主树 baseline、事件唯一性、无自动 Git 集成副作用。
