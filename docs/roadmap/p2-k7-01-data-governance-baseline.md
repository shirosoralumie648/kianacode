# P2-K7-01 数据治理与删除传播基线

> 快照日期：2026-09-18。本文记录 DataClass/Purpose/ProcessingGrant/Retention 及撤销、过期、删除的派生数据边界；运行时验收由 GitHub Actions 负责，本地不运行测试。

## Governed data contract

`DataPolicy` 由 server-owned `ProcessingGrant`、`DataClass`、`Purpose`、`Retention`、`policy_digest`、`revision` 和 `data_epoch` 组成。注册、撤销和父 grant cascade 都校验 source path/content hash、purpose/retention、revocation binding 和 digest；撤销提升 `data_epoch`，旧 scope/checkpoint/runner context 不能继续使用。

`DataGovernanceSnapshot` 从 policy 与连续、去重的 EventLog source cursor 重建每个 source 的 `Available`、`Expired`、`Revoked` 或 `Unknown` 状态，并同时投影 receipt、audit、artifact、memory、index、cache、export 七类派生 store。pending revocation/restore 先把 payload 和 derived stores 置为 Unknown；提交 `kiana.data-governance-result.v1` 后再按 revoked source 传播 Revoked，保留 source event ids 和 audit metadata 语义。

## Deletion and invalidation

daemon 的受控 `data.governance` capability 只允许 operator、无 cell 的请求。delete/revoke/expire 先以 revision/CAS 和文件 hash 校验更新 policy，并在触碰派生文件前持久化撤销事实；随后清理受 scope 的 memory JSONL、项目/用户 context cache、index、artifact-store 和 compaction 目录，source 文件删除失败返回结构化 error/`result_unknown`，不会把失败当作已删除。cache policy 明确排除 revoked source，runner snapshot 要求 fresh context，checkpoint 以 data epoch fence 旧快照。

## CI-only 验收

`deletion_propagates_to_memory_and_index` domain fixture 验证 revoke/expiry 以及七类派生 store 的状态传播；core source guard 验证事件 cursor、Unknown/Revoked/Expired、policy/data epoch、memory/cache/compaction 清理、checkpoint fence、路径/hash/CAS 和 no-second-loop 边界。OA-20 domain/core governance projections 作为回归来源。

```text
cargo fmt --all --check
cargo test -p kiana-domain --test p2_k7_01_data_governance --locked -- --test-threads=1
cargo test -p kiana-core --test p2_k7_01_data_governance --locked -- --test-threads=1
cargo test -p kiana-domain --test oa20_data_governance --locked -- --test-threads=1
cargo test -p kiana-core --test oa20_data_governance_projection --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- EventLog facts 永不由 projection 删除；当前治理 snapshot 是可重建 projection，尚未提供独立跨进程 DataGovernanceStore、retention worker 或 power-loss recovery proof。
- 本地清理覆盖受控 text memory/cache/index/artifact-store/compaction 路径；外部数据库、Provider/MCP、远程索引、备份、日志归档、生成 artifact 和现实世界复制品需要 PD/ER/DEP/INT/SC 的独立 receipt/reconciliation。
- policy revoke/expire 先拒绝未来读取和新 effect；若之后文件删除、目录遍历或 EventLog commit 不确定，状态保持 revoked/Unknown，不声称现实数据已物理清除，也不自动 retry。
