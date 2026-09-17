# P1-K5-01 成本与容量账本基线

> 快照日期：2026-09-18。本文记录当前运行时用量、预算和容量对象的边界；运行时验收由 GitHub Actions 负责，本地不运行测试。

## 三类账本与预算边界

| 对象 | 权威含义 | 当前接线 | 明确不代表 |
|---|---|---|---|
| `UsageRecord` | 一个 committed model/effect attempt 的 provider/model、token、耗时和 run/step 关联 | `receipts.rs::cost_ledger_from_events` 从 `run.model_turn` 生成；`ModelAttemptRecord` 同时保留 usage completeness/retry 状态 | 缺失 usage 不是零；不是 provider invoice |
| `CostLedger` | 多个 UsageRecord 的确定性 token 汇总 | 输入/输出 token 只有在所有记录已知时汇总，`cost_micros` 无价格来源时保持 `None` | 不是 RuntimeBudget、ProjectBudget、付款或质量结论 |
| `RuntimeBudget` | 单个 run/model harness 的最大 calls/tokens/wall time | ControlPlane authority 与 `JournalModelBudget` 在 provider 前 reserve、执行后 settle；Runner task-chain budget 另有 bounded counters | 不是项目周期预算或已花费成本 |
| `ProjectBudget` | 项目周期 max runs/tokens | Company `CompanyBudgetPolicy` 校验 project scope，并在 packet/run admission 检查 | 不是单次 run 剩余额度或 provider 账单 |
| `Quota` | scope 内 calls/tokens/concurrency 容量窗口 | Company project admission 组合检查 | 不是 CostLedger，也不授予 capability |

`CompanyBudgetPolicy { project, runtime, quota }` 明确保留三种类型；runtime admission 只读取 `RuntimeBudget`，project/run 数量只读取 `ProjectBudget`，容量并发只读取 `Quota`。`BudgetReservationFact`/`BudgetSettlementFact` 以 `BudgetScope`、authority epoch、expected version、lease 和 usage-known 绑定 reservation/settlement，不创建第二成本账本。

## 先拒绝再成功

- 空/越界 RuntimeBudget、项目 scope 不匹配、ProjectBudget/Quota 超额、child reservation 无 parent、authority/lease/version 漂移均 fail-closed，不能到达 provider/Broker。
- 已知 usage 按保守 token 结算；缺失或 Unknown usage 不退费、不伪造零成本，并在 Receipt aggregation/ModelAttempt 中保持 unknown/partial。
- Runner 的 attempts/tools/repairs/compactions/tokens/wall-time counters 与 Company ProjectBudget 分开；Continue 不清零 task-chain budget，provider retry 消耗 attempt budget。

## CI-only 验收

`runtime_and_project_budgets_are_not_interchangeable` domain fixture 验证 UsageRecord/CostLedger unknown cost 与 Runtime/Project/Quota 独立检查；core source guard 验证 provider 前 reservation、settlement、Company policy scope 和 receipt aggregation 的单一路径。CP-11 与 H-07 既有 fixtures 作为回归。

```text
cargo fmt --all --check
cargo test -p kiana-domain --test p1_k5_01_cost_capacity --locked -- --test-threads=1
cargo test -p kiana-core --test p1_k5_01_cost_capacity_guard --locked -- --test-threads=1
cargo test -p kiana-domain --test cp11_budget_contracts --locked -- --test-threads=1
cargo test -p kiana-runner --test h07_budget --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- 当前成本价格、provider invoice、跨 provider currency/rate card、refund/correction 和 durable quota projector 尚未实现；`cost_micros` 保持 Unknown，不进入 FinancialBudget 或 quality gate。
- 部分 reservation/settlement 与 EventLog/Cell/permit 仍由分步适配器组合，跨进程原子扣费、capacity backend、持久窗口和 crash reconciliation 留给 CP/BQ/PD/ER。
- `UsageRecord` 是 runtime observation，不能证明外部 provider 已计费；Receipt 是重建投影，不能替代项目达成、付款或现实 outcome。

