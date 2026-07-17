# Bounded Swarm Worker Process Identity Implementation Plan

> **Execution rule:** This project does not use TDD. Implement each approved process-identity contract first, then run focused, adversarial, integration, and security verification. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 bounded swarm worker 增加可恢复的 Linux 进程身份绑定，阻止 PID 复用或状态篡改导致 `status/monitor/cancel` 误识别、误终止无关进程。

**Architecture:** 新增独立 `swarm_process_identity` 内部模块，从 Linux procfs 捕获并验证 PID、PGID、start-time 和 cmdline digest。worker state 升级到 v2；所有危险操作从“裸 PID”改为“已验证 identity”，旧 state 默认 fail closed。

**Tech Stack:** Rust、serde/serde_json、sha2、Linux procfs、现有 bounded swarm CLI/EventLog/atomic JSON helpers。

---

### Task 1：冻结 process identity 类型和 parser

**Files:**
- Create: `kiana-commands/src/swarm_process_identity.rs`
- Modify: `kiana-commands/src/lib.rs`
- Test: `kiana-commands/src/swarm_process_identity.rs`

- [ ] **Step 1：实现最小类型和 parser**

```rust
pub(crate) struct WorkerProcessIdentity {
    pub schema: String,
    pub platform: String,
    pub pid: u32,
    pub process_group_id: u32,
    pub start_time_ticks: u64,
    pub command_sha256: String,
}

pub(crate) enum ProcessIdentityStatus {
    Verified,
    Exited,
    Missing,
    Mismatch(Vec<String>),
    Unavailable(String),
}
```

实现：

- `capture_worker_process_identity(pid)`；
- `check_worker_process_identity(expected)`；
- `identity_digest(identity)`；
- Linux stat/cmdline 读取；
- 非 Linux 返回 unavailable，不回退为裸 PID。

- [ ] **Step 2：增加 parser focused verification**

测试 `parse_linux_proc_stat`：普通 comm、包含空格或右括号的 comm、缺字段、非法 PID、零 PGID、零 start-time，以及 mismatch reason 的稳定顺序。

- [ ] **Step 3：运行 focused verification**

```bash
cargo test -p kiana-commands swarm_process_identity --locked --offline --no-fail-fast
```

### Task 2：启动时绑定身份并升级 state

**Files:**
- Modify: `kiana-commands/src/tasks.rs`
- Test: `kiana-commands/tests/swarm_command.rs`

- [ ] **Step 1：实现启动绑定**

- spawn 后捕获 identity；
- 捕获失败时终止刚创建的 child 并返回错误；
- 写入 v2 state 和 `process_identity_status=verified`；
- `worker_started` event 只写 identity digest；
- 对已有非终态 state 执行 identity 检查后才复用。

- [ ] **Step 2：扩展启动身份 focused verification**

扩展 `swarm_start_launches_isolated_workers_and_status_recovers_them`，要求：

- state schema 为 `kiana.swarm-worker-state.v2`；
- identity schema 为 `kiana.swarm-process-identity.v1`；
- identity PID 与顶层 PID 一致；
- PGID、start-time 为正数；
- command digest 使用 `sha256:` 前缀；
- state 不包含 runner、project 或 procfs 绝对路径。

- [ ] **Step 3：运行 focused verification**

```bash
cargo test -p kiana-commands --test swarm_command swarm_start_launches_isolated_workers_and_status_recovers_them --locked --offline -- --nocapture
```

### Task 3：status 和 monitor fail closed

**Files:**
- Modify: `kiana-commands/src/tasks.rs`
- Modify: `kiana-commands/tests/swarm_command.rs`

- [ ] **Step 1：实现统一状态投影**

新增 helper，将 persisted terminal、exit artifact 和 process identity 组合为稳定状态。删除 `status/monitor` 对裸 `worker_process_alive(pid)` 的依赖。

- [ ] **Step 2：增加 status 和 monitor mismatch focused verification**

新增 `swarm_status_marks_reused_pid_as_lost`：启动 fixture worker，篡改 state 中 `start_time_ticks`，要求 status 返回 `lost/mismatch`，同时证明 fixture worker 仍存活。

新增 `swarm_monitor_records_identity_mismatch_without_signaling`，要求 monitor 不发送信号、state 进入 `lost`、`termination_reason=process_identity_mismatch`，并且不生成成功 ResultPacket。

- [ ] **Step 3：运行 focused verification**

```bash
cargo test -p kiana-commands --test swarm_command process_identity --locked --offline -- --nocapture
```

### Task 4：cancel 两阶段身份预检

**Files:**
- Modify: `kiana-commands/src/tasks.rs`
- Modify: `kiana-commands/tests/swarm_command.rs`

- [ ] **Step 1：实现预检和安全终止**

- 先读取并验证全部选中 worker；
- 任一非终态 worker 不为 verified 时整体返回错误；
- `terminate_worker_process` 接收 identity；
- TERM 前和 KILL 前分别验证；
- identity 变化时停止后续信号。

- [ ] **Step 2：增加 cancel identity focused verification**

新增 `swarm_cancel_refuses_foreign_process_identity_without_signaling`：

- 启动两个 fixture workers；
- 篡改目标 worker identity；
- cancel 返回 `process_identity_mismatch`；
- 目标和 peer 均仍存活；
- 没有 `worker_cancelled` event；
- state 未变为 `cancelled`；
- 恢复原 identity 后 cancel 成功，peer 不受影响。

- [ ] **Step 3：运行 focused verification 和回归**

```bash
cargo test -p kiana-commands --test swarm_command swarm_cancel_refuses_foreign_process_identity_without_signaling --locked --offline -- --nocapture
cargo test -p kiana-commands --test swarm_command swarm_cancel_stops_one_worker_without_stopping_its_peer --locked --offline -- --nocapture
```

### Task 5：旧 state、schema 和产品文档

**Files:**
- Create: `docs/schemas/kiana-swarm-worker-state.v2.schema.json`
- Modify: `scripts/schema-contract-smoke.sh`
- Modify: `scripts/release-preflight.sh`
- Modify: `docs/reference-migration-roadmap.md`
- Modify: `docs/reference-feature-matrix.md`
- Modify: `docs/commercial-release-readiness.md`
- Modify: `USAGE.md`
- Test: `kiana-commands/tests/swarm_command.rs`

- [ ] **Step 1：实现 JSON Schema**

schema 使用 `additionalProperties=false`，identity 字段完整且带数值下限，reason 使用受控字符串，不包含绝对宿主路径字段。

- [ ] **Step 2：更新中文产品文档**

记录安全不变量、Linux-only identity backend、旧 state fail-closed、未完成的 Windows/macOS 后端，以及后续 Local Structured Memory v1 优先级。

- [ ] **Step 3：增加旧 state focused verification**

将 running state 降为 v1 并删除 identity，要求 status 返回 `lost/missing`、cancel 返回 `process_identity_missing` 且不发送信号；terminal v1 state 仍可读取。

- [ ] **Step 4：运行 schema 与 focused gates**

```bash
bash scripts/schema-contract-smoke.sh
cargo test -p kiana-commands --test swarm_command --locked --offline --no-fail-fast
cargo test -p kiana-tasks --test swarm_worker_lifecycle --locked --offline --no-fail-fast
```

### Task 6：独立审查、全量门禁和 RC

**Files:**
- Update only if verification finds a defect.

- [ ] **Step 1：独立 spec review**

检查实现是否满足：不凭裸 PID 发信号、cancel 无部分副作用、旧 state fail closed、无绝对路径泄漏。

- [ ] **Step 2：独立 code-quality/security review**

重点检查 stat parser、TOCTOU 窗口、TERM/KILL 二次验证、错误原因稳定性和测试清理。

- [ ] **Step 3：运行全量门禁**

```bash
cargo fmt --all --check
cargo test --workspace --locked --offline --no-fail-fast
git diff --check
bash scripts/release-preflight.sh --local-rc
```

- [ ] **Step 4：刷新路线图与本地 RC 证据**

记录新的 RC 路径、archive/binary/recovery/local-evidence SHA-256 和最新 blocker counts。不得把外部凭据、签名、渠道或客户验收伪造成通过。

## 执行边界

- 当前工作树已有大量用户成果，只做增量修改，不回退、不重排无关代码。
- 本计划不执行 git commit、push、merge 或 cleanup 用户改动。
- 若测试证明当前假设错误，先更新设计和计划，再继续实现。
