# P2-M2-01 UI 投影合同基线

> 快照日期：2026-09-18。本文记录 UI snapshot/action/cursor/epoch 的 projection 边界；运行时验收由 GitHub Actions 负责，本地不运行测试。

## Snapshot and action contract

协议层提供 versioned `UiSnapshotV1`、`UiFeedEnvelope`、`UiActionV1`、`UiActionResult` 及 cursor/epoch、pending action、capability、run/session/artifact/notice/limitation 字段，严格校验 schema、digest、scope、revision、未知字段和有界 JSON。兼容 surface 仍提供轻量 `UiSnapshot`/`UiAction`，由 daemon `RunStreamBus` 维护展示 cursor；两者都只是 projection/precondition，不携带 Broker authority。

`DaemonHost::ui_snapshot` 从 authenticated principal 拥有的 EventLog 事实投影 session/run/status/pending approvals，并附独立 run-stream cursor；`ui_events` 只返回 owner-scoped 事实。Web、Workbench 和其它 surface 通过同一 DaemonHost/ControlPlane path 消费 snapshot，不能把进程内 UI 状态当作事实源。

## Stale action and feed safety

`RunStreamBus::claim_ui_action` 在把动作交给控制面前原子校验 target/key、epoch 和 cursor，拒绝 stale/replayed action，并只推进 display cursor。`subscribe_after` 在 epoch 不同、cursor 超前或增量缺失时设置 gap；Web 发送 machine-readable `snapshot_required_after_stream_gap`，前端标记 incomplete 并回到 snapshot hydration，不把丢失 delta 当成完成。终态/approval 事件推进 UI cursor，但实际批准、取消、resume 等仍须走 ControlPlane。

## CI-only 验收

`stale_ui_action_is_rejected_by_epoch` 包含 daemon 的 stale/replay fixture 及跨 surface source guard；protocol `ui01_dto` 回归验证 versioned snapshot/feed/action DTO 的 round-trip、unknown field 和 Unknown retry 规则。

```text
cargo fmt --all --check
cargo test -p kiana-daemon --test p2_m2_01_ui_projection --locked -- --test-threads=1
cargo test -p kiana-daemon --lib stale_ui_action_is_rejected_by_epoch --locked -- --test-threads=1
cargo test -p kiana-protocol --test ui01_dto --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- RunStreamBus cursor/action replay key 是当前 daemon 进程内 bounded projection；跨进程 durable UI cursor/instance record、notification/read state 和 reconnect persistence 仍由 UI/NM/PD 专项负责。
- snapshot/feed 不替代 EventLog、Receipt、Approval 或 ControlPlane；UI action claim 只是 stale precondition，实际副作用、权限和审批继续由原 authority 决定。
- 本切片不声称多 tab/多进程 live 交付、外部 human authentication、browser/OS physical effects 或 durable/live proof；gap 只产生保守 incomplete 状态并要求重新 hydration。
