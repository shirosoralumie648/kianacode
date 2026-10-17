# H17 后台进程与长工具 JobHandle 基线

> 快照日期：2026-09-18；行为夹具段更新 2026-10-17（CI-only）。本页记录长任务句柄、轮询/输入/终止接缝；本地不运行测试，夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H17`](harness.md#step-h17) |
| feature_status | `implemented`（现有 process handler 的 typed handle/owner/authority 接线；跨进程进程恢复仍 fail-closed） |
| proof_level | `source`；本地仅做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| focused CI | 手动 workflow [`h17-job-handle-diagnostic.yml`](../../.github/workflows/h17-job-handle-diagnostic.yml) 复核 domain handle fixtures、daemon 源码守卫与 daemon lib `h17_behavior_*` 行为夹具，并各自编译 `--no-run`；不把未等待的远程结果提升为更高 proof |
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
| `h17_behavior_foreign_job_handle_cannot_receive_stdin_or_cancel` | owner/session 变化与句柄 owner/Turn 绑定不符都阻止 continuation；原 owner 重启后仍只能 poll 已记录状态 |
| `h17_behavior_pid_reuse_cannot_attach_to_other_process` | 无 live entry 时 stdin/resize/stop 只返回 `result_unknown:process_handle_unavailable_after_restart`；换绑句柄、缺进程组证据、被篡改或过期句柄分别以稳定码拒绝 |
| `h17_behavior_poll_does_not_restart_job` | restart path 不 spawn/attach，只返回 `process.finished` 或保守 Unknown，且不新增 `process.*` 事实 |
| `h17_behavior_long_command_has_one_job_and_distinct_operation_invocations` | live poll 返回 running/输出/cursor（无新输出时 cursor 稳定），start/poll/stdin/resize/stop 共享一个 JobHandle 但每次是独立 invocation，且不新增 process 事实 |

Domain 边界（`kiana-domain/tests/h17_job_handle.rs`）：`job_handle_round_trip_binds_start_invocation_and_process_group`、
`job_handle_digest_detects_pid_or_scope_tamper`、`job_handle_requires_process_group_epoch_and_expiry`。
Daemon 接线守卫（`kiana-daemon/tests/h17_job_handle.rs`）：四个卡片命名守卫加
`behavior_fixtures_cover_named_acceptance` 与 `behavioural_fixtures_are_wired_into_the_h17_lane`。

## 3. Proof ceiling and handoff

H17 proof ceiling 为 `source`：长任务已具备 typed handle、owner/authority/TTL fencing、bounded
poll cursor 和重启拒绝 attach 语义；2026-10-17 的 CI-only 行为夹具把四条卡片命名验收从
source marker 提升为 daemon lib 内的运行期断言（仍不提升 proof level）。当前 process/output maps
与句柄记录仍是本地 adapter；跨进程 JobHandle projector、进程组持久恢复、完整输出 Artifact
分段、OS/power-loss 和 external/live/physical proof 留待 H24/H25、PD/INT。
