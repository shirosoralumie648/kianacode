# P1-J8-01 Observability 与 trace/receipt 基线

> 快照日期：2026-09-18。本文记录 run receipt 的可复核观测投影；运行时验收由 GitHub Actions 负责，本地不运行测试。

## 统一 receipt 投影

`kiana-core::receipt_from_events` 仍只读取 committed EventLog，并新增只读 `observability` 区段。它绑定：

- `source_cursor`、有界去重 `source_event_ids` 和由两者计算的 `persistence_revision`，用于定位 projection 代际；
- `project_model_attempts` 的 provider/model/route/prompt hash、stream/latency/stop/usage/retry/cache 投影；缺 usage、截断、超时、重试和 malformed metadata 不会标 `ok`；
- `project_capability_attempts` 的 admission/approval/effect/stop/fence/action digest 投影；未知效果和未确认停止保持 Unknown/fenced；
- `project_spans` 的 run/turn/invocation 生命周期，所有行带 source cursor/event；
- `capability.decision` 的 policy/gate verdict、reason、authority epoch、action digest/tool args hash；模型、UI、provider 自述不能生成这些事实；
- cancellation event 的 bounded reason、stop confirmation 和 action digest。

所有新字段都经过既有 EventLog redaction boundary；只保留低基数分类、digest、ID、cursor 和有界原因，不携带 prompt、tool arguments、shell/path、header、token 或 raw provider response。`observability` 是 EventLog 的可重建 projection，不是授权、Receipt Outcome、provider invoice 或外部 telemetry sink。

## Deny-first 与边界

policy/gate deny、过期/缺失 approval、TOCTOU、取消、stop 未确认、effect unknown、来源 cursor/gap 和 malformed metadata 均不能被观测层改写为成功；观测 reducer 不调用 Broker、Provider、Runner 或 exporter。`policy-decision-trace.v1` 的输入/context/trace digest 保留在 policy 侧，receipt 只映射可展示的 verdict/reason/hash。

## CI-only 验收

`observability_fields_are_traceable_and_secret_free` source guard 约束 receipt 组合、三类 typed projection、policy/cancel/action hash、redaction 和 no-execution 边界；OA-08/OA-09/OA-07 的 domain/core runtime fixtures 继续作为 projection 回归来源。

```text
cargo fmt --all --check
cargo test -p kiana-core --test p1_j8_01_observability --locked -- --test-threads=1
cargo test -p kiana-core --test oa08_model_instrumentation --locked -- --test-threads=1
cargo test -p kiana-core --test oa09_capability_instrumentation --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- 当前 `observability`、model/capability/span projections 均为源事件上的进程内重建；durable projector checkpoint、metric/trace exporter、跨进程 query cursor 和 live backend 仍由 OA-10+ / PD / ER / SC 负责。
- `tool_args_hash` 使用 server-owned normalized action/args fingerprint，不保留原始参数；它不是外部系统已接收或执行的证明。`persistence_revision` 是 source cursor/event digest，不等同于 fsync/跨存储事务。
- provider receipt、真实费用/账单、外部业务 outcome、physical/live telemetry 和自动恢复仍保持 Unknown/未声明。

