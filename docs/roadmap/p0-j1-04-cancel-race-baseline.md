# P0-J1-04 取消竞态负向证据基线

> 快照日期：2026-09-18。本页回填中流取消与 late-result fence；本地不运行测试，验收夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`P0-J1-04`](../roadmap.md#step-p0-j1-04) |
| feature_status | `implemented`（daemon/core cancellation race source；CI-only race fixtures） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责运行时竞态 fixture |
| authority | Core terminal EventLog facts and the Runner cancellation signal fence late model/tool results; stream/transcript are projections |
| this step does | a cancellation racing a held model stream prevents late deltas and wrong completion, preserves exactly one terminal fact, and maps unconfirmed effect outcomes to `result_unknown` |
| this step does not | 不通过放宽断言隐藏 race、不把协作式取消升级为物理 effect rollback、不创建 race 专属执行循环；跨进程/power-loss reconciliation remains later CP/PD |

## 1. Contract

The cancel intent is persisted/signaled before the run driver can report a terminal success. The
Runner cancellation signal rejects late provider deltas and completion; core drains already emitted
events, records one terminal cancellation/Unknown fact, and ignores any subsequent success claim.
The existing daemon fixture observes the durable log, response and run stream together.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `cancel_race_never_produces_wrong_completion` | held-stream cancellation emits no late delta/completion and writes exactly one terminal fact |
| `late_runner_results_are_fenced_after_cancel` | core terminal/Unknown projection fences late Runner results after cancellation |
| `cancelling_mid_stream_never_completes_or_emits_a_late_delta` | existing daemon runtime regression remains unchanged and runs serialized in CI |

## 3. Proof ceiling and handoff

P0-J1-04 proof ceiling is `source` plus CI runtime fixtures: cancellation race fencing and terminal
uniqueness are explicit. Cross-process delivery, kernel/power-loss races, provider/external effect
reconciliation and physical proof remain CP/PD/INT work.
