# P1-L4-01 Code intelligence 快照基线

> 快照日期：2026-09-18。本文记录当前代码/上下文查询结果的 source、snapshot 与 freshness 边界；运行时验收由 GitHub Actions 负责，本地不运行测试。

## 结果合同

`kiana-query` 的 RepoMap、ContextIndex、关键词/向量 Search、ContextPack、Artifact/Dependency graph 均从 canonicalized project root 的受控读取生成：

- 每个文件/命中/节点保留 exact content SHA-256，路径使用 root-relative portable form，语言/符号/行区间/字节和 token 估算有界；
- deterministic path collection、ignore rules、排序和 tie-break 使同一 read snapshot 输出可复现；binary/invalid UTF-8、symlink/root 外路径和超限输入不会进入结果；
- daemon 的 `render_output` 是所有 context query 的统一包装，JSON 输出附 `kiana.context-provenance.v1`、`source_snapshot`（query kind + sorted source hashes）、`source=local_workspace`、`freshness=captured_at_read`、文件数和 runtime version；text 输出也显式展示 snapshot/freshness；
- Artifact ingest/store/graph 结果同时携带 source/stored paths、content hash、schema、同步差异、dependency evidence 和 readiness，查询层不把 heuristic map 当编译器或业务事实。

## Deny-first 边界

root canonicalization、relative path containment、ignore/binary/UTF-8/size/limit 失败均在读取前拒绝或跳过；query/index/cache 只读或受控派生，不授予 grant、approval、policy 或执行权。Context provenance 是观察证据，不是 EventLog 事实；UI/transcript/cache 不能反推 source 状态。

## CI-only 验收

`code_intelligence_results_carry_snapshot_source_and_freshness` source guard 覆盖 RepoMap/Index/Search/Vector/Pack/Artifact graph content hash、canonical root、source snapshot、freshness 和 no-model/provider execution 边界。上下文/记忆既有 CI workflow 继续负责其行为回归；本步不运行 stale CM baseline test。

```text
cargo fmt --all --check
cargo test -p kiana-query --test p1_l4_01_code_intelligence --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- `source_snapshot` 是一次读取期间的 content-hash 汇总，不是 immutable workspace snapshot；扫描期间文件变化、跨进程 lock、generation/atomic index switch 和 crash recovery 仍留 CM/PD。
- RepoMap 符号与 vector embedding 是启发式/本地 deterministic hash，不能证明语义完整性、编译正确性或模型质量；provider/live semantic index 未引入。
- cache/ingest manifest 目前是派生加速/审计材料，不是 EventLog 事实；删除/撤销/retention 传播、chunk-level provenance、selected/sent/cited 语义与 durable query cursor 仍由 CM/PD/OA/SC 负责。

