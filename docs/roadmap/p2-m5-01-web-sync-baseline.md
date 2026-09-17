# P2-M5-01 Web 快照水合与重连基线

> 快照日期：2026-09-18。本文记录 Web snapshot-first hydration、SSE cursor/epoch/gap 和重连边界；运行时验收由 GitHub Actions 负责，本地不运行测试。

## Snapshot-first lifecycle

Web 在发送 `/api/run` 前先通过 `/api/state` 获取 DaemonHost snapshot，并在 EventSource `onopen` 后再发起新 turn，避免初始 delta 丢失。snapshot 包含 owner-scoped session/run/status/thread/pending actions、projection cursor 和独立 stream cursor；浏览器只把它当作可丢弃展示状态。

`/api/events` 通过 `DaemonHost::subscribe_run_after` 使用 epoch/sequence cursor。首次 attach 到已运行任务、epoch 变化、cursor 超前、subscription lag 或 committed delta 缺失都产生 machine-readable `stream_gap`/`stream_error`，要求重新 snapshot hydration；terminal 只从 stream/receipt 读取，不由 polling 或 delta 推断完成。

## Reconnect and duplicate safety

浏览器维护每 session 的 `streamCursors`、`observedUiCursor` 和 refresh request generation。重连携带 `last-event-id`，服务端只保留 bounded terminal replay，不回放已交付 delta；前端拒绝旧 epoch/非单调 sequence，检测 gap 后标记 incomplete 并刷新 snapshot。渲染层的 `streamComplete` 只有在未 incomplete 且收到 terminal completed 时才为真，receipt 仍是结果权威。

## CI-only 验收

现有 `cli_web::web_sse_reconnect_emits_stream_gap_without_replaying_delta_items` 运行真实 loopback Web/DaemonHost/SSE 重连 fixture，确认先收到 gap、不重复首个 delta、只接收后续 delta 并最终收到 terminal。新增 `web_sync_source_contract` guard 与 Web gap unit regression 覆盖 snapshot/cursor/epoch/gap/refresh 边界。

```text
cargo fmt --all --check
cargo test -p kiana-entrypoints --test p2_m5_01_web_sync --locked -- --test-threads=1
cargo test -p kiana-entrypoints --test cli_web web_sse_reconnect_emits_stream_gap_without_replaying_delta_items --locked -- --test-threads=1
cargo test -p kiana-entrypoints --lib web_sse_gap_event_is_machine_readable_with_run_id_and_reason --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- RunStreamBus 的 delta/terminal retention、cursor 和 idempotency key 是 bounded daemon-process projection；跨进程 durable stream cursor、notification/read-state、multi-tab lease 和 reconnect checkpoint 仍由 UI/NM/PD 负责。
- snapshot hydration 只能修复展示视图，不能补回未持久化的外部 effect；gap/incomplete 不等于失败或成功，必须查询 Receipt/Incident/Recovery。
- 当前验证为 loopback/fake local process path；外部浏览器、OS notification、live provider、物理网络和跨机器 transport 不在本切片证明范围。
