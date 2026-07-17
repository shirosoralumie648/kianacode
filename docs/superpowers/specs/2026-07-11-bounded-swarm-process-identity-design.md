# Bounded Swarm Worker 进程身份绑定设计

## 1. 背景

Kiana 的 bounded swarm 已具备任务规划、dispatch、隔离启动、状态恢复、监控、取消、ResultPacket、串行集成、回滚和 cleanup。现有 worker 运行态仍只通过 PID 识别进程：

- `swarm start` 将 `child.id()` 写入 worker state；
- `swarm status` 和 `swarm monitor` 使用 `kill -0 <pid>` 判断进程是否存在；
- `swarm cancel` 直接向 `-<pid>` 进程组发送 TERM/KILL。

PID 是可复用的临时编号。Kiana 在崩溃、长时间暂停或 worker 已退出后恢复时，原 PID 可能已经属于无关进程。此时仅依赖 `kill -0` 会把无关进程误判为 worker，取消操作甚至可能向无关进程组发送信号。

这属于商业化安全缺口：Kiana 必须证明它只控制自己启动且身份仍连续的 worker。

## 2. 根因

根因不是信号命令本身，而是 worker state 缺少可跨进程恢复验证的身份材料。

当前数据流为：

1. 启动阶段只持久化 PID；
2. 恢复阶段只读取 PID；
3. 存活判断只验证“系统中存在这个 PID”；
4. 取消阶段默认“这个 PID 仍属于原 worker”。

缺失的是对以下事实的绑定：

- 进程创建时间是否与启动时一致；
- 进程组是否仍是原 worker 的独立进程组；
- 当前命令行是否仍与启动时的 sandbox/runner 命令一致。

## 3. 目标

第一版实现 Linux worker process identity binding：

1. worker 启动后、写入 running state 前捕获进程身份；
2. state 持久化 PID、PGID、Linux start-time ticks 和命令行摘要；
3. `start/status/monitor/cancel` 在依赖 PID 前重新验证身份；
4. 身份缺失或不匹配时 fail closed；
5. `cancel` 在任何身份不可信时不发送 TERM/KILL；
6. 输出稳定、可测试的原因码；
7. 保持 WorkPacket、ResultPacket 和 integration artifact 兼容。

## 4. 非目标

本切片不实现：

- `max_attempts` 自动重试；
- worker 自动重启；
- pidfd 跨进程持久化；
- Windows Job Object 或 macOS `proc_pidinfo` 后端；
- worker state 全量类型化迁移；
- bounded swarm 的跨平台可用性声明；
- 调度、scope、budget、integration 语义重构。

非 Linux 平台在没有等价身份后端前不得宣称具备安全恢复和安全取消能力。

## 5. 方案比较

### 5.1 PID + start-time

优点是字段少、实现简单。缺点是不能证明目标仍位于原进程组，也不能检测命令身份变化，防御层次不足。

### 5.2 PID + PGID + start-time + cmdline SHA-256

全部材料可从 Linux `/proc` 离线读取，可持久化，可在崩溃恢复后重新验证，并且不暴露绝对命令行内容。缺点是依赖 Linux procfs。

本设计采用此方案。

### 5.3 pidfd

pidfd 适合同一 Kiana 进程内的可靠控制，但不能作为普通 JSON 字段跨进程恢复，无法单独解决持久化恢复问题，可作为后续增强。

## 6. 数据契约

新启动的 worker state 使用 `kiana.swarm-worker-state.v2`，新增 `process_identity`：

```json
{
  "schema": "kiana.swarm-worker-state.v2",
  "worker_id": "worker-...",
  "dispatch_id": "dispatch-...",
  "task_id": "api",
  "status": "running",
  "pid": 12345,
  "process_identity": {
    "schema": "kiana.swarm-process-identity.v1",
    "platform": "linux_procfs",
    "pid": 12345,
    "process_group_id": 12345,
    "start_time_ticks": 987654321,
    "command_sha256": "sha256:..."
  },
  "process_identity_status": "verified",
  "process_identity_reason": null
}
```

约束：

- identity PID 必须与 state 顶层 PID 一致；
- PGID 和 start-time 必须大于零；
- `command_sha256` 只保存摘要，不保存 cmdline 原文；
- 不把宿主绝对路径写入 state；
- v1 state 可读，但缺少身份时视为 `missing`，不能执行危险控制操作。

## 7. Linux 身份读取

新增内部模块 `kiana-commands/src/swarm_process_identity.rs`。

### 7.1 `/proc/<pid>/stat`

第二字段 `comm` 可能包含空格和括号，不能对整行简单 split。解析规则：

1. 找到第一处 `(`；
2. 找到最后一处 `)`；
3. 只对右括号后的字段切分；
4. 从剩余字段读取 PGID 和 start-time；
5. PID、PGID、start-time 必须可解析并满足正数约束。

### 7.2 `/proc/<pid>/cmdline`

读取原始 NUL 分隔字节并计算 SHA-256。bounded swarm worker 捕获到空 cmdline 时启动失败。

### 7.3 身份检查状态

- `verified`：全部字段一致；
- `exited`：`/proc/<pid>` 已不存在；
- `missing`：旧 state 没有 process identity；
- `mismatch`：PID、PGID、start-time 或 command digest 不一致；
- `unavailable`：procfs 无法安全读取或当前平台没有后端。

## 8. 行为设计

### 8.1 `swarm start`

新 worker：

1. spawn worker；
2. 捕获 process identity；
3. 捕获失败时终止刚创建的 child，并拒绝写入 running state；
4. 捕获成功后写入 v2 state；
5. `worker_started` event 只写 identity digest。

已有非终态 state：

- identity verified：保持幂等复用；
- identity exited：保持“已退出、等待 monitor”语义；
- identity missing/mismatch/unavailable：拒绝静默复用并报告原因。

### 8.2 `swarm status`

`status` 保持只读：

- 终态 worker 使用持久化终态；
- 无 exit artifact且 identity verified：`running`；
- identity exited、missing、mismatch 或 unavailable：`lost`；
- 每个 worker 输出 `process_identity_status` 和 `process_identity_reason`。

### 8.3 `swarm monitor`

在 timeout、output budget 或其他需要终止 worker 的逻辑前先验证 identity：

- verified：允许继续存活判断或终止；
- exited：按 exit artifact/exit code 分类；没有 exit artifact 时写入 `lost`；
- missing/mismatch/unavailable：不得发送信号，持久化 `lost`，并写入稳定 termination reason。

ResultPacket 不得把 identity failure 伪装成 worker 成功或普通业务失败。

### 8.4 `swarm cancel`

取消采用两阶段处理：

1. 预检全部匹配 worker；
2. 只有全部需要控制的非终态 worker 均为 verified，才进入信号发送阶段。

如果任一 worker 为 missing/mismatch/unavailable：

- 整次 cancel 返回错误；
- 不向任何 worker 发送信号；
- 不写 `worker_cancelled` event；
- 不把 state 改成 `cancelled`；
- 错误包含稳定原因码和 worker/task ID。

### 8.5 终止过程

`terminate_worker_process` 接收已验证 identity，而不是裸 PID：

1. 发送 TERM 前验证；
2. 等待后再次验证；
3. 仍是同一 identity 时才发送 KILL；
4. identity 已退出则成功结束；
5. identity 发生变化则停止，不向新进程发送 KILL。

## 9. 兼容与恢复

- 新写 state 为 v2；
- v1 state 仍可读取；
- v1 非终态 state 因缺少身份不能 cancel；
- v1 终态 state 仍可 status、cleanup 和 integration；
- 不修改已生成的 WorkPacket、ResultPacket、VerificationPacket；
- 不自动升级历史 state，以保留审计事实。

## 10. 安全不变量

1. Kiana 不得仅凭 PID 发送信号。
2. state identity 与当前 procfs 不一致时不得发送信号。
3. TERM 后 PID 被复用时不得向复用后的进程发送 KILL。
4. 身份缺失不得自动降级为旧的不安全行为。
5. status 不得修改 state。
6. cancel 身份预检失败不得产生部分取消。
7. 绝对命令行和宿主路径不得写入持久化 artifact。

## 11. 测试与验收

必须覆盖：

- stat parser 正确处理带空格和右括号的 `comm`；
- start state 保存 v2 identity；
- 正常 worker status 仍为 running；
- 篡改 start-time 后 status 为 lost/mismatch；
- 篡改 identity 后 cancel 返回错误且进程仍存活；
- 恢复原 identity 后 cancel 正常，peer worker 不受影响；
- monitor 遇到 mismatch 不发送信号并持久化 lost；
- 旧 v1 state 缺失 identity 时 fail closed；
- TERM 与 KILL 之间 identity 变化时不发送 KILL；
- 全套 swarm、workspace、schema、release 和 package lifecycle 门禁通过。

## 12. 后续切片

本切片完成后，下一优先级为 Local Structured Memory v1。自动 retry、跨平台 identity backend 和 worker state 类型化分别作为独立设计，不与本安全修复合并。
