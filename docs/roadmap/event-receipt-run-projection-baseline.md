# ER-08 Run/Turn 状态投影与 terminal 约束基线

> 快照日期：2026-09-17。本页记录 run projector 的 source/CI 边界；RunState 是 EventLog
> 的只读折叠结果，不是新的状态事实或执行授权。

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-08`](event-receipt-recovery.md#step-er-08) |
| feature_status | `implemented`（RunPhase/RunOutcome terminal reducer 与 replay guard） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | EventLog run/turn facts；`project_run_state` 只读投影 |
| this step does | authorized/queued/running/awaiting-approval/cancelling/terminal phases、new prompt turn reset、same terminal replay idempotence、conflicting terminal fail-closed、post-terminal event suppression |
| this step does not | 不自动 resume、执行 Broker/Runner、修改 EventLog 或把 projection/cache 当 authority；Invocation/Approval/RunSnapshot durable projector 继续由 ER-09+ / CP-21 负责 |

## 1. Contract

`kiana-core::project_run_state` 先按同一 run stream version（或跨 aggregate 的 durable append
order）折叠事件，并以 event ID 去重。`run.prompt` 明确开启下一 turn，清除上一 turn 的 terminal
outcome；没有新 prompt，任何晚到 approval/effect/event 都不能复活终态。相同 terminal kind
重复回放只保留首次事实，不同 terminal kind 在同一 turn 形成 `TerminalConflict` 并 fail-closed。
审批请求、取消中和运行事件分别投影为 `AwaitingApproval`/`Cancelling`/`Running`，不触发
额外执行循环。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `run_projection_follows_approval_cancel_and_terminal_phases` | approval→cancelling→cancelled phase/outcome 按事实顺序折叠 |
| `terminal_projection_is_idempotent_and_rejects_conflicting_kinds` | 同 terminal replay 稳定；不同 terminal kind 返回 TerminalConflict |
| `late_effect_is_ignored_until_a_new_prompt_turn` | terminal 后晚到 effect 不改变状态；新 turn prompt 才清除 outcome |
| `er08_run_projection_is_terminal_and_replay_safe` | source guard 固定 terminal reducer、event ID 去重、late-event suppression 和无授权边界 |

## 3. Proof ceiling and handoff

ER-08 proof ceiling 为 `source`：RunState reducer 的 phase/terminal/replay 语义由 CI-only
fixtures 固化；未运行本地测试。Invocation/Execution/Attempt 投影、pending/lease/budget
rebuild、RunSnapshot/restart/resume 和外部/live/physical proof 留待 ER-09+、CP-18+、PD。
