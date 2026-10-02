# CP-13 execution permit 与 dispatch barrier 基线

> 快照日期：2026-09-17。本页记录 opaque permit、唯一 dispatch CAS 和 Broker 前屏障的
> source/CI 边界；不把 permit 提交写成外部服务 exactly-once。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CP-13`](control-plane.md#step-cp-13) |
| feature_status | `partial`（strict DispatchPermit、prepared/atomic dispatch-start CAS；本次 source correction 等待 GitHub CI 复核） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | ControlPlane EventLog transition + `ExecutionPermitVerifierPort`; Broker never mints authority; effect-start fact is committed by the verifier consume CAS |
| this step does | permit schema/version/digest/action binding, authority read-set, cancel-before-commit guard, one-time execution permit consumption, invocation dispatch boundary |
| this step does not | 不声称外部 effect exactly-once、不接完整 budget/lease/cancel/result reconciliation、不允许 forged authorization string 或 handler fallback |

## 1. Contract

`kiana-domain/src/dispatch.rs` 将 `DispatchPermit` 固化为 strict versioned DTO，绑定
execution/invocation/request/run/turn、decision/approval、server context、action digest、
project identity、authority read-set、TTL 和 permit digest；`validate_for_request` 在 Broker
入口重新核对准确 action/project/expiry，unknown/duplicate authority version 和 tamper
均 fail-closed。

`ControlPlane::dispatch_authorized` 仍先检查 cancellation、policy、business/resource
authority 和 action pin，再在一个 `execution.prepared` transition 写入 permit；
`JournalPermitVerifier` 只能读取唯一、版本为 1 的 prepared permit，并核对 permit body、
execution/request/run/turn/invocation/action/attempt 元数据、prepared invocation identity、
scope/expiry 与 authority read-set。它以同一个 execution stream CAS 连续写入
`invocation.dispatching` v2 和 `invocation.executing` v3；`TransitionBatch` 校验连续版本，
只有提交确认后 Broker 才进入 handler。重放、预先存在的 executing 事实、冲突或未知提交均
不能进入 handler。Broker 只消费 opaque `permit:<execution_id>`。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `dispatch_permit_is_opaque_and_binds_exact_action_and_expiry` | permit digest/identity/action/expiry exact binding，changed args 拒绝 |
| `dispatch_permit_rejects_tampered_digest_or_authority_read_set` | digest tamper 与 duplicate read-set fail-closed |
| `cp13_dispatch_requires_committed_opaque_permit_before_broker` | source guard 固定 prepared/dispatching/commit/cancel/Broker boundary |
| `concurrent_dispatch_consumes_one_permit` | CAP-05 fixture 断言并发首次 consume 只提交一组连续 dispatching/executing facts，重放拒绝 |
| `preexisting_executing_fact_is_rejected_without_dispatching_or_effect` | prepared 后已存在 executing 时 consume fail-closed，不能补写 dispatching 或抵达 handler |

## 3. 2026-10-03 atomic start correction

Run `37025517103` / CM-02 job `110899632784` failed
`model_written_memory_without_evidence_is_rejected_and_stays_unsearchable` at
`kiana-daemon/tests/daemon_host.rs:3781` with
`result_unknown:port_failed:execution_permit_already_consumed`. The prior source wrote
`invocation.executing` before the first Broker verifier call, so a valid prepared permit looked
consumed. The isolated correction makes verifier validation and the dispatching v2 / executing v3
facts one TransitionBatch guarded by the prepared and authority expected versions. The old
pre-Broker write is removed, and forged pre-existing executing facts are rejected. Source fixtures
and projection guards are updated; post-fix GitHub receipt is pending. CP-13 remains `partial` /
`source`, and the complete CM-02 memory path is not claimed.

## 4. Proof ceiling and handoff

CP-13 proof ceiling 为 `source`：唯一 permit/dispatching CAS 和 handler 前 execution marker 已
固定，但 permit 仍需 CP-11 budget、CP-12 lease/fence、CP-14 result settlement、CP-15 cancel、
CP-16 handler stop 与 CP-20 Unknown reconciliation 的完整事务接线；跨进程 crash、外部/live/
physical effect proof 未宣称。
