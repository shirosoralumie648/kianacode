# P2-J5-01 Workflow definition 与重放基线

> 快照日期：2026-09-18。本文记录确定性 workflow planner 与 ControlPlane 持久接线；运行时验收由 GitHub Actions 负责，本地不运行测试。

## Definition 与 planner

`kiana-domain::WorkflowDefinition` 固定 definition id/version、输入/输出 keys、allowed roles、max duration/steps、node DAG、retry policy 和 artifact dependencies。`kiana-workflow::validate_definition` 在注册前拒绝空/越界 schema、循环/缺边、递归 subworkflow、非法 capability authority、未知角色、无效 approval/signal/compensation。definition version 不可原地覆盖；升级必须使用新 key/digest。

`kiana-workflow::plan_command` 是纯 planner：给定 `AutomationState`、server-owned `AutomationAuthority`、command 和 runtime `AutomationProof`，只返回 next state 与待 ControlPlane 执行的 `WorkflowEffect`。它对 Start/Advance、Approval、WaitSignal、Pause/Resume、Cancel、Retry、Compensate 和 trigger 命令执行 owner/role/deadline/lease/evidence/attempt/parent/terminal 边界；external capability retry 需要新 reviewed contract，Unknown 不自动重跑。

## ControlPlane 接线与 replay

`kiana-core::handle_workflow_command` 先做 trust/role/schema/size/idempotency/revision 检查，从 workflow aggregate EventLog 加载并按 stream version 重放，每次使用同一 `plan_command`，再以 CAS 提交 `workflow.command_applied`。提交前没有 capability effect；只有 durable reservation 之后才把 AgentTask/Capability effect 交给现有 Company/ControlPlane→Broker→Runner 路径。重放同一 idempotency key 返回原 state，不再次 dispatch；冲突、未知 observation 和 contention 保持结构化 error/reconciliation。

取消先写 workflow state，再沿子实例向已有 Run/approval cancellation 路径传播；节点保持 unresolved 直到 Reconcile 读取 terminal fact。Compensate 创建新 workflow instance，引用原失败/取消实例和 evidence，不改写原历史或把补偿当原实例成功。

## CI-only 验收

`workflow_definition_replays_after_restart` 在 `kiana-workflow` 使用固定 authority/definition/commands 两次从空 state 规划，断言相同 revision、definition version、终态和输出；core source guard 约束 EventLog load/commit/CAS/idempotency、single spine、approval/signal/retry/compensation/Unknown 边界。

```text
cargo fmt --all --check
cargo test -p kiana-workflow --test p2_j5_01_replay --locked -- --test-threads=1
cargo test -p kiana-core --test p2_j5_01_workflow_guard --locked -- --test-threads=1
cargo test -p kiana-workflow --test state_matrix --locked -- --test-threads=1
cargo test -p kiana-core --test automation_baseline --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- workflow EventLog/aggregate 已有 durable commit/read/replay 接线，但跨进程自动 projector、power-loss reconcile、queue/claim/scheduler/ClockPort 和外部 workflow backend 尚未实现；这些由 AUT/SW/PD/ER 继续收口。
- `WorkflowEffect` 只描述待执行动作；provider/Broker/AgentTask 的真实效果、invoice、live/physical outcome 和 compensation success 仍需各自 Receipt/Unknown/reconcile 证据。
- definition version 固定不等于运行实例跨版本迁移完成；schema migration、trigger occurrence、capacity fairness 和 UI/query projections 仍是后续步骤。

