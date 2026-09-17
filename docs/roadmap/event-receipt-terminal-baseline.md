# ER-16 Terminal Event 唯一性与必达基线

> 快照日期：2026-09-17。本页记录 run terminal 的 source/CAS/flush 边界；它不把进程退出、
> RunStream 或客户端响应当成终态事实。终态事实仍只能来自 EventLog。

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-16`](event-receipt-recovery.md#step-er-16) |
| feature_status | `implemented`（terminal writer conflict/idempotency hardening + shutdown order） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | `ControlPlane::record_terminal_event` + EventStore atomic transition；projection 只读 |
| this step does | 固定四种 run terminal kind、稳定 `run.terminal` command/idempotency key、run-stream CAS、同 kind replay 幂等、异 kind conflict、contention/append uncertainty→`result_unknown`，以及 DaemonHost flush→observability drain→close 顺序 |
| this step does not | 不修改旧事实、不自动重跑 runner/provider、不以 terminal response 覆盖失败、不声称掉电/跨主机/physical durability |

## 1. Contract

`record_terminal_event` 在构造 batch 前拒绝非 terminal kind；同一 turn 已有同 kind 终态时返回
幂等 `false`，不同 kind 返回 `run_terminal_conflict`。所有新 terminal 仍使用稳定的
`derived_request_id("run.terminal", run+turn)`、固定 idempotency key 和 run aggregate expected
version，经 `commit_confirmed` 原子提交；CAS contention 超过重试预算不返回普通失败，而是
`result_unknown:terminal_append_unconfirmed`。

`run.result_unknown` 组合事实仍追加 `resource.quarantined`，并要求 stop evidence 才能释放；
如果 EventStore 本身不可确认，调用方必须保留 Unknown，不能报告 Completed/Cancelled。投影遇到
不同 terminal kinds 仍返回 `RunProjectionError::TerminalConflict`，late invocation/effect 不得
复活 terminal run。

DaemonHost `shutdown` 先等待 EventStore flush ack，再 drain/shutdown best-effort observability
queue，最后执行 EventStore close ack；任何 ack 失败都由调用方保留 Unknown。该入口不创建第二
shutdown loop，也不让 observability queue 成为事实源。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `duplicate_terminal_kind_is_idempotent_but_conflict_is_unknown` | 同 kind terminal 可重放，不同 kind 由纯 projection 拒绝 |
| `late_events_cannot_resurrect_terminal_run` | terminal 后到达的 invocation 不改变终态 |
| `terminal_writer_and_shutdown_are_cas_and_flush_ordered` | writer kind/conflict/Unknown/quarantine marker 与 shutdown flush 顺序 source guard |

## 3. Proof ceiling and handoff

ER-16 proof ceiling 为 `source`：终态 writer 的拒绝、幂等、CAS 和 Unknown 分类已由 CI-only
fixtures/source guard 固化，DaemonHost 提供明确 shutdown acknowledgement 顺序；未运行本地测试。
真实故障注入、掉电/跨进程 terminal durability、worker stop/reap、restart projector、resume/
reconciliation 和 live/physical proof 仍留待 ER-17+ / PD / DEP / INT。
