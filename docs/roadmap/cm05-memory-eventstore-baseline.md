# CM-05 Memory EventStore 唯一提交点基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Authority commit and projection

生产 `MemoryScope` 持有 DaemonHost 装配的同一个 `EventStorePort`。每次 `memory.write` 或
operator `memory.review` 先完成服务端 scope、mutation ledger、expected revision 和
idempotency 预检，再向 `memory` aggregate stream 追加 `memory.fact`（包含有界
`MemoryJournalFact`、`MemoryBodyRef`、operation 和 mutation key）。只有
`append_idempotent_expected` 接受/重放后，才向分层 `memory/*.jsonl` 写 projection row。
因此 JSONL、索引和检索都不是独立事实源；EventLog fact 未提交时不得有可见 row。

`MemoryJournalFact` 的 body ref 只包含 stream id/content hash，不暴露宿主绝对路径；fact
仍保留经校验的 bounded record snapshot，便于新进程从 committed stream 重建 projection。
EventStore 的 aggregate CAS 和 idempotency 处理重复提交、并发版本冲突以及写入结果未知；
无法确认的 append 不继续写文件，也不伪造 `effect_committed`。

## Replay and lag

`project_memory_facts` 按连续 stream version、唯一 event/idempotency key 和 body hash 重建
每个 record 的最新 JSON 值。Search/review 在 production scope 先将该 projection 与 JSONL
逐条比较：空 stream + 非空文件返回 `memory_projection_unjournaled`，已提交 stream 与文件
不一致返回 `memory_projection_lag`，不会把落后误报为空结果。torn tail、gap、重复 event、
body hash drift 和 unknown fields 均 fail-closed。写 projection 失败后，后续读取保持 lag，
等待显式修复/重建；这比自动猜测提交结果安全。

尚未具备 EventStore 绑定的兼容/单元调用使用 `MemoryScope::capture(None)`，仅用于既有纯
函数 fixture，不代表 product path 可绕过 journal。`accept_proposal` 的多目标批量物化在
production EventStore scope 下暂时返回 `memory_proposal_event_journal_required`，避免留下
未入账可见性；完整批量 TransitionBatch 留后续 CM-06/PD。

## CI-only 验收

```text
cargo fmt --all --check
cargo test -p kiana-domain --test cm05_memory_eventstore --locked -- --test-threads=1
cargo test -p kiana-daemon --test cm05_memory_eventstore --locked -- --test-threads=1
cargo test -p kiana-core --test cm05_memory_eventstore --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行格式、workspace test-target 静态编译和 `git diff --check`；不执行测试，也不等待
GitHub CI。

## 限制与交接

- 这是 EventLog→JSONL projection 的 source/fixture 接线，不声称跨进程 Memory projection
  worker、自动 repair、完整 multi-target proposal 原子物化、power-loss 实验或 durable index
  generation。
- Event fact 为 bounded record snapshot，生产敏感数据 retention/processing grant/source
  dependency/delete propagation 仍由 CM-06、PD、SC 负责；没有 semantic recall、外部/live/
  physical 业务效果证明。
