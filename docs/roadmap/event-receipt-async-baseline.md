# ER-06 异步写入、背压与 shutdown ack 基线

> 快照日期：2026-09-17。本页记录 EventStore async lifecycle 的 source/CI 边界；flush/close
> acknowledgement 只证明本地 adapter 已完成自身边界，不提升外部 effect 的 proof level。

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-06`](event-receipt-recovery.md#step-er-06) |
| feature_status | `implemented`（bounded worker admission、flush/health/cursor/close contract） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 eventlog fixtures |
| authority | EventStorePort + JsonlEventLog；DaemonHost 只转发存储生命周期 ack，不自产事实 |
| this step does | bounded `spawn_blocking`/Semaphore admission、structured queue/worker/close errors、durable flush and last cursor, strict health digest, close-in-progress/closed fencing, daemon/core lifecycle delegation |
| this step does not | 不保证无限队列、强制杀掉已运行 handler、跨主机/NFS durability、掉电证明、投影 checkpoint、外部 effect exactly-once 或 external/live/physical proof |

## 1. Contract

`EventStorePort` 新增 `flush`、`health`、`last_durable_cursor`、`close`。默认旧 adapter 对这些
能力显式返回 `Unavailable`，不会把 task drop 或 Memory flush 伪装成 durable。strict
`EventStoreHealth` 绑定 schema/version、writer version、healthy/durable、最后完整 logical
cursor、closed 和 digest。

`JsonlEventLog` 以有限 `MAX_STORAGE_WORKERS` semaphore 拒绝超载 producer；worker panic/queue
耗尽/关闭中的调用返回结构化错误。flush 在同一 process lock 下确认 file/parent sync 与
identity；close 先进入 closing 状态、完成该边界后转 closed，关闭后所有新读写拒绝。DaemonHost
和 ControlPlane 只通过 EventStorePort 转发 ack，不让健康状态成为授权或第二事实源。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `jsonl_flush_health_cursor_and_close_are_explicit_acknowledgements` | flush/health/last cursor 经过 durable boundary，close ack 可验证，关闭后调用拒绝 |
| `er06_eventlog_contract_is_bounded_and_observable` | source guard 固定 bounded worker、queue/worker/close error、health digest 和 core/daemon delegation |

## 3. Proof ceiling and handoff

ER-06 proof ceiling 为 `source`：async adapter 已有有限 worker admission 和显式 lifecycle
ack，CI-only fixture 负责运行时验证；未运行本地测试。慢磁盘真实压测、worker panic 注入、
shutdown 与 terminal flush 组合、projector/backup/retention 和跨进程/掉电/live/physical
证据留待 ER-07+、PD/DEP。
