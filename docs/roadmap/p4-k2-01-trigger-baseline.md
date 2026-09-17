# P4-K2-01 Trigger 与调度基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Trigger boundary

`TriggerDefinition` 绑定 immutable workflow definition/version、owner/role、inputs、approval
reference、expiry、max firings、missed-schedule policy 和 Reject/Queue/Replace/Coalesce
concurrency。`RegisterTrigger` 只能在 owner/role、已批准 evidence、definition input schema、
expiry/firing bounds 通过后写入 Automation aggregate；Disable/Fire/Tick 重新检查 owner、role、
enabled、expiry、occurrence key 和 schedule evidence，重复 firing key 不重复创建实例。

`Fire`/`Tick` 的纯 planner 只创建带 `trigger_id` 的 `WorkflowInstance`，不返回
`WorkflowEffect::Dispatch`，也不接受公开 Capability 参数。之后的 `Advance` 根据冻结的
WorkflowNode 返回 effect；`ControlPlane::handle_workflow_command` 先 `commit_workflow` 再沿
Company `AgentTask` 或 `authorize_and_execute` 路径派发。触发器不持有 capability permit，
不能绕过 policy/gate/approval/receipt/reconciliation。

`trigger_cannot_execute_a_capability_directly` 验证 Fire 无 effect、并发 Reject，Advance 才
返回 capability dispatch effect；core guard 固定 CAS/idempotency、owner/approval/expiry、
RecordObservation 与无第二执行循环边界。

## CI-only 验收

```text
cargo fmt --all --check
cargo test -p kiana-workflow --test p4_k2_01_trigger --locked -- --test-threads=1
cargo test -p kiana-core --test p4_k2_01_trigger --locked -- --test-threads=1
cargo test -p kiana-core --test automation_baseline --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行格式、workspace test-target 静态编译和 `git diff --check`；不执行测试，也不等待
GitHub CI。

## 限制与交接

- 当前 scheduler/trigger aggregate 使用本地 EventLog 与 inline ControlPlane effect；不声称
  durable worker queue、跨进程 timer/lease、power-loss recovery 或生产调度公平性。
- Trigger 只创建 workflow/run，不直接执行 capability；实际 effect 的 provider、connector、
  external/live/physical 结果仍需各自 receipt/对账和后续 AUT/ER/PD/INT/DEP/SC 证据。
- 旧 `watch_scheduled_tasks` 兼容面仍被标记为 legacy，不成为第二调度器或权限根。
