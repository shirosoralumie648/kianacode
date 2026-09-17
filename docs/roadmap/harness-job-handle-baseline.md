# H17 后台进程与长工具 JobHandle 基线

> 快照日期：2026-09-18。本页记录长任务句柄、轮询/输入/终止接缝；本地不运行测试，夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H17`](harness.md#step-h17) |
| feature_status | `implemented`（现有 process handler 的 typed handle/owner/authority 接线；跨进程进程恢复仍 fail-closed） |
| proof_level | `source`；本地仅做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | Every `process.start/poll/stdin/resize/stop` remains a separate ControlPlane invocation/permit; JobHandle is evidence, not authority |
| this step does | `JobHandle` 绑定 start request/invocation、Run/Turn、owner/session、project digest、authority epoch、process-group evidence 和 TTL；start 先写 prepared，再写 started handle；continuation 重新校验 owner/scope/authority/expiry，poll 提供 cursor，重启无 live process 只能读取已落盘 outcome |
| this step does not | 不用 PID 单独认领进程，不重复 start 伪装 poll，不把后台句柄变成权限或取消逃生口；跨进程 PID/进程组恢复与完整 Artifact/stream durability 仍未实现 |

## 1. Contract

`process.start` 生成唯一 `job_id`，并将 `JobHandle` 写入 `process.started` 事实；句柄中的
`process_group_id` 只证明启动时的 OS 进程组，不授予任何新 capability。poll/stdin/resize/stop
都携带相同 handle scope，并分别经过 Core 的 invocation/permit，authority revision 变化、
owner/session/run/turn 不匹配或 TTL 过期均拒绝。

live entry 的 poll 返回 bounded stdout/stderr、running/exited/unknown outcome 和随输出长度
变化的 cursor；没有 live entry 时，重启路径只允许 poll 并返回已记录的 terminal outcome，
stdin/stop 不尝试按 PID attach。长任务由原 start 的单一 JobHandle 关联，后续操作不会复用已消费
的 start invocation。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `foreign_job_handle_cannot_receive_stdin_or_cancel` | owner/session/run/turn/authority 变化阻止 continuation |
| `pid_reuse_cannot_attach_to_other_process` | 无 live entry 只读 persisted outcome，缺 process-group evidence fail-closed |
| `poll_does_not_restart_job` | restart path 不 spawn/attach，仅返回 process.finished 或 Unknown |
| `long_command_has_one_job_and_distinct_operation_invocations` | start/poll/stdin/resize/stop 共享 JobHandle 但每次是独立 Core invocation |

## 3. Proof ceiling and handoff

H17 proof ceiling 为 `source`：长任务已具备 typed handle、owner/authority/TTL fencing、bounded
poll cursor 和重启拒绝 attach 语义。当前 process/output maps 与句柄记录仍是本地 adapter；跨进程
JobHandle projector、进程组持久恢复、完整输出 Artifact 分段、OS/power-loss 和 external/live/
physical proof 留待 H24/H25、PD/INT。
