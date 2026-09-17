# P0-J1-01 统一 cancellation token 与状态词表基线

> 快照日期：2026-09-18。本页回填当前 cancellation fact/signal/state 实现；本地不运行测试，验收夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`P0-J1-01`](../roadmap.md#step-p0-j1-01) |
| feature_status | `implemented`（domain/core/runner source；CI-only state/guard fixtures） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | RunCancellationFact/EventLog is the durable intent/state authority; core watch and Runner cancellation signal are adapters, not independent terminal facts |
| this step does | stable `ExecutionStatus::Queued/Cancelling`, explicit cancellation transition table and terminal fence; current CP15 names `Active→Requested→Stopping→Cancelled/ResultUnknown` are the compatibility representation of accepted/cancelling/terminal phases and preserve stop confirmation semantics |
| this step does not | 不把协作式 signal 当作物理停止、不自动重开 terminal、不创建取消专属执行循环；queued drain/process-group confirmation and mid-stream race remain P0-J1-02/03/04 |

## 1. Contract

Cancellation is an intent plus a monotonic state fact. `Active` can become `Requested`, then
`Stopping`; only a confirmed stop can become `Cancelled`, while uncertain stop remains
`ResultUnknown`. `ExecutionStatus` exposes `accepted/queued/cancelling` before terminal outcomes and
rejects reopening `Cancelled` or `ResultUnknown`. Core persists the fact before signaling the
Runner, and late results are fenced by the terminal projector.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `cancel_transitions_are_total_and_irreversible` | cancellation and execution status transitions are explicit, total over supported paths and terminal states cannot reopen |
| `cancellation_status_wire_names_are_stable` | queued/cancelling and cancellation fact wire names remain stable |
| `cancellation_uses_one_fact_and_signal_chain` | core/domain/runner use the EventLog fact plus signal adapters without a second cancel loop |
| `cancellation_terminals_are_not_reopened_by_late_results` | terminal projector/finalizer retains cancellation/unknown fence for late results |

## 3. Proof ceiling and handoff

P0-J1-01 proof ceiling is `source`: status vocabulary, transition table, durable cancellation fact,
signal ordering and terminal fence are explicit. Queued tool drain, OS process-group stop proof,
mid-stream race and cross-process recovery remain P0-J1-02/03/04, CP/PD/INT work.
