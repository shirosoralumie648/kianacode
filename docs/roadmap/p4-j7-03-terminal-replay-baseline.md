# P4-J7-03 事件投影与 terminal replay 基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Event projection

`RunStreamBus::project_committed` 只消费已提交 EventLog：usage/model-turn 投影为 `Usage`，
capability/tool-result 为 `ToolCall`，approval 请求为 `ApprovalRequested`，失败/Unknown 为
`Error`。这些 envelope 是展示投影，不是执行指令；未知 wire 事件降为 `Unknown` 并由客户端
安全忽略。所有事件仍与同一 run 的 sequence/epoch 绑定，EventLog/Receipt 保持事实权威。

## Late subscriber contract

每个 run channel 保留一个 bounded terminal envelope。`subscribe_after` 对 epoch 不匹配、
cursor 超前或 channel 有未见序号设置 gap；它不会伪造缺失 delta，但会把尚未见过的 terminal
重放一次，方便迟到订阅者看到确定终态。已追上的 cursor 不重复重放，过期/lag 仍要求
snapshot hydration，不能触发第二次执行。

`terminal_is_replayed_to_late_subscriber` 通过真实 bus 先投影 Usage/ToolCall/Approval/Error，
再发布 terminal，最后从空 cursor 的迟到订阅者读取 terminal 并断言 gap/sequence；core guard
固定四类投影、unknown/gap/epoch 和 no-provider/no-second-loop 边界。

## CI-only 验收

```text
cargo fmt --all --check
cargo test -p kiana-daemon --lib terminal_is_replayed_to_late_subscriber --locked -- --test-threads=1
cargo test -p kiana-core --test p4_j7_03_terminal_replay --locked -- --test-threads=1
cargo test -p kiana-protocol --test wire_contracts run_stream_events_are_additive_and_unknown_events_are_ignored --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行格式、workspace test-target 静态编译和 `git diff --check`；不执行测试，也不等待
GitHub CI。

## 限制与交接

- terminal retention、broadcast channel 和 gap 标记是进程内 best-effort；不声称跨进程 durable
  stream、无限历史回放或网络 exactly-once 送达。
- terminal replay 只展示已提交响应，不重启 Runner、不重放 capability、不替代 receipt 或
  reconciliation；外部 provider stream、设备同步和 live/physical proof 仍未覆盖。
