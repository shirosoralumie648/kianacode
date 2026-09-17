# P1-C-02 Cell 生命周期与 retire 基线

> 快照日期：2026-09-18。本页回填 Cell reserve→commit→terminal→retire 与资源释放；本地不运行测试，验收夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`P1-C-02`](../roadmap.md#step-p1-c-02) |
| feature_status | `implemented`（core/domain CompanyOS cell source；CI-only lifecycle guards） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | MemoryCellRegistry keeps one process-local reservation state; ControlPlane/Company EventLog records lifecycle projections |
| this step does | reserve validates template/grant/budget/supervision/parent/path/fingerprint, commit advances plan and Cell, terminal paths transition deterministically, and retire releases only the owning grant/budget/path locks once after active capabilities drain |
| this step does not | 不把进程内 mutex/CAS 当 durable projector，不跨 Cell 释放资源，不在 active capability 未结束时 retire；跨进程 recovery、scheduler/Swarm and durable resource facts remain later steps |

## 1. Contract

Admission stores the complete reservation before a Cell can run. `commit_spawn` moves a validated
reservation to ready, the ControlPlane drives it to running/terminal, and `retire_cell` only accepts
terminal-compatible states. `release_resources` is idempotent, returns the reserved budget, removes
path locks only when still owned by that Cell, and records the grant/supervision/budget identity in
the retirement receipt.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `retire_revokes_grants_and_releases_budget` | reserve/commit/terminal/retire hooks and budget/path release are present, with active capability fence |
| `cell_retirement_releases_only_its_own_resources_once` | release checks owner identity, retains resource IDs and rejects repeated release |
| `failed_cell_reservation_releases_builder_path_lock` | existing core runtime regression confirms a failed admission does not strand its path lock |

## 3. Proof ceiling and handoff

P1-C-02 proof ceiling is `source` plus CI fixtures: deterministic lifecycle and owned resource release
are explicit. Durable Cell/Grant/Budget/Lease projector, restart recovery, scheduler/Swarm claim and
cross-process fencing remain AUT/SW/ER/PD/CP work.
