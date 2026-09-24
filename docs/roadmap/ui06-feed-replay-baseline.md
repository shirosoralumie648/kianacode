# UI-06 feed cursor、gap、replay 与背压基线

> 快照日期：2026-09-24。UI-06 的协议、daemon feed projection 和拒绝夹具由 GitHub Actions 执行；本地不运行测试、构建或检查。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`UI-06`](ui-entrypoints.md#step-ui-06) |
| source snapshot | `0cc07fd6`（最新 master）+ `ui-06-feed-replay-20260924` UI-06 source slice |
| feature_status | `implemented`（versioned feed DTO + bounded daemon projection + remote fixture wiring） |
| proof_level | `source`；不提升为 local_behavior/durable/live/physical |
| canonical path | committed run event → `RunStreamBus` bounded projection → snapshot boundary / replay / gap / heartbeat frame → UI subscriber |

UI-06 为 daemon feed 定义独立的 `UiFeedCursorV1`、`UiFeedFrameV1` 和 `UiFeedGapV1`。游标绑定 instance、authority epoch、feed sequence、snapshot cursor 和 digest；帧区分 snapshot boundary、delta、heartbeat、gap、terminal 与 unknown。feed 只展示已提交事件，不授予 capability、approval 或执行权限。

每个 run 保留最多 `FEED_REPLAY_WINDOW`（128）个 envelope，广播通道固定为 `UI_FEED_QUEUE_CAPACITY`（256）。重连只从窗口内按服务端 sequence 回放；旧 epoch、foreign instance、sequence ahead、窗口过期和广播 lag 产生明确 gap，并要求重新 hydrate snapshot。sequence 不由 timestamp 或客户端决定。

## 2. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `feed_cursor_round_trip_and_gap_are_explicit` | cursor 编解码、digest、gap range 和 snapshot-required 约束保持 |
| `feed_cursor_digest_and_terminal_boundary_fail_closed` | 篡改 cursor digest、伪造 terminal flag 被拒绝 |
| `feed_replays_only_the_bounded_window_and_marks_boundaries` | 首帧为 snapshot boundary，断线后只回放 bounded history |
| `feed_rejects_foreign_epoch_and_expired_replay` | old epoch 与 replay expired 返回 gap，不伪造 delta |
| `slow_consumer_gets_backpressure_gap_and_terminal_delta_is_denied` | 慢消费者收到 backpressure gap；terminal 后 delta/duplicate terminal 被拒绝，指标递增 |
| `heartbeat_is_bounded_and_explicit` | heartbeat 帧带当前 feed/snapshot cursor，并通过 DTO 校验 |
| `ui06_feed_replay_guard` | protocol/daemon facade 保留 cursor、gap、replay window、backpressure 和 terminal markers，未新增第二执行循环 |

## 3. Durable boundary and limitations

feed history 和 broadcast queue 都是 daemon 进程内、有限容量的展示投影；它们不是 EventLog 的第二事实源。订阅建立时发送 boundary，之后按 sequence 投影；检测到 gap 后客户端必须走 UI-05 snapshot query 重新 hydrate，再以新 cursor 继续。terminal 只允许一次，terminal 后不接受 delta；terminal retention 不承诺跨 daemon restart 的 durable replay。`UiFeedBackpressureMetrics` 只记录队列容量、窗口、lag、gap 和 terminal rejection 计数。

当前证据只覆盖源码与 GitHub CI wiring，CI 结果未等待。UI-06 不实现跨进程 socket/SSE adapter、持久化订阅收件箱、notification delivery、artifact 内容下载、provider/live timing 或 physical proof；UI-07 typed client、UI-08 reducer、UI-18 SSE reconnect 与 UI-33 crash recovery 继续依赖本步骤的 contract。旧 `RunStreamSubscription` API 保留兼容，不改变既有 stream gap 语义。

## 4. Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

CI runs `cargo fmt --all --check`, protocol UI-06 feed contract fixtures, daemon bounded feed fixtures, core source guard and workspace test-target compile. No local tests/build/check/clippy/smoke are run; CI result is not awaited.
