# P4-J7-02 wire sequence/epoch 基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Additive stream contract

`RunStreamEnvelope` 在不改变 `kiana.protocol.v1` 请求/响应 schema 的前提下，以可选兼容字段
`epoch`、`sequence` 和 `ui_cursor` 表示 daemon incarnation、每 run 单调展示序号和 UI 投影
游标。旧 envelope（两字段皆空/sequence=0）仍可读取；新客户端通过 `advance_cursor` 拒绝
epoch 变化、非连续 gap 和非法空字段，重复 envelope 返回幂等 false。

`RunStreamBus` 为每个 run 从 1 开始递增 sequence；delta/usage/tool/approval/error/terminal
共享同一 envelope，terminal 保留有限重放。UI cursor 仅在 terminal/approval 或 committed
projection 前进，不能取代 EventLog/Receipt；Web SSE 使用 `epoch:sequence` 的 Last-Event-ID，
gap/lag/epoch 变化回到 snapshot hydration。

`run_stream_sequence_is_monotonic` 通过真实 bus 验证两个 delta、terminal、late cursor replay
的连续序号和 epoch；协议 fixture 额外验证 duplicate/gap/epoch-change 行为。

## CI-only 验收

```text
cargo fmt --all --check
cargo test -p kiana-protocol --test p4_j7_02_sequence --locked -- --test-threads=1
cargo test -p kiana-daemon --lib run_stream_sequence_is_monotonic --locked -- --test-threads=1
cargo test -p kiana-daemon --test p2_m2_01_ui_projection --locked -- --test-threads=1
cargo test -p kiana-entrypoints --test p2_m5_01_web_sync --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行格式、workspace test-target 静态编译和 `git diff --check`；不执行测试，也不等待
GitHub CI。

## 限制与交接

- sequence/epoch 是进程内展示投影，不是跨进程 durable cursor；进程重启需 snapshot hydration，
  不能把旧 delta 当成事实或执行结果。
- broadcast lag/terminal retention、SSE 重连和 browser state 仍是 best-effort；不声称外部
  provider stream、跨设备同步、网络交付或 live/physical 证明。
