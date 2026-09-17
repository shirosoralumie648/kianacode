# H07 Harness Budget 基线

> 快照日期：2026-09-17。本页记录 Harness 预算配置、预留和任务链累计边界；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H07`](harness.md#step-h07) |
| feature_status | `implemented`（配置 + per-task ledger + CP-11 admission 接线） |
| proof_level | `source`；本地仅做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | ControlPlane/ModelAssignment 与 CP-11 `ModelBudgetPort` 仍是授权和 durable budget authority；runner ledger 只做提前拒绝与累计视图 |
| this step does | model steps、attempts、tool calls、repair、compaction、task wall-time、tokens 分账；预留→结算；unknown usage 保守占用；Continue/同项目角色新 Run 不清空进程内 task scope |
| this step does not | 不把进程内 ledger 当 durable projector，不宣称 provider tokenizer/账单、跨进程并发合并、自动退款或 live/physical proof |

## 1. Contract

`HarnessBudgetConfig` 由构造参数和环境解析器统一校验，零值、非数字和超出边界的值在模型调用
前 fail-closed。环境覆盖优先于角色快照，角色步数再与 Start 命令和 authority runtime budget
取交集；来源写入 `harness_budget` model-turn metadata。CP-11 的 `reserve_prepared`/
`settle` 仍在 provider 前后执行，runner 不另建授权通路。

`BudgetLedger` 以 `project_root + role_id` 作为 task scope。每次 provider attempt 先以完整
`TokenBudget.total` 预留，再按已报告 usage 结算；缺 usage 以预留上限计费并增加
`unknown_attempts`，报告超过预留或计数/加法溢出直接拒绝。Continue 只重置 `ActiveRun` 的
turn 步数，ledger scope 不重置；不同项目/角色的 run 账本隔离。工具、`Rejected` 修复和上下文
压缩分别消耗对应额度。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `continue_cannot_reset_task_chain_total_budget` | 同项目/角色 Continue 命中累计 attempt 上限，第二次不访问模型 |
| `provider_retries_consume_attempt_budget` | `BeforeSend` provider retry 每次占用 attempt，达到上限后不发第三次请求 |
| `invalid_effective_budget_never_calls_model` | 构造非法 token budget 在模型前失败，调用计数为零 |
| `role_limits_reach_two_product_runs_without_leakage` | 不同项目 scope 的同角色预算互不污染，两个 run 均可完成 |
| `h07_budget_is_shared_and_reserved_before_effects` | source guard 固定分类预算、预留/结算顺序、CP-11 接线和 unknown 边界 |

## 3. Proof ceiling and handoff

H07 proof ceiling 为 `source`：runner 账本与配置链已接入，ModelTurn 带受限 accounting 摘要，
CI-only fixtures/source guard 固化拒绝路径。进程重启后的 task ledger 重建、跨进程/并行 sibling
原子合并、Company project cumulative projector、provider tokenizer/账单、人工等待 TTL、真实
network retry 和 live/physical proof 仍留待 H08+ / CP-11/12/15–17 / P4 / PD/INT。
