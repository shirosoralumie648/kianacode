# H28 进度、停滞检测与有界修复策略基线

> 快照日期：2026-09-19。本页记录 H28 的 source slice 与 CI-only 夹具；本地不运行测试，GitHub Actions 负责运行域、协议和 Core/Runner source guard。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H28`](harness.md#step-h28) |
| feature_status | `implemented`（domain/runner checkpoint/protocol source；CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| authority | ProgressTracker 只产生 bounded feedback/clarification/blocked 建议；Core/ControlPlane 仍决定是否能发起下一次模型或工具动作 |
| this step does | 新增 observation/workspace/artifact/verification/job cursor/heartbeat 证据指纹；识别 A→B→A、重复失败、空 turn、停滞窗口和 stop-hook budget；ProgressTracker 随 Harness checkpoint 保存/恢复，既有 repeated-tool threshold 保持有效 |
| this step does not | 不把模型自述当进度、不允许无限 stop-hook feedback、不让 job poll 的 cursor 变化被误判为停滞；本切片不宣称所有 live hook/provider 结果或跨进程 process projector 已接入生产 |

## 1. Contract

`ProgressEvidence` 排除纯模型鼓励文本，只接受 observation digest、workspace/artifact
revision、verification digest、JobHandle cursor 或 heartbeat。没有这些有效变化时，窗口内的
首次停滞只允许一次反馈，第二次请求澄清，达到 no-progress limit 后 blocked。相同 failure
digest 达到阈值或检测到 A→B→A 循环不会无限重试。

`stop_hook_feedback` 必须消耗显式剩余额度；额度为零直接 blocked。Runner 继续保留原有相同
工具调用阈值，并把 ProgressTracker checkpoint 化；Reducer 不执行模型、Broker、Hook 或
JobHandle 操作，因此不会新增第二条执行循环。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `alternating_failed_calls_hit_a_b_a_no_progress_limit` | alternating failures eventually become blocked instead of receiving infinite repair prompts |
| `progressing_job_poll_is_bounded_without_false_loop_failure` | advancing cursor/heartbeat is real progress and resets stall count |
| `empty_turn_and_stop_hook_feedback_are_bounded` | empty turns and stop-hook feedback consume a finite budget |
| `progress_reducer_and_runner_threshold_share_the_same_bounded_path` | runner retains repeated-tool guard and checkpointed ProgressTracker |

## 3. Proof ceiling and handoff

H28 proof ceiling is `source`: evidence fingerprint, bounded reducer and checkpoint field are
established. Actual hook/provider scheduling, external job heartbeat truth, durable cross-process
tracker projection and live performance calibration remain later H29/PD/ER/provider work.
