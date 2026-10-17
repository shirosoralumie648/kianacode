# UI-06 feed cursor、gap、replay 与背压基线

> 快照日期：2026-09-24；2026-10-17 完成切片（CI-only）。UI-06 的协议、daemon feed projection 和拒绝夹具由 GitHub Actions 执行；本地不运行测试、构建或检查。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`UI-06`](ui-entrypoints.md#step-ui-06) |
| source snapshot | `441df52431c1`（本分支起点）+ 2026-10-17 UI-06 完成切片；历史切片起点 `0cc07fd6`（feed DTO slice）与 `a5052381`（gap boundary cursor recovery） |
| feature_status | `implemented`（versioned feed DTO + bounded daemon projection + consumer state machine + 真实 `DaemonHost` fault fixtures） |
| proof_level | `source`；不提升为 local_behavior/durable/live/physical |
| canonical path | 已提交 run 事件 → `RunStreamBus` 有界投影 → cursor/boundary/replay/gap/heartbeat/terminal 帧 → UI subscriber；gap 后用 UI-05 snapshot page 按服务端 cursor 重新 hydrate |

UI-06 为 daemon feed 定义独立的 `UiFeedCursorV1`、`UiFeedFrameV1` 和 `UiFeedGapV1`。游标绑定 instance、authority epoch、feed sequence、snapshot cursor 和 digest；帧区分 snapshot boundary、delta、heartbeat、gap、terminal 与 unknown。feed 只展示已提交事件，不授予 capability、approval 或执行权限。

每个 run 保留最多 `FEED_REPLAY_WINDOW`（128）个 envelope，广播通道固定为 `UI_FEED_QUEUE_CAPACITY`（256）。重连只从窗口内按服务端 sequence 回放；旧 epoch、foreign instance、sequence ahead、窗口过期和广播 lag 产生明确 gap，并要求重新 hydrate snapshot。sequence 不由 timestamp 或客户端决定。

2026-10-17 完成切片补齐三件事：

1. **retention 语义真实化**：`UiFeedGapReason::TerminalRetention` 不再是无主枚举值。同 instance、同 epoch 的 cursor 若领先一个**已被有界表淘汰**的 channel（`current_sequence == 0`），现在报告 `terminal_retention` 而不是把服务端保留期丢失说成客户端「跳过了序列」；两种情况都 `snapshot_required`。sequence ahead 只在服务端确实还持有更高位置时才报告。
2. **consumer state machine 收口**：`NotificationStreamBridge` 的 `ingest()` 现在区分三种此前被折叠的情形——已经看过的位置返回 `Replayed`（不重复投递）、回放的 terminal 仍然返回 `Terminal`（terminal retention 对重连客户端可见，不再降级成「重复帧」）、无法解释的 `UiFeedFrameKind::Unknown` 返回新的 `NotificationStreamDisposition::Unknown`：cursor 前进（位置是服务端给的）但 `snapshot_required = true`，绝不被渲染成 delta。
3. **真实组合根 fault fixtures**：新增 `kiana-daemon/tests/ui06_feed_replay.rs`，用真实 `KianaHarness` run + `DaemonHost` 证明 boundary→delta→terminal 的 live 序列、duplicate/replay 抑制、terminal retention、四类 gap（old epoch / instance changed / sequence ahead / replay expired）、慢消费者背压与重启后的 `instance_changed`；hydration 走 UI-05 的只读 `ui_snapshot_page`。新增声明式 fault plan `kiana-daemon/tests/fixtures/ui06-feed-replay.json`（队列/窗口预算 + gap 原因矩阵 + required/denied）。

feed 仍然只有一条投影路径：不追加事实、不调用模型、不构造 capability broker、不打开第二条执行循环。

## 2. Failure-first fixture matrix

协议（`kiana-protocol/tests/ui06_feed_contract.rs`）：

| Fixture | 断言 |
|---|---|
| `feed_cursor_round_trip_and_gap_are_explicit` | cursor 编解码、digest、gap range 和 snapshot-required 约束保持 |
| `feed_cursor_digest_and_terminal_boundary_fail_closed` | 篡改 cursor digest、伪造 terminal flag、跨 epoch snapshot cursor 被拒绝 |

Daemon 单元（`kiana-daemon/src/run_stream.rs`）：

| Fixture | 断言 |
|---|---|
| `feed_replays_only_the_bounded_window_and_marks_boundaries` | 首帧为 snapshot boundary，断线后只回放 bounded history |
| `resumed_feed_replays_nothing_when_the_cursor_is_already_current` | 已看过的位置只给 boundary，不重复投递同一帧，且不产生 gap |
| `feed_rejects_foreign_epoch_and_expired_replay` | old epoch 与 replay expired 返回 gap；随后 snapshot boundary 使用服务端当前 cursor，不回退到旧请求 cursor |
| `retired_feed_window_reports_terminal_retention_and_requires_snapshot` | 有界表淘汰该 run 的窗口后，旧 cursor 得到 `terminal_retention` gap，`from` 是请求位置、`to`/boundary 是服务端当前 cursor |
| `slow_consumer_gets_backpressure_gap_and_terminal_delta_is_denied` | 慢消费者收到 backpressure gap；terminal 后 delta/duplicate terminal 被拒绝，指标递增 |
| `stalled_consumer_keeps_the_queue_and_history_bounded` | 广播队列与 history window 有界；停滞消费者不会让任何已提交事实失败，滞后计数与 gap 计数递增 |
| `heartbeat_is_bounded_and_explicit` | heartbeat 帧带当前 feed/snapshot cursor，并通过 DTO 校验 |
| `late_progress_cannot_overwrite_terminal_outcome` | terminal 之后 late progress 与第二个 terminal 都被拒绝，重连回放仍是原终态 |
| `reconnect_replays_terminal_without_reinvoking_runner_or_model` | 重连只回放终态，不重新进入 runner |

Daemon consumer bridge（`kiana-daemon/tests/nm13_notification_stream.rs`）：

| Fixture | 断言 |
|---|---|
| `bridge_requires_boundary_then_handles_after_cursor_heartbeat_gap_and_dispose` | 先 boundary、after cursor、heartbeat、sequence gap、dispose 的处置 |
| `old_epoch_and_terminal_late_update_fail_closed` | old epoch 与 snapshot-required 状态先于位置推进 |
| `terminal_retention_and_unknown_frames_never_collapse_into_a_delta` | 回放 terminal 仍返回 `Terminal`（幂等、可选位置不倒退），terminal 后 delta 是 `TerminalUpdate` 错误；`Unknown` 帧返回 `Unknown` 并置 `snapshot_required`，cursor 前进但终态标记不变 |

Daemon 真实 `DaemonHost` + EventStore（`kiana-daemon/tests/ui06_feed_replay.rs`）：

| Fixture | 断言 |
|---|---|
| `ui06_feed_fault_plan_binds_the_bounded_budget_and_the_deny_matrix` | fault plan schema、队列/窗口预算、七个 gap 原因与 required/denied 矩阵 |
| `live_feed_keeps_the_boundary_the_sequence_and_the_retained_terminal` | boundary 先行、sequence 稠密且由服务端决定、terminal 是最后一帧、heartbeat 带当前 cursor、重复位置不重放、无 cursor 重连仍看得到 terminal retention、replay 不追加事实也不重新调用模型 |
| `gap_frames_use_the_server_cursor_and_hydrate_through_the_snapshot_page` | old epoch / foreign instance / sequence ahead 三类 gap、gap 目标等于服务端当前 cursor 与随后的 boundary，用服务端 cursor 复订不再 gap，随后用 UI-05 `ui_snapshot_page` hydrate 到全部已提交事实且重复 hydrate 摘要一致 |
| `a_stalled_consumer_gaps_but_never_blocks_committed_facts` | 消费者停滞时 run 仍完成、事实仍提交；滞后与 gap 计数递增；慢消费者拿到 `backpressure` gap 而不是静默丢帧 |
| `restarted_host_requires_hydration_and_keeps_the_same_ledger` | 新宿主实例的 feed instance 变化 → `instance_changed` gap、boundary 是服务端当前 cursor；同一 `JsonlEventLog` 账本上 hydrate 仍看得到这条 run 的终态事实 |

Core 守卫（`kiana-core/tests/ui06_feed_replay_guard.rs`）：

| Guard | 断言 |
|---|---|
| `feed_replay_stays_a_bounded_projection_and_never_an_execution_path` | protocol/daemon facade 保留 cursor、gap、replay window、backpressure 和 terminal markers，未新增第二执行循环 |
| `ui06_feed_fault_matrix_stays_wired_to_the_lane_and_the_auto_shards` | 九个源/行为夹具名绑定 baseline 与 `run_stream.rs`；fault plan 预算（256/128）等于源码常量；七个 gap 原因保留；daemon 夹具只走 `host.handle`/`subscribe_run_feed_after`/`ui_snapshot_page`/`persisted_events` 且不含 `EventStorePort`/`CapabilityBroker`/`.append(`；夹具无 `#[ignore]`/`--ignored`；手动 lane 保持 dispatch-only、`contents: read` 且串行执行五个 target 加 `--no-run`；自动分片册仍收录 `ui06_feed_replay_guard` 且 `kiana-daemon` 是整 crate 分片 |

## 3. Lane wiring

- 手动 lane：`.github/workflows/ui06-feed-replay.yml`（`workflow_dispatch`-only、`permissions: contents: read`、`cargo fetch --locked`、fmt 门，随后串行跑 `kiana-protocol ui06_feed_contract`、`kiana-daemon --lib run_stream::tests`、`kiana-daemon ui06_feed_replay`、`kiana-daemon nm13_notification_stream`、`kiana-core ui06_feed_replay_guard`，并以 `cargo test -p kiana-protocol -p kiana-daemon -p kiana-core --no-run --locked` 收尾）。
- 自动覆盖：统一 `ci.yml` 的 `kiana-core-s*` 分片（已列 `ui06_feed_replay_guard`）；`kiana-daemon` 与 `kiana-protocol` 是整 crate 分片（`targets: null`），会自动发现 `ui06_feed_replay`、`nm13_notification_stream` 与 `ui06_feed_contract`。
- 无 `Cargo.toml`/`Cargo.lock`/`package.json` 变化，无新依赖，无第二执行循环；五个模型可见工具面不变。

## 4. Durable boundary and limitations

feed history 和 broadcast queue 都是 daemon 进程内、有限容量的展示投影；它们不是 EventLog 的第二事实源。订阅建立时发送 boundary，之后按 sequence 投影；检测到 gap 后客户端必须走 UI-05 snapshot query 重新 hydrate，再以服务端 gap target cursor 继续，不能把旧 after cursor 作为新的 boundary。terminal 只允许一次，terminal 后不接受 delta；terminal retention 只承诺同一实例内、有界表未淘汰时的回放，不承诺跨 daemon restart 的 durable replay。`UiFeedBackpressureMetrics` 只记录队列容量、窗口、lag、gap 和 terminal rejection 计数。

当前证据只覆盖源码与 GitHub CI wiring，CI 结果未等待。UI-06 不实现跨进程 socket/SSE adapter、持久化订阅收件箱、notification delivery、artifact 内容下载、provider/live timing 或 physical proof；UI-07 typed client、UI-08 reducer、UI-18 SSE reconnect 与 UI-33 crash recovery 继续依赖本步骤的 contract。旧 `RunStreamSubscription` API 保留兼容，不改变既有 stream gap 语义。生命周期指标只有进程内计数，没有持久化或跨进程对账。

## 5. Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

CI runs `cargo fmt --all --check`, protocol UI-06 feed contract fixtures, daemon `run_stream` unit
fixtures, the real `DaemonHost` UI-06 feed fixtures, the NM-13 consumer bridge fixtures, the core
source guard and a `--no-run` workspace-target compile. No local tests/build/check/clippy/smoke are
run; CI result is not awaited.
