# Bounded Swarm Worker Lifecycle Implementation Plan

> **Execution rule:** This project does not use TDD. Implement each approved worker-lifecycle contract first, then run focused, adversarial, integration, and recovery verification. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为已持久化的 Swarm WorkPacket 增加可恢复的本地 worker 启动、状态、监控、取消、预算终止和 ResultPacket 闭环。

**Architecture:** `kiana-tasks` 负责 schema、状态投影、不可变 artifact 与 Workflow event；`kiana-commands` 负责隔离目录、后台进程和跨平台终止。worker 运行在独立 worktree/snapshot 中，主工作树只读取 dispatch，不自动集成 diff。

**Tech Stack:** Rust 2021、serde/serde_json、sha2、tokio、std::process、现有 WorkflowRun/EventLog/immutable artifact 原语。

---

### Task 1: 生命周期 schema 与恢复投影

**Files:**
- Modify: `kiana-tasks/src/swarm.rs`
- Modify: `kiana-tasks/src/workflow.rs`
- Modify: `kiana-tasks/src/lib.rs`
- Create: `kiana-tasks/tests/swarm_worker_lifecycle.rs`

- [ ] 实现 execution/launch/state/result schema、dispatch loader、安全 ID 校验和 prepared commit。
- [ ] 增加 focused verification：从 dispatch artifacts 准备 execution manifest、launch artifacts 和唯一 `SwarmWorkersPrepared` event。
- [ ] 重跑测试，确认 prepared artifact 幂等且绝对路径不进入 JSON。

### Task 2: 隔离与后台启动

**Files:**
- Modify: `kiana-commands/src/tasks.rs`
- Modify: `kiana-commands/tests/swarm_command.rs`
- Modify: `kiana-commands/Cargo.toml` only if direct hashing dependency is required

- [ ] 实现 `swarm start`、runner executable 校验、prompt/runner script、PID handshake 和原子 state projection。
- [ ] 实现 `swarm status` 只读投影。
- [ ] 增加 focused/adversarial verification：两个 fixture workers 并行启动、重复 start 幂等、脏 Git 选择 `snapshot_copy`、干净 Git 选择 `git_worktree`。
- [ ] 运行定向测试并确认实现通过。

### Task 3: Monitor、Cancel 与预算终止

**Files:**
- Modify: `kiana-tasks/src/swarm.rs`
- Modify: `kiana-commands/src/tasks.rs`
- Modify: `kiana-tasks/tests/swarm_worker_lifecycle.rs`
- Modify: `kiana-commands/tests/swarm_command.rs`

- [ ] 实现 process liveness、process group terminate、graceful/force kill 和终态不可回退。
- [ ] 增加 focused/adversarial verification：exit 0、exit 非 0、PID lost、timeout、max_output_bytes，以及单 task cancel 隔离。
- [ ] 运行测试，确认重复 monitor/cancel 幂等。

### Task 4: Scope diff 与 ResultPacket

**Files:**
- Modify: `kiana-tasks/src/swarm.rs`
- Modify: `kiana-tasks/src/workflow.rs`
- Modify: `kiana-tasks/tests/swarm_worker_lifecycle.rs`
- Modify: `kiana-commands/tests/swarm_command.rs`

- [ ] 实现 baseline/diff projection、telemetry 上限校验和 immutable `result.json` commit。
- [ ] 增加 focused verification：allowed path 变更、scope violation，以及重复 monitor 的单一 ResultPacket event。
- [ ] 运行测试，确认 ResultPacket 不伪造 acceptance pass 或 commands。

### Task 5: 中文文档与真实 CLI 冒烟

**Files:**
- Modify: `USAGE.md`
- Modify: `docs/kiana_project_os/07-bounded-swarm-worker-model.md`

- [ ] 更新 start/status/monitor/cancel 命令、schema、恢复语义和非目标。
- [ ] 运行 `cargo test -p kiana-tasks --test swarm_worker_lifecycle --locked --offline`。
- [ ] 运行 `cargo test -p kiana-commands --test swarm_command --locked --offline`。
- [ ] 运行 `cargo test --workspace --locked --offline --no-fail-fast`。
- [ ] 运行 `cargo fmt --all --check`、`git diff --check`、`cargo build --locked --offline --bin kiana`。
- [ ] 使用临时 Git 项目和 fixture runner 冒烟，证明并行 PID、状态恢复、单 worker cancel、ResultPacket 幂等、主工作树 hash 不变和无 worktree 泄漏。
