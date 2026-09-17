# P0-M1-01 Workbench 基线

> 快照日期：2026-09-18。本页回填 CLI/TTY/Web/Desktop 的 Run terminal state 一致性；本地不运行测试，验收夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`P0-M1-01`](../roadmap.md#step-p0-m1-01) |
| feature_status | `implemented`（entrypoints/daemon/protocol source；CI-only cross-surface guard） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | EventLog/ControlPlane response and DaemonHost run stream own terminal status; UI state is a disposable projection |
| this step does | Workbench/TTY, CLI stream renderer and Web ledger/SSE consume the same run-scoped `RunStreamEvent::Terminal` and `ExecutionStatus`; Desktop launches the same Web/DaemonHost surface and does not invent a second run loop; cancelled/unknown statuses remain visible |
| this step does not | 不让颜色/布局覆盖安全状态、不在 UI 缓存中生成 terminal facts、不把 `result_unknown` 画成成功；versioned UiSnapshot/action/reconnect durability remain UI-01+ / P2-M2 |

## 1. Contract

Each surface may maintain a local transcript for presentation, but terminal status is accepted only
from the server-owned response or run stream for the matching RunId. `Completed`, `Failed`,
`Cancelled`, `ResultUnknown`, `AwaitingApproval` and other non-success states remain distinct in
status lines and history. Desktop merely hosts the same Web entrypoint and worker lifecycle.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `workbench_surfaces_agree_on_terminal_state` | CLI/TTY/Web/Desktop use the same run terminal event/status authority and no UI creates a terminal fact |
| `terminal_state_rendering_does_not_hide_unknown_or_cancelled` | cancelled and unknown outcomes remain explicit in each renderer |

## 3. Proof ceiling and handoff

P0-M1-01 proof ceiling is `source`: shared terminal wire/projection and surface parity are explicit.
Durable UiSnapshot/action projector, reconnect/gap recovery, accessibility audit and physical
desktop packaging remain UI-01+ / P2-M2/P2-M5/deployment work.
