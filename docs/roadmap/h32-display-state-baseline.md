# H32 事实流、展示流与三入口状态一致性基线

> 快照日期：2026-09-19。本页记录 H32 的 source slice 与 CI-only 夹具；本地不运行测试，GitHub Actions 负责运行 protocol 与 entrypoint source guard。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H32`](harness.md#step-h32) |
| feature_status | `implemented`（protocol canonical display reducer + existing daemon/entrypoint stream route；CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| authority | EventLog/ControlPlane response remains fact authority; RunDisplayState is a bounded display projection and never authorizes actions |
| this step does | 新增 canonical RunDisplayState：同一 epoch/sequence 去重，gap 要求 snapshot，epoch 变化拒绝旧事件，terminal event 即使跨 gap 也保留终态；UiSnapshot hydration 重置 gap；CLI/TTY/Web 已有 RunStreamEnvelope/RunStreamEvent/ResponseEnvelope shared path 被 source-guarded |
| this step does not | 不把展示流当事实，不由客户端本地时钟推断 completed，不宣称所有浏览器重连/慢订阅/真实网络断线已有 live durable 证明；真实 terminal snapshot 仍由 daemon/EventLog 提供 |

## 1. Contract

`RunDisplayState::apply` 只接收 versioned `RunStreamEnvelope`：同 cursor 重复事件 harmless，旧
epoch 不得覆盖新状态，非终态 sequence gap 返回 `GapRequiresSnapshot`，terminal event 可以
落下终态但保留 `gap_detected`。`hydrate` 只接受同 run 的 `UiSnapshot`，从服务器状态重建
cursor/status/text，而不是依赖客户端 elapsed time。

三个入口继续消费同一 protocol/daemon stream 类型和最终 `ResponseEnvelope`；审批、取消、
Unknown、waiting 等展示只是 projection，任何 action 仍回 ControlPlane。UI local text/delta
不能改变事实终态。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `duplicate_and_gap_rules_preserve_terminal_state` | duplicate is ignored, non-terminal gap requests snapshot, terminal is not lost |
| `old_epoch_cannot_mutate_snapshot_and_hydration_resets_display_gap` | old epoch cannot mutate state and snapshot hydration resets cursor/gap |
| `cli_tty_web_share_the_versioned_stream_and_snapshot_contract` | all three surfaces use the same stream/response/cursor contracts |
| `display_surfaces_do_not_turn_local_time_into_completion` | entrypoints derive status from response/fact path rather than local timers |

## 3. Proof ceiling and handoff

H32 proof ceiling is `source`: canonical cursor/epoch/terminal reducer and three-surface route
alignment are established. Real slow-subscriber behavior, browser reconnect, durable feed replay,
Desktop embedding and live network timing remain H35/H36/PD/provider evidence.
