# P0-B-01 formal state transition baseline

> 快照日期：2026-09-16。本文记录 Approval/Capability/RunCancellation/WorkPacket/Execution/Company 状态机的 domain transition contract；本地不运行测试，运行时/guard 夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P0-B-01`](../roadmap.md#step-p0-b-01) |
| source snapshot | `ff7076b`（P4-J7-10 parent）加本步 fixtures/source guard；最终 commit 记录在 git history |
| feature_status | `implemented`（domain transition tables + core/runner usage guard source） |
| proof_level | `source`；静态编译与远程 fixtures 不提升为 local_behavior/durable/live/physical |
| authority path | domain state `can_transition_to/transition/transition_via` → ControlPlane lifecycle/dispatch/projectors → Runner state driver; result_unknown never becomes success |
| this step does | 复核现有 state transition matrices、终态谓词和 Company status macro，补专用 illegal/terminal fixture/source guard/CI；明确 intermediate compressed event path 只能走 `transition_via` |
| this step does not | 不新增第二状态机或让 UI/transcript 推断状态；不改变已存在 transition semantics，具体业务对象 edge/legacy migration 留在 CO/ER/PD cards |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain states | `kiana-domain/src/states.rs`, `kiana-domain/src/company.rs` | `46d66d1411a3bc2238f5d272e5243f736cc0bf7425011303e41aefcc282c6793`, `4cf29654e4f6e3729b47c5f65f1beab9f7daea3e244d643cc8562273d147767b` |
| Core/runner consumers | `kiana-core/src/lifecycle.rs`, `kiana-core/src/dispatch.rs`, `kiana-runner/src/state_driver.rs` | `58fd4d1b3eae52c838295f8cf9e73a743e543913f47b6445b9ae85d6117fe045`, `464b68ec143743580a201b01d26e7ca8d4f1dc542acb4116edb228a885b65cd0`, `4c468299a4ab79ed348eb42b4e1d58304375c76ed751f666eb05c7c8d596d957` |
| Fixtures and CI | `kiana-domain/tests/p0_b01_state_machine.rs`, `kiana-core/tests/p0_b01_state_machine_guard.rs`, `.github/workflows/p0-b01-state-machine.yml` | `b539a0dda20386dc562e77ef646a4fa369b453ec8f9988c779c319294bc60132`, `6e4a6c8966c472da1b7775baee4d14df22024379ffb302ddbc1de9104ea40208`, `598fc1c0fee61e53f988830c57ff181e38428bd3db9b8df50c8d9a91c4db1f59` |

## 2. Transition contract

Domain states expose explicit allowed edges and `is_terminal` predicates. Approval terminal states (Denied/Expired/Cancelled/Consumed), Capability terminal states (Denied/Succeeded/Failed/Cancelled/Unknown), RunCancellation ResultUnknown, WorkPacket Closed/Failed/Cancelled and Execution ResultUnknown cannot reopen. `CapabilityExecutionState::transition_via` allows only the documented compressed event kinds; arbitrary string event names do not widen the graph. Company object states use the same pure transition macro and reject illegal edges.

ControlPlane lifecycle/dispatch records terminal events only after domain transition or structured outcome classification. `result_unknown` is a durable uncertainty/terminal state requiring reconciliation; it cannot be mapped to Completed/Success or auto-retried by a caller. Runner state driver consumes these terminal decisions and does not define a parallel graph.

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `illegal_state_transitions_are_rejected_and_terminals_do_not_reopen` | approval/capability/cancellation/packet/execution/Company terminal reopen and illegal transitions fail |
| `result_unknown_is_terminal_and_only_explicit_paths_can_leave_intermediate_states` | Unknown terminal, explicit intermediate recovery and no implicit success |
| `core_and_runner_use_domain_state_transition_contracts` | core dispatch/lifecycle and Runner state driver consume domain transition contract |

`.github/workflows/p0-b01-state-machine.yml` 在 GitHub runner 执行 domain state fixtures、core source guard、fmt 和 domain/core/runner test-target compile；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- Formal matrix covers current domain enums but not every Company object command edge or all legacy runtime event projector paths; CO-08/ER/PD continue per-object replay and migration proof.
- A transition function/type does not prove every caller uses it; source guard is narrow and later cross-entry UAT must verify no ad-hoc state inference.
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
