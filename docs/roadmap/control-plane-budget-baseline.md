# CP-11 unified model/tool budget baseline

> 快照日期：2026-09-17。本页记录统一预算租约与模型/能力 accounting 的 source/CI 边界；不把
> 进程内账本或静态编译写成跨进程 billing 或 provider usage 证明。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CP-11`](control-plane.md#step-cp-11) |
| feature_status | `implemented`（shared BudgetLease accounting + reservation/settlement facts） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | ControlPlane/CellRegistry 与 EventLog-backed JournalModelBudget；Runner 只提交 prepared model call |
| this step does | Turn/Run/Project/Parent/Child scope enum，strict reservation/settlement digest facts，model/tool 共用 token lease，model-call count、effect、concurrency 与 unknown-usage 保守结算 |
| this step does not | 不宣称 provider tokenizer/billing、跨进程 Cell/Budget projector、并行 sibling 全局持久 CAS、自动退款、Secret/egress 或 external/live/physical proof |

## 1. Contract

`kiana-domain/src/budget_contracts.rs` 新增 `BudgetScope`、`BudgetReservationFact` 和
`BudgetSettlementFact`。reservation 绑定 run/execution/request/lease、scope、父 reservation、
model/tool/effect/token 上限、authority epoch、expected version 与 expiry；settlement 绑定同一
reservation，`usage_known=false` 时不伪造实际用量，过量、重复或未知主体均 fail-closed。

`BudgetLease` 新增 `max_model_calls`/`model_calls_used`，旧 JSON 缺失字段收紧为
`max_tool_calls` 的兼容上限；`consume_model_call` 与 `consume` 共享 `tokens_used`，因此模型和
工具不能分别绕过 token ceiling。CellRegistry 的模型 accounting 使用该方法，JournalModelBudget
把 typed reservation/settlement fact 嵌入现有 `model.reserved`/`model.settled` EventLog facts，
以 request/execution id 做幂等键并拒绝重复 settlement；provider 前先 reserve，unknown usage
按 reservation 上限保守占用。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `budget_lease_accounts_model_and_tool_consumption_together` | model call、tool call 和 token 共用同一 lease，越界拒绝 |
| `budget_reservation_and_settlement_facts_bind_scope_and_usage` | parent/child scope、known/unknown settlement、过量 usage 绑定严格 |
| `budget_contract_rejects_child_without_parent_and_zero_model_reservation` | 缺 parent、model 零 token reservation fail-closed |
| `cp11_budget_is_one_model_tool_ledger_with_conservative_unknown_usage` | source guard 固定 shared lease、pre-provider reserve、settlement/unknown boundary |

## 3. Proof ceiling and handoff

CP-11 proof ceiling 为 `source`：模型与能力已共享 domain lease 语义，事件 facts 具备 digest/幂等
边界，但 Cell/Run/Project/parent-child 全局预算尚未由 durable projector 在多进程原子合并；实际
provider usage、价格、退款、崩溃恢复和并行 sibling 证明留待 CP-13/14、ER/PD/DEP，路径 lease/fence
留待 CP-12，external/live/physical proof 未宣称。
