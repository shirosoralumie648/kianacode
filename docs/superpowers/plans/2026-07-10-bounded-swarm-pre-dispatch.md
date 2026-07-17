# Bounded Swarm Pre-Dispatch Implementation Plan

> **Execution rule:** This project does not use TDD. Implement the planner and CLI contracts first, then add and run focused, adversarial, integration, and workspace verification.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 增加一个可验证、无副作用的 Bounded Swarm 预派发入口，从 Project Board 的 Ready task 生成并行 wave、path lock table 和 WorkPacket，并明确跳过冲突或边界不完整的任务。

**Architecture:** 在 `kiana-tasks` 新增纯函数 planner，输入 `ProjectBoardProjection` 和 `max_workers`，输出稳定的 `kiana.swarm-plan.v1`。`kiana-commands` 只负责解析 `kiana tasks swarm plan` 参数、构建 Board 和格式化输出；本切片不启动 worker、不创建 worktree、不修改 task 状态、不 merge。

**Tech Stack:** Rust 2021、serde/serde_json、现有 `kiana-tasks` Project Board/WorkPacket、现有 `kiana-commands` Command 路由。

---

### Task 1: Pure Bounded Swarm Planner

**Files:**
- Create: `kiana-tasks/src/swarm.rs`
- Modify: `kiana-tasks/src/lib.rs`

- [ ] **Step 1: Add public contracts**

Implement and export:

```rust
pub const SWARM_PLAN_SCHEMA: &str = "kiana.swarm-plan.v1";
pub const WORK_PACKET_PREVIEW_SCHEMA: &str = "kiana.swarm-workpacket-preview.v1";

pub struct SwarmPlan { /* task list, max workers, status, assignments, skipped, locks, summary */ }
pub struct SwarmAssignment { pub task_id: String, pub worker_type: String, pub workpacket: WorkPacket }
pub struct SwarmSkippedTask { pub task_id: String, pub reason: String, pub conflicts_with: Vec<String> }
pub struct SwarmPathLock { pub task_id: String, pub path: String, pub resolved_path: String, pub mode: String }
pub struct SwarmPlanSummary { pub ready_tasks: usize, pub dispatched: usize, pub skipped: usize }
```

- [ ] **Step 2: Implement conservative path normalization**

Rules:

- reject absolute paths and any `..` component;
- normalize `\\` to `/`;
- remove leading `./` and trailing `/`;
- reduce `src/**` and `src/*` to conservative root `src`;
- parent/child roots conflict;
- `Cargo.lock`, `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock` and `uv.lock` use `exclusive` mode.

- [ ] **Step 3: Implement selection algorithm**

Algorithm:

```text
ready column -> stable sort -> validate allowed_paths -> detect selected lock overlap
-> select until max_workers -> skip remainder with typed reason -> create WorkPacket inline
```

When fewer than two dispatchable tasks remain, return `status = "blocked"` and keep assignments empty, because one task is not a swarm.

- [ ] **Step 4: Keep planner construction side-effect free**

The pure planner may not mutate tasks, create worktrees, start workers, or write WorkPacket files.

### Task 2: Swarm Planner Verification

**Files:**
- Create: `kiana-tasks/tests/swarm_planner.rs`
- Create: `kiana-tasks/src/swarm.rs`
- Modify: `kiana-tasks/src/lib.rs`

- [ ] **Step 1: Add independent Ready task verification**

构造含两个 Ready task 的 `ProjectBoardProjection`。每个 task 都有不同 `allowed_paths` 和至少一个 `verification_commands`。断言 `build_swarm_plan(&board, 2)` 返回：

```rust
assert_eq!(plan.schema, "kiana.swarm-plan.v1");
assert_eq!(plan.status, "ready");
assert_eq!(plan.assignments.len(), 2);
assert_eq!(plan.path_locks.len(), 2);
assert!(plan.assignments.iter().all(|item| item.workpacket.schema == "kiana.swarm-workpacket-preview.v1"));
```

- [ ] **Step 2: Add conflict and incomplete scope verification**

覆盖：

```rust
// src/** conflicts with src/api/client.rs
assert_eq!(skipped.reason, "path_conflict");

// empty allowed_paths is never dispatched
assert_eq!(skipped.reason, "missing_allowed_paths");

// ../outside and absolute paths are rejected
assert_eq!(skipped.reason, "invalid_allowed_path");
```

- [ ] **Step 3: Add deterministic priority and max_workers verification**

Priority 降序，相同 priority 按 `created_at`、`task_id` 稳定排序。第三个无冲突 task 超出 worker 上限时返回 `worker_limit`。

- [ ] **Step 4: Run planner verification**

Run:

```bash
cargo test -p kiana-tasks --test swarm_planner --locked --offline -- --nocapture
```

Expected: all planner tests pass.

### Task 3: CLI Route and Human Output

**Files:**
- Modify: `kiana-commands/src/tasks.rs`
- Modify: `USAGE.md`
- Modify: `docs/kiana_project_os/07-bounded-swarm-worker-model.md`

- [ ] **Step 1: Add `tasks swarm plan` route**

Route:

```text
kiana tasks swarm plan [--json] [--max-workers <2..32>] [task_list_id]
```

The command loads tasks through existing task-list logic, builds the Project Board through `kiana-tasks`, calls the pure planner, and emits JSON or concise Chinese text.

- [ ] **Step 2: Keep the first slice side-effect free**

The command must not:

- mutate task status;
- append WorkflowRun events;
- create worktrees;
- start Agent/Bash workers;
- write WorkPacket files;
- merge or commit.

The JSON response must explicitly include `execution_mode: "plan_only"` and `next_action: "run_swarm_dispatch"` when ready.

- [ ] **Step 3: Document command and current boundary**

Update Chinese docs with command examples, typed skip reasons, path lock semantics, and the explicit boundary that dispatch/integrate remain later slices.

### Task 4: CLI Verification

**Files:**
- Create: `kiana-commands/tests/swarm_command.rs`
- Modify: `kiana-commands/src/tasks.rs`

- [ ] **Step 1: Add the CLI JSON contract verification**

创建临时 `.kiana/tasks/default/*.json` fixture，两个 task 均为 pending、无依赖、有 verification commands 和互不重叠的 allowed paths。执行：

```text
kiana tasks swarm plan --json --max-workers 2
```

断言 schema、task list、assignments、path locks 和 WorkPacket 字段。

- [ ] **Step 2: Add CLI validation verification**

覆盖：

```text
--max-workers 0         -> error
--max-workers 33        -> error
duplicate --max-workers -> error
unknown option          -> usage error
```

- [ ] **Step 3: Run command and crate verification**

Run:

```bash
cargo test -p kiana-commands --test swarm_command --locked --offline -- --nocapture
cargo test -p kiana-tasks --locked --offline --no-fail-fast
cargo test -p kiana-commands --locked --offline --no-fail-fast
```

Expected: all pass.

### Task 5: Workspace and Real CLI Verification

**Files:**
- Verify only.

- [ ] **Step 1: Run formatting and workspace tests**

```bash
cargo fmt --all --check
cargo test --workspace --locked --offline --no-fail-fast
git diff --check
```

- [ ] **Step 2: Run real CLI smoke**

Create a temporary task list with three Ready-capable tasks: two independent paths and one overlapping path. Run:

```bash
target/debug/kiana tasks swarm plan --json --max-workers 3
```

Expected:

- schema `kiana.swarm-plan.v1`;
- two assignments;
- conflicting task skipped with `path_conflict`;
- inline WorkPackets contain verification commands and allowed files;
- no worker process, worktree, task mutation, or commit is created.
