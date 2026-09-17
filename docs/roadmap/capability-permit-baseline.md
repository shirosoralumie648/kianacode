# CAP-05 可核验许可与单次 dispatch 基线

> 快照日期：2026-09-17。本页记录 Broker 在 handler 前核验 DispatchPermit 的 source/epoch/CAS
> 边界；Permit 本身不是事实源，EventStore 的 committed transition 才是 authority。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CAP-05`](capability.md#step-cap-05) |
| feature_status | `implemented`（permit verifier read-set recheck + single-consume fence） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | `JournalPermitVerifier` + EventStore `execution.prepared`/`invocation.dispatching` CAS |
| this step does | non-empty opaque `permit:` identity、strict permit/request/project/expiry validation、authority dependency epoch recheck、single consumption and handler-after-commit ordering |
| this step does not | 不把任意 authorization string 当 permit，不使用进程内 HashSet 记消费状态，不在 permit 未确认时调用 handler，不声明外部 exactly-once |

## 1. Contract

Broker 只接受 `permit:<ExecutionId>`；空或无前缀授权立即返回 `execution_permit_required`。
`execution.prepared` 必须是唯一记录且 `DispatchPermit::validate_for_request` 通过；每个
`authority_versions` dependency 在消费前重新读取并要求当前 stream version 精确相等，漂移返回
`old_epoch_permit_rejected`。然后以 expected execution-permit/authority versions 的原子
`commit_confirmed` 写 `invocation.dispatching`；重放/并发消费只会返回
`execution_permit_already_consumed` 或结构化 conflict，绝不进入 handler 两次。

若 dispatch fact CAS 返回 Unknown/失败，Broker 不调用 handler，调用方保留
`result_unknown`/fence；只有 committed permit consumption 后才进入现有 ControlPlane→Broker
handler 路径。没有新事件存储、第二执行循环或权限并集。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `concurrent_dispatch_consumes_one_permit` | 两个并发 verifier 只有一个可消费同一 permit |
| `opaque_or_empty_authorization_never_reaches_dispatch` | 任意非 permit/空 permit 标识 fail-closed |
| `cap05_dispatch_has_no_authorization_or_epoch_bypass` | authority epoch recheck、commit-before-handler 和 no HashSet/source guard |

## 3. Proof ceiling and handoff

CAP-05 proof ceiling 为 `source`：MemoryEventLog CI fixture 固化 permit identity、read-set/CAS
和单次消费边界，未运行本地测试。真实 JSONL power-loss、跨进程 contention、OS effect spawn、
provider/connector exactly-once 和 full approval resume 仍留待 CAP-06/12+、CP/ER/PD/INT。
