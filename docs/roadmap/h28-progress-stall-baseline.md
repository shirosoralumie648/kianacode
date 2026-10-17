# H28 进度、停滞检测与有界修复策略基线

> 快照日期：2026-10-17（源切片 2026-09-19，harness 接线与 CI 夹具 2026-10-17）。本页记录
> H28 的 source slice 与 CI-only 夹具；本地不运行测试，GitHub Actions 负责运行域、runner、
> 协议和 Core source guard。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H28`](harness.md#step-h28) |
| feature_status | `implemented`（domain reducer、runner 接线、checkpoint/protocol source；CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| authority | ProgressTracker 只产生 bounded feedback/clarification/stop 建议；Core/ControlPlane 仍决定是否能发起下一次模型或工具动作 |
| this step does | observation/workspace/artifact/verification/job cursor/heartbeat 证据指纹；A→B→A、重复失败、空 turn、停滞 poll 的有界阶梯；harness 在已知失败、带 job/进程语义（`job_handle`/`process_id`/`cursor`/`heartbeat`）的结果和无工具无文本空 turn 上调用 `observe()`，注入一次反馈、发 `ClarificationRequested` 等待人工输入或停止 turn；ProgressTracker 随 Harness checkpoint 保存/恢复，既有 repeated-tool threshold 保持有效 |
| this step does not | 不把模型自述当进度、不允许无限 stop-hook feedback、不让 job poll 的 cursor 变化被误判为停滞；普通成功结果的同形 ack 不进入停滞阶梯（不同操作的同形输出会被误判为无进展，精确重复仍由 repeated-tool threshold 覆盖）；不宣称 live hook/provider 调度、external job heartbeat truth、跨进程 tracker 投影、durable/live/physical proof 或完整澄清回答 resume 闭环 |

## 1. Contract

`ProgressEvidence` 排除纯模型鼓励文本和 call/request identity，只接受 observation digest、
workspace/artifact revision、verification digest、JobHandle cursor 或 heartbeat。没有这些有效
变化时，窗口内的首次停滞只允许一次反馈，第二次请求澄清，达到 no-progress limit 后 blocked；
相同 failure digest 连续达到阈值或检测到 A→B→A 循环同样有界停止。任何可观测进展都会重置
停滞计数和重复失败窗口，因此正常的长任务 poll（同一 JobHandle、cursor/heartbeat 推进）
不会被相同工具名误伤。

`stop_hook_feedback` 必须消耗显式剩余额度；额度为零直接 blocked。harness 只在已知失败
（`FailedKnown`）、带 job/进程语义（`job_handle`/`process_id`/`cursor`/`heartbeat`）的结果和
无工具无文本空 turn 上调用 reducer：普通成功结果的同形 ack 不构成“无进展”证据（不同操作
返回同形 ack 会被误判，精确重复由 repeated-tool threshold 覆盖）。`FeedbackOnce` 还要消耗
一次 repair 预算；`RequestClarification` 生成绑定 run/turn/step 的 ClarificationRequest，把
driver 置为 `AwaitingInput` 后由 Core 持久化，runner 不再自动追加催促消息。Reducer 不执行
模型、Broker、Hook 或 JobHandle 操作，因此不会新增第二条执行循环。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `alternating_failed_calls_hit_no_progress_limit`（domain） | alternating failures eventually become blocked instead of receiving infinite repair prompts |
| `stop_hook_cannot_extend_turn_indefinitely`（domain） | stop-hook feedback consumes an explicit budget and becomes blocked when the budget is exhausted |
| `progressing_job_poll_is_bounded_without_false_loop_failure`（domain） | advancing cursor/heartbeat over more observations than the window stays progress; a stalled poll still escalates bounded |
| `empty_turn_after_tool_evidence_is_not_reported_as_progress`（domain） | an empty turn is never counted as progress even when its evidence shape differs from the previous one |
| `repeated_failure_window_resets_after_observable_progress`（domain） | repeated-failure counting only accumulates within a progress-free window |
| `alternating_failed_calls_stop_with_bounded_clarification`（runner） | real harness turns stop with a ClarificationRequested event instead of looping |
| `identical_failed_calls_hit_no_progress_limit`（runner） | identical failures stop the turn with `no_progress:repeated_failure_limit` |
| `empty_turn_cannot_complete_silently_without_bounded_feedback`（runner） | a no-tool/no-text turn receives one bounded feedback and then stops |
| `progressing_job_poll_is_bounded_without_false_loop_failure`（runner） | a progressing poll sequence still completes without false stall detection |
| `stalled_job_poll_stops_instead_of_polling_forever`（runner） | an unchanged poll sequence escalates to clarification instead of polling forever |
| `progress_reducer_and_runner_threshold_share_the_same_bounded_path`（Core guard） | reducer 阶梯与既有 repeated-tool threshold 共用同一条有界路径 |

## 3. Proof ceiling and handoff

H28 proof ceiling is `source`: evidence fingerprint, bounded reducer, harness wiring and
checkpoint field are established. The poll wait ceiling is the run's existing wall-time and
tool-call budgets (the harness does not schedule or back off the job itself); provider retry
backoff remains in `kiana-runner/src/retry.rs`. Actual hook/provider scheduling, external job
heartbeat truth, durable cross-process tracker projection, the full clarification answer/resume
loop and live performance calibration remain later H26/H29/PD/ER/provider work.
