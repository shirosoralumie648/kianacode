# P0-J1-02 取消排空与 replay-safe 结果基线

> 快照日期：2026-09-18。本页回填 queued tool drain/合成结果；本地不运行测试，验收夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`P0-J1-02`](../roadmap.md#step-p0-j1-02) |
| feature_status | `implemented`（runner/core source；CI-only runner/core fixtures） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | Runner pending-tool identity and ControlPlane terminal facts remain authoritative; synthetic cancellation results never authorize or claim an external effect |
| this step does | cancel takes the queued batch, preserves the first already-dispatched boundary, emits bounded `ToolCancelled` results for remaining calls with `not_executed=true` and `replay_safe=true`, and lets core record terminal cancellation/unknown facts |
| this step does not | 不把合成结果写成真实成功、不重放已启动工具、不从取消路径执行 capability；OS process-group confirmation and mid-stream race remain P0-J1-03/04 |

## 1. Contract

For a serial tool batch, the first call may already be in the ControlPlane boundary. Cancellation
does not guess its external effect; it drains only the remaining queued siblings and labels each
synthetic result as not executed/replay safe. Runner cancellation signal prevents a late model
completion, while core maps the resulting fact to `cancelled` or `result_unknown` according to stop
evidence.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `cancel_drains_queued_tool_calls_with_replay_safe_results` | queued siblings produce synthetic bounded results and no remaining capability request is dispatched |
| `synthetic_cancel_results_cannot_become_model_success` | synthetic cancellation observations cannot enter model success/history as a real effect |
| `cancel_drains_queued_tool_calls_before_terminal_projection` | core cancellation helpers preserve ToolCancelled/not-executed/replay-safe evidence before terminal projection |

## 3. Proof ceiling and handoff

P0-J1-02 proof ceiling is `source`: serial queue drain, replay-safe synthetic results and
terminal-fact handoff are explicit. The first already-started effect, OS process-group stop
confirmation, late stream fencing and cross-process recovery remain P0-J1-03/04 and CP/PD/INT work.
