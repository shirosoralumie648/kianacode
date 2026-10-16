# CP-13 execution permit 与 dispatch barrier 基线

> 快照日期：2026-09-17。本页记录 opaque permit、唯一 dispatch CAS 和 Broker 前屏障的
> source/CI 边界；不把 permit 提交写成外部服务 exactly-once。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CP-13`](control-plane.md#step-cp-13) |
| feature_status | `partial`（strict DispatchPermit、prepared/atomic dispatch-start CAS；本次 source correction 等待 GitHub CI 复核） |
| proof_level | `source`；本地不运行测试，`.github/workflows/cp13-dispatch-permit.yml` 负责 fixtures 与 workspace test-target compile |
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

## 5. Current-head focused local permit lane (2026-10-05)

当前 head 的 opaque permit、CAS 消费与 handler 前拒绝边界已在本地串行验证。该回执
不把局部 permit 行为等同于完整 ControlPlane 事务；CP-13 仍保持 `partial/source`。

```text
source_snapshot: current dirty worktree; kiana-domain/src/dispatch.rs; kiana-core/src/dispatch.rs and capabilities; CP-13/CAP-05 fixtures
worktree_status: local verification only; no second dispatch path, manifest or lockfile change
command_argv: bounded serial `cargo test -p kiana-domain --test cp13_dispatch_permit --locked --offline -- --test-threads=1`; `cargo test -p kiana-core --test cp13_dispatch_guard --locked --offline -- --test-threads=1`; `cargo test -p kiana-core --test cap05_permit --locked --offline -- --test-threads=1`; daemon regression `cargo test -p kiana-daemon --test daemon_host model_written_memory_without_evidence_is_rejected_and_stays_unsearchable --locked --offline -- --test-threads=1`; Core parity selectors run separately for `start_run_brokers_harness_tools`, `direct_capability_events_keep_request_identity_without_run_scope` and `approval_resumes_the_stored_request_once_with_monotonic_events`
cwd·environment: repository root Linux/bash; `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`, 32 MiB test stack, stripped/non-incremental profile; permit lane max RSS 241292 KiB, daemon regression max RSS 713168 KiB, parity selectors max RSS 85908 KiB, 0 swaps
fixture·cassette: domain 2/2; Core CP-13 guard 1/1; CAP-05 permit/effect deny-first 12/12; daemon memory/evidence regression 1/1; direct identity, approval-resume and Harness broker parity selectors 1/1 each; Harness fixture now uses a real temporary project root and synchronizes authority before dispatch
exit_code: all listed selectors 0; logs/resources `/tmp/kiana-roadmap-20261005-evidence/cp13-harness-parity-fixed.{log,resources}`
status_change: focused CP-13/CAP-05 behavior recorded as `local_behavior`; roadmap row 129 remains `🔄`, feature_status=`partial`, proof_level=`source`
proof-level_change: selected selectors `source` → `local_behavior`; no whole-card durable/live/physical promotion
limitations: budget/resource reservation is not atomic with execution.prepared, post-prepare helper failure can leave Prepared, cancel-after-prepare reconciliation, crash/power-loss/cross-process permit recovery, and external exactly-once remain open
reviewer: integration agent with read-only CP-13 audit; focused local permit reviewer
```

## 6. Post-prepare Unknown terminalization (2026-10-07)

The dispatch barrier now closes the permit stream when a revocation/checkpoint preparation helper
fails after `execution.prepared`, or when cancellation is observed before the Broker. The shared
`commit_prepared_result_unknown` helper appends exactly one CAS-guarded
`execution.result_committed` v2 fact with a typed receipt, `effect_known=false`, and `fenced=true`.
It does not mint `invocation.dispatching` or `invocation.executing`, because the handler has not
been admitted. This prevents a retry from seeing an orphan Prepared permit while retaining an
explicit reconciliation anchor when preparation may have partially changed state.

```text
source_snapshot: current dirty worktree; kiana-core/src/dispatch.rs; kiana-core/src/dispatch_delivery_tests.rs; kiana-core/tests/cp13_dispatch_guard.rs
worktree_status: CP-13 row 129 remains 🔄 / feature_status=partial / proof_level=source; one shared post-prepare terminal helper; no manifest/lockfile, second dispatch loop, commit/push/merge/rebase or worktree deletion
command_argv: RED source guard `cargo test -p kiana-core --test cp13_dispatch_guard --locked --offline -- --test-threads=1`; GREEN source guard with the same selector; behavior `cargo test -p kiana-core --lib dispatch::delivery_tests::prepared_pre_broker_failure_is_closed_once_as_result_unknown --locked --offline -- --exact --test-threads=1`; full delivery module `cargo test -p kiana-core --lib dispatch::delivery_tests --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo job and one Rust test thread; `RUST_MIN_STACK=33554432`; stripped/non-incremental test profile; RED source guard max RSS 4092988 KiB / 90% CPU; GREEN source guard max RSS 4099488 KiB / 93% CPU; behavior max RSS 4104888 KiB / 93% CPU; full delivery max RSS 4083436 KiB / 89% CPU; all completed runs reported 0 swaps
fixture·cassette: RED failed because the pre-fix tree had no post-prepare terminal writer; GREEN source guard 1/1 requires all three stable Unknown reasons and three helper call sites; behavior `prepared_pre_broker_failure_is_closed_once_as_result_unknown` uses the real MemoryEventLog and asserts `execution.prepared → execution.result_committed` v2, typed Unknown receipt, `effect_known=false`, `fenced=true`, cancellation metadata and idempotent replay; delivery module 13/13
exit_code: RED 101 expected; source guard 0; behavior 0; delivery module 0
status_change: selected post-prepare helper-failure/cancel terminalization moves source → local_behavior; CP-13 whole card remains partial/source
proof-level_change: durable-looking permit fact is locally CAS-tested only; no durable/live/physical promotion
limitations: the behavior fixture calls the shared terminal helper directly rather than driving every ControlPlane authority/policy setup for each failure injector; budget/resource reservation is still separate from preparation, full cancellation timing, crash/power-loss/cross-process recovery, CP-14 settlement, CP-20 reconciliation and external exactly-once are unproven
reviewer: root integration agent with CP-13 read-only audit; focused permit/delivery reviewer
```

## 7. Invocation/request derived identity fence (2026-10-08)

`DispatchPermit::validate` now rejects a permit whose `invocation_id` is not the
server-derived `InvocationId::from_uuid(request_id.as_uuid())`, even when an attacker recomputes
the outer permit digest. The check intentionally uses the permit's capability request ID and does
not compare against `RequestContext.request_id`: Harness context and capability request identity
are separate snapshots in valid flows. The domain fixture covers both forged invocation denial and
the independent-context success path.

```text
source_snapshot: current dirty worktree; kiana-domain/src/dispatch.rs; kiana-domain/tests/cp13_dispatch_permit.rs; kiana-core/tests/cp13_dispatch_guard.rs
worktree_status: CP-13 row 129 remains 🔄 / feature_status=partial / proof_level=source; only the DispatchPermit identity validator, domain fixtures and source guard changed; no manifest/lockfile, second execution path or automatic commit/push/merge/rebase
command_argv: RED `/usr/bin/time -v env CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 RUST_MIN_STACK=33554432 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_STRIP=symbols CARGO_INCREMENTAL=0 cargo test -p kiana-domain --test cp13_dispatch_permit dispatch_permit_rejects_invocation_request_mismatch_even_with_valid_digest --locked --offline -- --exact --test-threads=1`; GREEN same bounded serial environment `cargo test -p kiana-domain --test cp13_dispatch_permit --locked --offline -- --test-threads=1`; Core guard same environment `cargo test -p kiana-core --test cp13_dispatch_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo job/test thread; 32 MiB test stack; stripped, non-incremental test profile; RED max RSS 242772 KiB / 65% CPU; domain GREEN max RSS 83228 KiB / 19% CPU; Core guard max RSS 3967896 KiB / 86% CPU; all completed runs reported 0 swaps
fixture·cassette: RED forged invocation with recomputed digest was accepted by the old validator; GREEN domain 4/4 covers exact action/expiry, tamper/read-set, invocation/request mismatch and independent Harness context/request; Core source guard 1/1 requires the stable error and server-derived expression
exit_code: RED 101 expected; domain GREEN 0; Core guard 0
status_change: invocation/request identity mismatch is now locally fail-closed; CP-13 whole card remains partial/source
proof-level_change: selected DispatchPermit identity behavior source → local_behavior; no durable/live/physical promotion
limitations: the relation is enforced at DTO validation and Broker permit validation but does not complete budget/lease/fence/result/cancel/Unknown transaction wiring; cross-process crash/replay, external exactly-once, Secret/egress, live and physical proof remain open
reviewer: root integration agent with CP-13 read-only audit; focused permit identity reviewer
```

## 8. Legacy authority read-set stream-version fence (2026-10-13)

The Broker verifier no longer substitutes the request-local `sequence` field when an authority
read omits its aggregate `stream_version`. CP-28 requires re-authorization for missing authority
metadata, and ER-02 establishes that request-local sequence is not an aggregate version. A legacy
reader that drops the metadata therefore receives `old_epoch_permit_rejected` before the handler;
the prepared permit remains unconsumed.

```text
source_snapshot: current dirty worktree; kiana-core/src/dispatch.rs; kiana-core/tests/cap05_permit.rs
worktree_status: CP-13 row 129 and CAP-05 row 143 remain 🔄 / feature_status=partial / proof_level=source; no manifest/lockfile, second dispatch path, commit/push/merge/rebase or worktree deletion
command_argv: bounded serial RED exact `cargo test -p kiana-core --test cap05_permit broker_rejects_legacy_authority_read_without_stream_version --locked --offline -- --exact --test-threads=1`; GREEN same selector; full `cargo test -p kiana-core --test cap05_permit --locked --offline -- --test-threads=1`; `cargo test -p kiana-core --test cp13_dispatch_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo/compiler worker and one Rust test thread; 32 MiB test stack; stripped/non-incremental test profile; locked/offline dependencies
fixture·cassette: `LegacyAuthorityMetadataEventStore` delegates to `MemoryEventLog` and removes `stream_version` only from authority reads; RED old fallback executed the counted handler, GREEN rejects before dispatch and asserts only `execution.prepared`; CAP-05 15/15 and CP-13 guard 3/3
exit_code: RED 101 / max RSS 554888 KiB / CPU 92% / swap 0; exact GREEN 0 / max RSS 1212972 KiB / CPU 93% / swap 0; CAP-05 0 / max RSS 85300 KiB / CPU 71% / swap 0; guard 0 / max RSS 141416 KiB / CPU 58% / swap 0; all swaps 0
status_change: authority freshness verification now fails closed when an explicit stream version is unavailable
proof-level_change: selected legacy-read denial moves source → local_behavior; CP-13/CAP-05 cards remain partial/source
limitations: no migration/upcaster marker distinguishes legacy metadata loss from malformed current data; budget/lease/fence/result/cancel/Unknown transaction, cross-process crash/recovery and external/live/physical proof remain open
reviewer: root integration agent; focused permit/dispatch reviewer
```
