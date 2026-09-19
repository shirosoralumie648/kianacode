# H31 受控子 Agent Harness 接缝基线

> 快照日期：2026-09-19。本页记录 H31 的 source slice 与 CI-only 夹具；本地不运行测试，GitHub Actions 负责运行域、协议和 Core source guard。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H31`](harness.md#step-h31) |
| feature_status | `implemented`（domain child intent/outcome/cancel contract；existing Core SpawnPlan/CellRegistry source guard；CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| authority | ChildHarnessIntent is a narrowed input to existing SpawnPlan/CellRegistry; Core owns parent/depth/grant/budget admission and the same RunnerPort/KianaHarness route |
| this step does | typed parent/child run-turn-cell identity、budget/tool/path/sandbox intersection、depth and output contract fence；child result returns bounded summary + artifact/evidence refs without transcript; cancellation records stop_confirmed/effect_known and keeps Unknown when evidence is insufficient |
| this step does not | 不新增自由消息总线、TeamCreate/SendMessage、任意 Agent process spawn 或第二 Harness loop；真实子任务 live execution、跨进程 child projector 和外部结果确认仍留后续集成/恢复工作 |

## 1. Contract

`ChildHarnessIntent` 的 parent/child IDs、depth、budget、tools、paths、sandbox、input refs 和
output contract 是一份可验证 typed intent。`validate_against` 只允许子预算/工具/路径/沙箱
收窄，且 child depth 必须正好是 parent+1；最终仍需调用既有 SpawnPlan → CellRegistry →
ControlPlane → Runner 路径。

`ChildHarnessOutcome` 只返回 bounded summary、artifact refs 和 evidence refs，显式拒绝转发
完整 transcript，避免结果文字成为父级 system prompt。`ChildHarnessCancellation` 由 parent
cancel 传播；停止未确认或 effect 未知时强制 `result_unknown`，不宣称子任务已安全停止。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `child_scope_is_an_intersection_and_uses_typed_intent` | child intent stays inside parent budget/tool/path/depth/sandbox bounds |
| `child_cannot_expand_budget_tools_paths_or_depth` | widening and depth violations are rejected before admission |
| `child_result_returns_refs_without_transcript_and_cancel_unknown_is_explicit` | parent receives refs/summary only and uncertain stop remains Unknown |
| `child_harness_contract_reuses_spawn_and_cell_containment` | existing SpawnPlan/CellRegistry checks cover parent, depth, grant, budget and child limits |

## 3. Proof ceiling and handoff

H31 proof ceiling is `source`: typed handoff and existing Core containment path are aligned. Actual
child live scheduling, parent/child cancellation across processes, durable result projector and
external effect verification remain later H32/H33/PD/ER work.
