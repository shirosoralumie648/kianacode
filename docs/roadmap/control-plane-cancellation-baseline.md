# CP-15 cancellation state 与停止屏障基线

> 快照日期：2026-09-17。本页记录持久取消意图、审批/排队竞态和终态事实的
> source/CI 边界；不把进程内 signal 或 Runner ACK 写成现实世界停止证明。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CP-15`](control-plane.md#step-cp-15) |
| feature_status | `implemented`（RunCancellationFact、持久 `run.cancelling` 屏障、终态 receipt markers） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | ControlPlane EventLog transition + RunCancellationFact；watch signal 只负责进程内唤醒 |
| this step does | cancellation schema/state transition、command digest/idempotency、明确 Run/Invocation targets、cancel-before-signal、pending approval NotExecuted、Cancelled/ResultUnknown 终态绑定 |
| this step does not | 不声称 handler/子进程/文件/远端 effect 已停止，不实现 CP-16 typed StopReport、跨进程 recovery、外部对账、Secret/egress 或 external/live/physical proof |

## 1. Contract

`kiana-domain/src/cancellation.rs` 新增 strict `RunCancellationFact`。事实只保存脱敏
reason digest、actor、排序后的 invocation targets、run stream expected version、停止请求/确认
标记和 fact digest；`Cancelled` 必须有 `stop_confirmed=true`，无法确认的停止只能是
`ResultUnknown`。unknown fields、状态/摘要/目标顺序冲突和 active 伪造全部 fail-closed。

`ControlPlane::cancel_run` 先以 `run.cancel` command digest 和 run aggregate read-set CAS
提交 `run.cancelling`，再发既有 `signal_cancel` 与 Runner `Cancel`。相同 command/digest 可重放，
不同 actor/reason/target 冲突；竞态中已经提交终态时只读返回既有结果，不再发送取消。等待中的
审批先失效，并为每个 pending invocation 写 `not_executed` synthetic tool result。终态
`run.cancelled`/`run.result_unknown` 通过 `record_terminal_event` 重新构造并校验嵌套 fact。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `cancellation_fact_requires_durable_stop_evidence_for_cancelled` | stopping/unknown 可表达；cancelled 缺 stop confirmation 拒绝；targets canonical |
| `cancellation_fact_is_strict_and_digest_bound` | unknown field、取消请求标记篡改和 reason digest 违反 strict contract 时拒绝 |
| `cp15_persists_cancel_before_signal_and_closes_queued_invocations` | source guard 固定 CAS→signal 顺序、command conflict/terminal guard、审批/排队 NotExecuted、terminal/result-unknown boundary |

## 3. Proof ceiling and handoff

CP-15 proof ceiling 为 `source`：持久 cancellation intent 与同一 EventLog/CAS 屏障已固定，
但现有 `await_capability_stop` 仍是进程内 bounded watch，不是 handler stop proof；物理锁释放、
Shell/Patch/MCP descendant 与文件身份证据留给 CP-16，撤销传播/重启恢复/Unknown 对账留给
CP-17/19/20，ER/PD/SC 与 external/live/physical proof 仍未宣称。
