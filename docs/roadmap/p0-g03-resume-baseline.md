# P0-G-03 `resume_run` 与协议入口基线

> 快照日期：2026-09-18。本页回填既有恢复实现；本地不运行测试，验收夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`P0-G-03`](../roadmap.md#step-p0-g-03) |
| feature_status | `implemented`（domain/protocol/client/core/daemon source；CI-only source guard） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责运行时/guard fixture |
| authority | EventLog snapshot and run stream are the recovery facts; ControlPlane owns scope, authority, data-epoch and CAS checks |
| this step does | additive `ResumeRequest` on unchanged `kiana.protocol.v1`; DaemonHost routes it to `ControlPlane::resume_run`; resume rebuilds snapshot/pending invocation, validates owner/scope/authority/data epoch, claims `run.resume_prepared`, restores the same Runner, and sends approved continuation through the existing `drive_run` |
| this step does not | 自动在启动时恢复、按 PID 复连或创建第二模型循环；没有 snapshot/材料、scope drift、stale stream 或审批条件变化均 fail-closed |

## 1. Contract

`ResumeRequest` is additive and preserves the public v1 Continue request. A resume is explicit: the
ControlPlane resolves the server-owned session/run, reads the event-derived snapshot and invocation
projection, rechecks current role/project/sandbox/authority/data epoch and claims the observed run
stream version before restoring Runner state. Approval continuations preserve original invocation
identity and later call the same `drive_run` used by normal execution.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `resume_run_reuses_the_same_drive_run_path` | protocol/daemon route reaches the existing core resume function and the approved path calls the shared drive loop without a spawned second loop |
| `resume_requires_snapshot_and_rejects_stale_scope` | missing snapshot, authority/data/scope drift and stale stream markers remain fail-closed |

## 3. Proof ceiling and handoff

P0-G-03 proof ceiling is `source`: explicit resume routing, snapshot validation, stream-version CAS,
Runner restore and shared drive path are present. Full durable cross-process Runner hydration,
power-loss reconciliation, process/approval projector and live/physical proof remain P0-F-03,
H24/H25 and PD/ER work.
