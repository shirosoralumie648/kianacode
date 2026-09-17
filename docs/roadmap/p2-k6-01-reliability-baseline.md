# P2-K6-01 可靠性与对账基线

> 快照日期：2026-09-18。本文记录六类失败的 Incident/Recovery 投影与 Unknown 对账边界；运行时验收由 GitHub Actions 负责，本地不运行测试。

## Failure contract

`FailureClass` 固定覆盖 crash、timeout、cancel、disk full、MCP failure 和 provider Unknown。每个类别都通过 `RecoveryPlan` 给出有界的人工检查步骤；计划显式区分已知失败与不确定效果，并且 `automatic_retry_allowed` 永远为 `false`。

ControlPlane 从已提交的 `run.failed`、`run.result_unknown`、`run.cancelled`、`capability.failed` 和 `capability.result_unknown` 事实重建 `FailureIncident`。错误码、效果/停止证据和错误摘要只用于保守分类；缺少本机 continuation 的 run 也按 crash/Unknown 处理，不把缺失事实当作已停止或可重试。

## Reconciliation boundary

`failure.incidents` 返回带 source event、run、class、recovery 和 reconciliation 状态的稳定投影；需要对账的 incident 进入 Human Inbox 的 reconciliation 项。`failure.reconcile` 要求唯一 incident、受保护 EventLog 证据引用、显式 resolution、revision/CAS 和幂等键，追加 `failure.reconciled`，但不改写原始运行结果，也不触发模型、Provider 或 Broker。

资源释放继续由 `failure.release` 单独控制：必须先存在对账事实，再确认所有 execution stop，校验 resource-quarantine revision，才追加 `resource.released`。释放响应明确 `automatic_retry_allowed=false` 和 `new_request_required=true`；未知效果不会被自动 retry 或静默释放。

## CI-only 验收

`every_failure_class_has_an_incident_and_recovery` 在 domain fixture 中逐一验证六类恢复计划，在 core source guard 中验证事件投影、Human Inbox 对账入口、证据/CAS/停止确认和无第二执行循环边界。OA-19 incident projection 与 ER-10 resource projection 作为回归来源。

```text
cargo fmt --all --check
cargo test -p kiana-domain --test p2_k6_01_reliability --locked -- --test-threads=1
cargo test -p kiana-core --test p2_k6_01_reliability --locked -- --test-threads=1
cargo test -p kiana-core --test oa19_incident_projection --locked -- --test-threads=1
cargo test -p kiana-core --test er10_resource_projection --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- Incident/Recovery 当前是从 EventLog 与平台流重建的受控 projection，不是独立的跨进程 IncidentStore 或外部告警系统。
- reconciliation queue 由 `failure.incidents`/Human Inbox 的未解决投影表达；没有后台自动 worker、自动 retry 或外部 provider query，真实外部效果仍需人工/连接器 receipt 证据。
- 本切片不声称 power-loss durability、跨进程 stop 物理证明、外部业务 outcome、live provider/MCP 或 physical release；这些边界仍由 ER/PD/DEP/INT/SC 专项负责。
