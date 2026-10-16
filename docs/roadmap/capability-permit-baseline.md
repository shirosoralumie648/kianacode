# CAP-05 可核验许可与单次 dispatch 基线

> 快照日期：2026-09-17。本页记录 Broker 在 handler 前核验 DispatchPermit 的 source/epoch/CAS
> 边界；Permit 本身不是事实源，EventStore 的 committed transition 才是 authority。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CAP-05`](capability.md#step-cap-05) |
| feature_status | `partial`（permit verifier read-set recheck + single-consume fence；pre-handler execution facts 已并入 consume CAS，等待远端复核） |
| proof_level | `source`；本地不运行测试，`.github/workflows/cap05-permit.yml` 负责 fixtures 与 workspace test-target compile |
| authority | `JournalPermitVerifier` + EventStore `execution.prepared`/`invocation.dispatching`/`invocation.executing` CAS |
| this step does | non-empty opaque `permit:` identity、strict permit/request/project/expiry validation、authority dependency epoch recheck、single consumption and handler-after-commit ordering |
| this step does not | 不把任意 authorization string 当 permit，不使用进程内 HashSet 记消费状态，不在 permit 未确认时调用 handler，不声明外部 exactly-once |

## 1. Contract

Broker 只接受 `permit:<ExecutionId>`；空或无前缀授权立即返回 `execution_permit_required`。
`execution.prepared` 必须是 stream version 1 的唯一记录；permit body/id、prepared
`InvocationIdentity` 与 authorized request 必须完全匹配，且 `DispatchPermit::validate_for_request` 通过；每个
`authority_versions` dependency 在消费前重新读取并要求当前 stream version 精确相等，漂移返回
`old_epoch_permit_rejected`。然后以 expected execution-permit/authority versions 的原子
`commit_confirmed` 同批写连续的 `invocation.dispatching` 和 `invocation.executing` 事件；只有
CAS 确认首次提交，Broker 才进入 handler。`invocation.executing` 是 handler-boundary/effect
事实，不得在 permit verifier 前写入，也不能由 verifier 当成 prepared permit 接受。之后 stream
再有 dispatching/executing/result 记录的重放只会返回 `execution_permit_already_consumed` 或结构化
conflict；unknown commit 不进入 handler，不声称其事实已提交。

若 dispatch fact CAS 返回 Unknown/失败，Broker 不调用 handler，调用方保留
`result_unknown`/fence；只有 committed permit consumption 后才进入现有 ControlPlane→Broker
handler 路径。没有新事件存储、第二执行循环或权限并集。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `concurrent_dispatch_consumes_one_permit` | 两个并发 verifier 只有一个可消费同一 permit |
| `preexisting_executing_fact_is_rejected_without_dispatching_or_effect` | 只有 `execution.prepared` 后出现伪造 executing 事实时，consume fail-closed，不补写 dispatching |
| `request_drift_does_not_consume_execution_permit` | 不匹配 prepared request 在读取/校验阶段失败，不推进 stream |
| `prepared_identity_header_drift_does_not_consume_execution_permit` | prepared 顶层 action digest 与 permit 不一致时拒绝消费，stream 保持 prepared-only |
| `opaque_or_empty_authorization_never_reaches_dispatch` | 任意非 permit/空 permit 标识 fail-closed |
| `cap05_dispatch_has_no_authorization_or_epoch_bypass` | authority epoch recheck、commit-before-handler 和 no HashSet/source guard |
| `execution_prepared_contract_requires_server_identity_envelope` | EventKindSpec 接受 run/turn/invocation/execution/request/action/attempt envelope 与可选 cell reservation，并要求非空执行身份字段 |

## 3. 2026-10-02 atomic start correction

Run `37025517103` / CM-02 job `110899632784` failed
`model_written_memory_without_evidence_is_rejected_and_stays_unsearchable` at
`kiana-daemon/tests/daemon_host.rs:3781` with
`result_unknown:port_failed:execution_permit_already_consumed`. `dispatch_authorized` had committed
`invocation.executing` version 2 before entering Broker, then the first
`JournalPermitVerifier::verify_and_consume` required the stream to contain only
`execution.prepared` and rejected its own valid pre-handler event. The verifier now validates the
single prepared record, exact permit/id/request/project/expiry, prepared invocation identity and
authority read set, then atomically CAS-writes `invocation.dispatching` v2 plus
`invocation.executing` v3. A duplicate/replayed/unknown CAS cannot reach the handler; no standalone
pre-Broker effect-start event remains. The CAP-05 fixture and H13 projection cassette encode this
order. Post-fix GitHub receipt is pending; CAP-05 remains `partial` / `source` and no complete CM-02
behavior is claimed.

## 4. Proof ceiling and handoff

CAP-05 proof ceiling 为 `source`：MemoryEventLog CI fixture 固化 permit identity、read-set/CAS
和单次消费边界，未运行本地测试。真实 JSONL power-loss、跨进程 contention、OS effect spawn、
provider/connector exactly-once 和 full approval resume 仍留待 CAP-06/12+、CP/ER/PD/INT。

## 5. 2026-10-04 Broker effect-boundary fixtures

The focused lane now uses the real `CapabilityBroker`, static action catalog, real
`JournalPermitVerifier` and the same `MemoryEventLog` for both authority and handler evidence.
The handler increments its counter only after observing the committed execution receipt and
`dispatching`/`executing` facts. Added fixtures cover unknown/empty execution IDs, legal one-shot
execution and replay, two contenders at the actual CAS boundary, stale authority and malformed
read sets, failed/unknown/TOCTOU commit refusal, and cancellation before consumption. Every
refusal asserts zero handler calls and a prepared-only execution stream.

GitHub CI run `37164052536` proves the earlier resolver slice, while the CAP-05 changed-source
workflow is dispatched after this commit. This is still `feature_status=partial` and
`proof_level=source`: ControlPlane's full prepare/approval/budget/lease transaction, durable
power-loss recovery, external effect reconciliation and physical/live proof remain open.

## 6. Current-head focused local permit/effect lane (2026-10-05)

```text
source_snapshot: current dirty worktree; kiana-core/src/dispatch.rs; kiana-capability-broker/src/lib.rs; kiana-core/tests/cap05_permit.rs
worktree_status: CAP-05 remains roadmap row 143 🔄; real Broker/JournalPermitVerifier CAS path is exercised locally; no automatic commit/push/merge/rebase
command_argv: bounded serial `cargo test -p kiana-core --test cap05_permit --locked --offline -- --test-threads=1` with `CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 RUST_MIN_STACK=33554432 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_STRIP=symbols CARGO_INCREMENTAL=0`, wrapped by `/usr/bin/time -v -o /tmp/kiana-roadmap-20261005-evidence/cap05-local.resources`
cwd·environment: repository root Linux/bash; one Cargo/test worker; 32 MiB test stack; stripped, non-incremental test profile; 2.26s elapsed, 1.07s test time, max RSS 357436 KiB (~349.1 MiB), 0 swaps
fixture·cassette: `cap05_permit` 12/12; opaque/empty authorization, one-shot and concurrent CAS consume, stale authority/read-set/request/identity drift, failed/Unknown/TOCTOU commit, cancellation and preexisting executing deny; handler/effect calls remain zero on refusal
exit_code: 0
status_change: focused Broker permit/effect behavior recorded as `local_behavior`; whole card remains `feature_status=partial`, `proof_level=source`
proof-level_change: focused deny-first and single-consume selectors `source` → `local_behavior`; no whole-card durable/live/physical promotion
limitations: ControlPlane complete prepare/approval/budget/lease transaction, post-prepare crash/power-loss/cross-process recovery, external exactly-once, provider/connector effects, Secret/egress and physical/live proof remain open
reviewer: integration agent with read-only CAP-05 audit; focused local Broker/permit reviewer
```

## 7. Prepared-event owner binding (2026-10-08)

`JournalPermitVerifier::verify_and_consume` now requires the top-level `request_id` of the
`execution.prepared` fact to equal the server-derived `execution.prepare` request ID for the
permit's capability request. A valid payload under a foreign event owner is rejected before the
consume CAS, so it cannot reach a handler or leave a partially consumed permit.

```text
source_snapshot: current dirty worktree; kiana-core/src/dispatch.rs; kiana-core/tests/cap05_permit.rs
worktree_status: CAP-05 roadmap row 143 remains 🔄 / feature_status=partial / proof_level=source; verifier binds prepared top-level request_id to derived execution.prepare owner; no second dispatch path, manifest/lockfile or automatic commit/push/merge/rebase
command_argv: RED `cargo test -p kiana-core --test cap05_permit prepared_top_level_request_owner_drift_does_not_consume_execution_permit --locked --offline -- --exact --test-threads=1`; GREEN `cargo test -p kiana-core --test cap05_permit --locked --offline -- --test-threads=1`; final `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; one Cargo job and one Rust test thread; `RUST_MIN_STACK=33554432`; stripped/non-incremental test profile; RED max RSS 1197868 KiB / 85% CPU; GREEN max RSS 1198748 KiB / 90% CPU; all measured runs reported 0 swaps
fixture·cassette: RED foreign top-level request_id passed the old verifier; GREEN `cap05_permit` 13/13 covers owner drift, payload/action drift, request/authority/read-set drift, replay/CAS contention, cancellation and preexisting execution denial; foreign owner leaves the stream at one `execution.prepared` record and invokes no handler
exit_code: RED 101 expected; GREEN/fmt/diff 0
status_change: selected prepared-event owner binding moves source → local_behavior; whole CAP-05 remains partial/source
proof-level_change: local pre-handler owner denial only; no durable/live/physical promotion
limitations: no full ControlPlane prepare/approval/budget/lease transaction, crash/power-loss or cross-process recovery, external exactly-once, provider/connector effect, or physical/live proof
reviewer: root integration agent with CAP-05 read-only permit audit; focused Broker/permit reviewer
```
