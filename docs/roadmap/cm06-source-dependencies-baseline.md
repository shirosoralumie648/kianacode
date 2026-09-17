# CM-06 Source dependency graph 与治理 epoch 基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Dependency graph

`SourceDependencyGraph` 是 domain-owned 的确定性 provenance index。节点覆盖 Memory、
Evidence、Event、Artifact、File、ContextPlan、Index、Summary 和 Plan；有向边采用
`derived -> dependency` 方向，所有 node/edge 先做 bounded/存在性/重复校验，再按
`from,to,relation` 排序并计算 graph digest。它不取代 EventLog、MemoryRecord 或 Artifact
内容，也不凭图边授予读取/执行权限。

`affected_by_source(source_id)` 从 source node/source_id 做反向 BFS，得到稳定排序的所有
派生节点闭包；无关 source 不会被波及。`revoke_source` 要求严格递增 `data_epoch`，返回
带 previous/new epoch、affected node IDs 和新 graph digest 的 `SourceInvalidation`。epoch
rollback、unknown/missing node、duplicate edge、self edge、字段超限和 digest drift 均
fail-closed。实际删除、过期、撤销传播由 DataGovernance/CM-05 EventStore projection 继续
承接；图查询本身不会物理擦除或自动重写事实。

## CI-only 验收

```text
cargo fmt --all --check
cargo test -p kiana-domain --test cm06_source_dependencies --locked -- --test-threads=1
cargo test -p kiana-core --test cm06_source_dependencies --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行格式、workspace test-target 静态编译和 `git diff --check`；不执行测试，也不等待
GitHub CI。

## 限制与交接

- 本步交付的是可重放 graph contract 与 epoch/invalidation projection；graph 还未作为独立
  durable EventStore aggregate 或 UI/query 命令持久化，不能声称跨进程 crash recovery 或
  自动 repair。
- DataGovernance 当前仍以 server policy 的 revoked source set 驱动清理；CM-06 API 提供
  精确依赖闭包，后续 PD/SC/CM-07+ 负责将其接到 artifact/index/cache/retention 删除作业。
- 没有 semantic recall、外部/live/physical effect 或业务结果证明。
