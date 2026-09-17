# H08 Deadline / Cancellation 基线

> 快照日期：2026-09-17。本页记录 Harness、Provider port、shell/MCP effect 的取消与 deadline 接缝；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H08`](harness.md#step-h08) |
| feature_status | `implemented`（model cancellable port + existing effect stop confirmation 接线） |
| proof_level | `source`；本地仅做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | ControlPlane cancellation watch / terminal reducer；runner only propagates a signal and never reclassifies unknown effects |
| this step does | model attempt cancellation fence、deadline timeout、retry-backoff interruption、shell process-group reaping、MCP stdio cancellation/read/write timeout 和 stop confirmation |
| this step does not | 不把 future drop 当作 external effect 已停止，不把 stop 未确认当 cancelled success，不改变 P0-J1 未完成的整体状态词表或 durable recovery 结论 |

## 1. Contract

`RunCancellation` 同时保留 runner 的错误原因和可订阅 `watch<bool>` 信号。每个 model attempt
将该信号传入 `complete_admitted_cancellable` / `complete_prepared_cancellable`；默认端口在
模型无 delta 的静默期间也能被 cancellation 唤醒，Harness 的 deadline `timeout` 仍是外层最早
到期围栏。完成结果在返回后再次检查取消状态，避免 cancel 与 response 的竞态产生错误完成。

Provider 已使用不可变 `PreparedModelCall.deadline_unix_ms`，transport 的 headers/first-event/
idle/total timeout 只能收紧该期限；Harness retry backoff 也在同一 cancellation select 中等待。
shell 通过 `terminate_process_group` 后检查 leader/process group，MCP write/read/call 和 stop
使用同一 watch receiver；无法确认停止的执行进入 `result_unknown`，未启动的调用才可标为
`not_started`。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `silent_model_is_interrupted_by_deadline` | 模型不吐 delta 时 deadline 中止，不能完成 turn |
| `cancel_during_retry_wait_prevents_next_attempt` | provider retry backoff 期间取消，不发下一次 attempt |
| `unconfirmed_tool_stop_is_unknown` | source guard 固定 shell/MCP 未确认停止→result_unknown 边界 |
| `cancel_reaps_process_group_and_records_one_turn_terminal` | 复核 daemon 现有 shell process-group 取消与单终态夹具 |
| `h08_cancellation_fence_covers_model_and_effect_boundaries` | source guard 固定 cancellable model port、watch signal、reap/stop markers 和 no-bypass |

## 3. Proof ceiling and handoff

H08 proof ceiling 为 `source`：模型端口与 effect adapter 的取消信号、deadline 和停止确认已接线，
CI-only runner/daemon fixtures 固化拒绝路径。跨进程取消信号持久化、OS 强杀/电源故障窗口、真实
provider 网络与 MCP server 证据、完整 P0-J1 状态机和 live/physical proof 仍留待后续 CP/PD/P4/INT。
