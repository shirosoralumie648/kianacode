# CP-13 execution permit 与 dispatch barrier 基线

> 快照日期：2026-09-17。本页记录 opaque permit、唯一 dispatch CAS 和 Broker 前屏障的
> source/CI 边界；不把 permit 提交写成外部服务 exactly-once。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CP-13`](control-plane.md#step-cp-13) |
| feature_status | `implemented`（strict DispatchPermit、prepared/dispatching CAS、Broker verifier） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | ControlPlane EventLog transition + `ExecutionPermitVerifierPort`; Broker never mints authority |
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
`JournalPermitVerifier` 只能读取已提交 permit，在同一 execution stream CAS 写
`invocation.dispatching`，重放或缺失 permit 不会进入 handler。实际 handler boundary 仍由
`invocation.executing` 记录，Broker 只消费 opaque `permit:<execution_id>`。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `dispatch_permit_is_opaque_and_binds_exact_action_and_expiry` | permit digest/identity/action/expiry exact binding，changed args 拒绝 |
| `dispatch_permit_rejects_tampered_digest_or_authority_read_set` | digest tamper 与 duplicate read-set fail-closed |
| `cp13_dispatch_requires_committed_opaque_permit_before_broker` | source guard 固定 prepared/dispatching/commit/cancel/Broker boundary |

## 3. Proof ceiling and handoff

CP-13 proof ceiling 为 `source`：唯一 permit/dispatching CAS 和 handler 前 execution marker 已
固定，但 permit 仍需 CP-11 budget、CP-12 lease/fence、CP-14 result settlement、CP-15 cancel、
CP-16 handler stop 与 CP-20 Unknown reconciliation 的完整事务接线；跨进程 crash、外部/live/
physical effect proof 未宣称。
