# NM-13 run stream/notification bridge baseline

> 快照日期：2026-10-17（completion slice）。测试只由 GitHub Actions 执行；本步骤不在本地运行测试、
> build、check、clippy 或 smoke，且不等待 CI。

`NotificationStreamBridge` consumes the existing daemon-owned `RunStreamFeedSubscription` and
classifies versioned `UiFeedFrameV1` frames. It is snapshot-first: a delta before a boundary,
old epoch/instance, replay expiry, lag/backpressure or sequence gap yields an explicit
`SnapshotRequired`; heartbeat, terminal and disposed states remain visible. It preserves feed and
source cursor identity and never creates another broadcast bus or delivery worker.

## 2026-10-17 completion slice (CI-only)

The 2026-09-25 slice was a source-only cursor/disposition adapter. This slice closes the remaining
exit conditions of the card in the same daemon adapter (no new crate, no new bus, no new loop):

- **subscribe-then-hydrate / merge window**：boundary 前一律 `SnapshotRequired`（首次 attach 不渲染任何
  delta）。一旦 bridge 已采纳过 boundary（reconnect/recovery），在等待新 boundary 期间到达的帧进入有界
  `NOTIFICATION_STREAM_MERGE_WINDOW = 64` 窗口；新 boundary 采纳时，位置 ≤ boundary 的帧被快照取代而丢弃，
  位置在 boundary 之后、且从 `boundary+1` 起连续的帧按序合并并交给 surface 渲染；窗口溢出或合并集合不连续
  一律清空窗口并保持 `snapshot_required`，绝不把部分合并报告成完整。
- **按 ID 去重**：`NOTIFICATION_STREAM_SEEN_EVENT_IDS = 256` 的有界记忆记录已投递 event id。同一事件在更晚
  feed 位置被重新发布时返回 `Replayed` 且**不推进** feed 位置；合并窗口里已投递过的 id 也不会二次投递。
  记忆溢出时最旧的 id 先被忘记（有界内存优先于无限增长）。
- **foreign run 拒绝**：帧载荷顶层携带 `run_id` 时必须是本 bridge 的 run；外来 run → 结构化
  `NotificationStreamError::ForeignRun`，`run_id` 不可解析 → `Invalid("frame_run")`，两种情况都不移动任何
  cursor 状态（拒绝路径零副作用）。
- **terminal 丢失告警**：`terminal_observed()` 与 `terminal_query_required()`。feed 在未观测到 terminal 时
  被 dispose（或仍处在 snapshot-required 状态）→ 必须回读 `run.finished`/approval/incident 权威事实，
  不允许把“没有终态”当成“已完成”或“仍在运行”。`Error` delta（`run.failed` 投影）本身不是终态事实。
- **lagged / retired / late socket**：`Backpressure` gap、`TerminalRetention` gap、`ReplayExpired`、
  `SequenceGap`、`OldEpoch`、`InstanceChanged` 全部映射成显式 `SnapshotRequired(reason)`；迟到的 socket 仍能
  看到保留期内的回放 terminal（`replay = true` → `Terminal`），保留期已淘汰的窗口则以 gap 暴露，绝不沉默。
- **terminal retention 与易失 delta**：回放 terminal 仍返回 `Terminal` 并置 `cursor.terminal`；重复位置返回
  `Replayed`；`Unknown` 帧按服务端位置前进但 `snapshot_required = true`，绝不当 delta 渲染。bridge 只把 delta
  与合并帧交还 surface 渲染，自己不写任何持久化状态。

### Fixtures（先拒绝在前）

| fixture | 断言 |
|---|---|
| `fixture_declares_snapshot_first_gap_and_disposed_contract` | 声明式 fixture 绑定 snapshot-first / gap / merge window / event id / foreign run / terminal loss |
| `bridge_requires_boundary_then_handles_after_cursor_heartbeat_gap_and_dispose` | boundary 前 delta 拒绝、after cursor、heartbeat、disposed（既有，未放宽） |
| `old_epoch_and_terminal_late_update_fail_closed` | old epoch gap 与 terminal 之后 delta 的围栏（既有，未放宽） |
| `terminal_retention_and_unknown_frames_never_collapse_into_a_delta` | 回放 terminal 可见、`Unknown` 不当 delta（既有，未放宽） |
| `the_first_attach_hydrates_before_it_renders_and_keeps_no_window` | 首次 attach 不缓冲任何帧，boundary 后才接受 delta |
| `lagged_and_retired_gaps_never_report_completion` | `Backpressure` gap 不作终态、后续 delta 被围栏、新 boundary 后恢复 |
| `a_late_socket_keeps_the_retained_terminal_and_a_retired_window_stays_visible` | 迟到 socket 看到回放 terminal；已淘汰窗口报 `TerminalRetention` gap |
| `a_recovery_window_merges_only_the_frames_the_snapshot_does_not_contain` | boundary 前/后帧的合并分类、合并 terminal 仍可见、之后 delta 是 `TerminalUpdate` |
| `a_recovery_window_never_delivers_an_event_twice` | 合并窗口与 live 路径都按 event id 去重且不移动位置 |
| `an_overflowing_merge_window_fails_closed_instead_of_reporting_a_partial_merge` | 窗口溢出清空并保持 snapshot-required |
| `a_foreign_run_payload_is_refused_before_any_state_moves` | 外来 run 与不可解析 run_id 拒绝且 cursor 不变 |
| `a_feed_that_ends_without_a_terminal_raises_the_query_alarm` | dispose 无 terminal → query alarm；有 terminal → 无告警 |
| `the_event_id_memory_is_bounded_and_the_oldest_id_is_forgotten_first` | 最新 id 仍去重、被淘汰 id 不再识别（有界内存的取舍） |

### 接线

- 手动 lane `.github/workflows/nm13-notification-stream.yml`（`workflow_dispatch`-only、
  `permissions: contents: read`、`cargo fetch --locked` + `cargo fmt --all --check` + 两个 target
  `--locked -- --test-threads=1` + 两条 `--no-run --locked`）。
- 自动覆盖：统一 `ci.yml` 的 `kiana-daemon` 整 crate 分片（`targets: null`）发现
  `nm13_notification_stream` 与 `nm13_notification_stream_guard`；`scripts/ci/test-shards.json` 未改。

## Known limits

`feature_status=implemented`; `proof_level=source`. The bridge is a daemon adapter over the existing
RunStreamBus, not a durable notification fact source. The merge window and the event id memory are
in-process bounded state: a fresh process cannot resume a merge window, and evicting the oldest event
id means an extremely old re-published event can be accepted again (the explicit trade-off of the
bound). `take_merged_frames()` hands back volatile display frames that a surface must reconcile with
committed facts; terminal retention only covers the daemon's bounded retention window, so a terminal
lost across a restart still has to be read back from the committed projection. Browser/SSE/CLI/TTY/
Desktop wiring (NM-14/NM-15), cross-process subscriber recovery, notification action reconciliation
(NM-18) and physical/live delivery proof remain later steps. Verification is CI-only: the local
session ran `cargo fmt --all` only and the receipt is not awaited.
