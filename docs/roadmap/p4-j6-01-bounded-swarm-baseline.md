# P4-J6-01 有界 Swarm 基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Bounded fan-out contract

`SwarmPlan`/`SwarmWorkGraph` 固定 parent/project packet 集合、分区输入/输出 contract、
path/data scope、`max_concurrency`、`max_depth`、spawn rate、TTL、token/model-call budget 和
`receipt_only` merge 策略。共享 `packet_graph`/`WorkFingerprint` 校验缺失/成环依赖、重叠
写集和重复工作；同一输入不会通过改变 partition 顺序生成第二个有效执行。

`ControlPlane::handle_swarm_command` 在 `commit_swarm` 写入受保护 aggregate fact 后，才沿唯一
Company `StartRun` 路径派发 `StartChild`；Controller Cell/Grant/Supervision/预算由
`ensure_swarm_controller` 和 CellRegistry 维护。`Reconcile` 只从 Company/EventLog 观察 child，
失败/取消/`ResultUnknown` 传播并阻止盲目重试；只有所有分区都有独立 Review/evidence 的
`Merge` 才能变为 Completed，随后 parent Cell 才可 retire。事件先提交后派发的窗口仍保持
显式 `event-before-dispatch`/reconciliation 语义。

`swarm_fanout_is_bounded_and_merges_deterministically` 重复 graph projection 验证稳定 fan-in
顺序，模拟 Reserved→Ready→Running→ReadyToMerge→Completed(review) reducer，并断言重叠路径、
重复指纹和无 review 完成均拒绝；core guard 固定预算/TTL/并发、child identity、cancel/
Unknown、commit-before-dispatch、Company reuse 和 no-second-loop 边界。

## CI-only 验收

```text
cargo fmt --all --check
cargo test -p kiana-domain --test p4_j6_01_bounded_swarm --locked -- --test-threads=1
cargo test -p kiana-core --test p4_j6_01_bounded_swarm --locked -- --test-threads=1
cargo test -p kiana-domain --test sw02_work_graph --locked -- --test-threads=1
cargo test -p kiana-domain --test sw03_reducer --locked -- --test-threads=1
cargo test -p kiana-core --test sw02_work_graph_guard --locked -- --test-threads=1
cargo test -p kiana-core --test sw03_reducer_guard --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行格式、workspace test-target 静态编译和 `git diff --check`；不执行测试，也不等待
GitHub CI。

## 限制与交接

- 当前 Swarm Cell/Grant/预算与 aggregate projector 仍包含进程内 MemoryCellRegistry 和
  event-before-dispatch reconciliation 窗口；不声称跨进程 durable queue、power-loss recovery、
  实际并发 worker 或 live/physical effect。
- `MergeReceipt`/Review 证明的是受控 child evidence，不等于业务 Acceptance、Delivery 或
  Outcome；Integrator/冲突合并、完整故障矩阵和四入口 Company UX 留 SW-14..18/CO-43..48。
- Source/fixture 通过不等于 provider/网络/外部交付成功；所有 unknown 仍要求显式对账，自动
  retry/广播/第二 Agent loop 均被禁止。
