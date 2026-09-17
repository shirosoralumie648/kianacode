# P0-J1-03 进程组确认与 `stop_confirmed` 基线

> 快照日期：2026-09-18。本页回填 shell/MCP/long-process 停止确认；本地不运行测试，验收夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`P0-J1-03`](../roadmap.md#step-p0-j1-03) |
| feature_status | `implemented`（daemon shell/MCP/process source；CI-only process-group fixtures） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | adapter stop reports and ControlPlane cancellation facts are authoritative; `stop_confirmed` is evidence, not a user hint |
| this step does | shell commands create an isolated process group, cancellation/timeout terminates and polls the group, drains bounded pipes, reports `stop_confirmed`, and returns explicit `shell_result_unknown:*` when the group cannot be confirmed stopped; MCP stdio and long-running process control reuse the same termination helper |
| this step does not | 不把 `start_kill`/Drop 当作确认、不把未确认停止标成 Cancelled/Completed、不新增取消执行循环；mid-stream race and full cross-process reconciliation remain P0-J1-04/CP/PD |

## 1. Contract

The child process and its descendants are placed in a process group before any effect. On cancel or
timeout the adapter sends bounded termination signals, waits for the leader and group to disappear,
then drains output. Only a positive absence check sets `stop_confirmed=true`; inability to confirm
is surfaced as a structured Unknown result so core can quarantine/reconcile instead of claiming a
successful cancellation.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `cancel_confirms_shell_process_group_stopped` | process group setup, termination, absence polling, output drain and stop evidence are wired through shell/MCP/process paths |
| `unconfirmed_process_stop_is_result_unknown_not_cancelled_success` | stop uncertainty uses result_unknown/reconciliation markers and cannot be reported as a successful cancellation |

## 3. Proof ceiling and handoff

P0-J1-03 proof ceiling is `source`: process-group lifecycle, stop confirmation and Unknown fallback
are explicit. OS/kernel edge cases, descendant races, mid-stream late-delta fencing, durable
reconciliation and cross-process/power-loss proof remain P0-J1-04 and CP/PD/INT work.
