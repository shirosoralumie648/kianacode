# ER-08 Run/Turn 状态投影与 terminal 约束基线

> 快照日期：2026-09-17。本页记录 run projector 的 source/CI 边界；RunState 是 EventLog
> 的只读折叠结果，不是新的状态事实或执行授权。

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-08`](event-receipt-recovery.md#step-er-08) |
| feature_status | `implemented`（RunPhase/RunOutcome terminal reducer 与 replay guard） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | EventLog run/turn facts；`project_run_state` 只读投影 |
| this step does | authorized/queued/running/awaiting-approval/cancelling/terminal phases、new prompt turn reset、same terminal replay idempotence、conflicting terminal fail-closed、post-terminal event suppression |
| this step does not | 不自动 resume、执行 Broker/Runner、修改 EventLog 或把 projection/cache 当 authority；Invocation/Approval/RunSnapshot durable projector 继续由 ER-09+ / CP-21 负责 |

## 1. Contract

`kiana-core::project_run_state` 先按同一 run stream version（或跨 aggregate 的 durable append
order）折叠事件，并以 event ID 去重。`run.prompt` 明确开启下一 turn，清除上一 turn 的 terminal
outcome；没有新 prompt，任何晚到 approval/effect/event 都不能复活终态。相同 terminal kind
重复回放只保留首次事实，不同 terminal kind 在同一 turn 形成 `TerminalConflict` 并 fail-closed。
审批请求、取消中和运行事件分别投影为 `AwaitingApproval`/`Cancelling`/`Running`，不触发
额外执行循环。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `run_projection_follows_approval_cancel_and_terminal_phases` | approval→cancelling→cancelled phase/outcome 按事实顺序折叠 |
| `terminal_projection_is_idempotent_and_rejects_conflicting_kinds` | 同 terminal replay 稳定；不同 terminal kind 返回 TerminalConflict |
| `late_effect_is_ignored_until_a_new_prompt_turn` | terminal 后晚到 effect 不改变状态；新 turn prompt 才清除 outcome |
| `run_projection_rejects_malformed_present_identity_links_but_accepts_legacy_absence` | self-parent/self-causation 与显式 nil parent/causation link 在筛选前拒绝；缺失 optional links 的 legacy event 仍可投影 |
| `er08_run_projection_is_terminal_and_replay_safe` | source guard 固定 terminal reducer、event ID 去重、late-event suppression 和无授权边界 |

## 3. Proof ceiling and handoff

ER-08 proof ceiling 为 `source`：RunState reducer 的 phase/terminal/replay 语义由 CI-only
fixtures 固化；未运行本地测试。Invocation/Execution/Attempt 投影、pending/lease/budget
rebuild、RunSnapshot/restart/resume 和外部/live/physical proof 留待 ER-09+、CP-18+、PD。

## 4. Run identity conflict hardening (2026-10-08)

`project_run_state` now validates aggregate and payload run identities before filtering. Both
identities may be absent for legacy non-run events, or one may be present, but when both exist they
must match. Malformed run aggregate/payload identities and mismatches return stable projection
errors instead of being silently ignored or allowing payload identity to override the stream.

```text
source_snapshot: current dirty worktree; kiana-core/src/projection.rs; kiana-core/src/persistence_read_model.rs; kiana-core/tests/er08_run_projection.rs; kiana-core/tests/er08_projection_guard.rs
worktree_status: ER-08 row 134 remains ✅ / feature_status=implemented / proof_level=source; selected identity hardening locally verified as a post-closeout correction; no new execution path
command_argv: RED `cargo test -p kiana-core --test er08_run_projection run_projection_rejects_aggregate_and_payload_identity_conflicts --locked --offline -- --exact --test-threads=1`; GREEN `cargo test -p kiana-core --test er08_run_projection --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test er08_projection_guard --locked --offline -- --test-threads=1`; regression `cargo test -p kiana-core --test pd10_read_model --test er16_terminal_uniqueness --test h02_lifecycle --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo job/test thread, `RUST_MIN_STACK=33554432`, stripped/non-incremental test profile; RED max RSS 324032 KiB / 95% CPU; GREEN max RSS 1198536 KiB / 96% CPU; guard max RSS 129844 KiB / 62% CPU; regression max RSS 339984 KiB / 122% CPU; all measured runs reported 0 swaps
fixture·cassette: RED foreign aggregate/payload drift was accepted or silently filtered; GREEN ER-08 4/4, Core guard 1/1 and read-model/terminal/lifecycle regression 13/13; stable `run_event_run_id_conflict` and `run_event_run_id_invalid` mappings are fixed in projection and persistence read-model paths
exit_code: RED 101 expected; GREEN and regression commands 0
status_change: selected ER-08 run identity consistency moves source → local_behavior; row remains ✅ / implemented/source and no proof ceiling promotion
proof-level_change: direct projection identity denial source → local_behavior; no durable/live/physical promotion
limitations: full EventLog projector runner, checkpoint/restart/resume, cross-process recovery, external/live/physical proof and all upstream ingress identity guarantees remain open
reviewer: root integration agent with ER-08 read-only projection audit; focused identity reviewer
```

## 5. Current-head malformed identity-link receipt (2026-10-08)

The direct `project_run_state` read path now validates every input `RuntimeEvent` before run
identity filtering. Self-parent, self-causation, and explicitly supplied nil parent/causation IDs
return the structured `IdentityLinksInvalid` projection error and stable read-model/Core mappings.
Events that omit these optional links remain compatible with the legacy reader boundary.

```text
source_snapshot: current dirty worktree; kiana-core/src/projection.rs; kiana-core/src/persistence_read_model.rs; kiana-core/tests/er08_run_projection.rs; kiana-core/tests/er08_projection_guard.rs
worktree_status: ER-08 row 134 remains ✅ / feature_status=implemented / proof_level=source; selected malformed present identity-link denial is locally verified; no new execution path, manifest or lockfile change
command_argv: RED `cargo test -p kiana-core --test er08_run_projection run_projection_rejects_malformed_present_identity_links_but_accepts_legacy_absence --locked --offline -- --exact --test-threads=1`; GREEN `cargo test -p kiana-core --test er08_run_projection --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test er08_projection_guard --locked --offline -- --test-threads=1`; regression `cargo test -p kiana-core --test pd10_read_model --test er16_terminal_uniqueness --test h02_lifecycle --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo job/test thread, `RUST_MIN_STACK=33554432`, stripped/non-incremental test profile; RED max RSS 324332 KiB / 43% CPU; ER-08 GREEN max RSS 1197420 KiB / 85% CPU; guard max RSS 130480 KiB / 65% CPU; regression max RSS 340452 KiB / 122% CPU; all measured runs reported 0 swaps
fixture·cassette: RED old projector accepted self-parent; GREEN ER-08 5/5 covers self-parent, self-causation, nil parent/causation and legacy optional-link absence; Core guard 1/1 enforces validation-before-filtering and structured marker; pd10/er16/h02 regression 13/13; stable `run_identity_links_invalid:<reason>` and persistence `persistence_read_model_run:run_identity_links_invalid:<reason>` mappings
exit_code: RED 101 expected; GREEN/guard/regression 0
status_change: selected malformed present identity-link denial moves source → local_behavior; row remains ✅ / implemented/source
proof-level_change: direct run projector input validation source → local_behavior; no durable/live/physical promotion
limitations: EventLog append/frame boundaries and Invocation projection already have separate validation but full projector orchestration, checkpoint/restart/resume, cross-process recovery, external/live/physical proof and all upstream ingress guarantees remain open
reviewer: root integration agent with ER-08 read-only projection audit; focused malformed-link reviewer
```

## 6. Current-head EventId replay identity fence (2026-10-08)

`project_run_state` now distinguishes an exact replay from content drift. Repeating the same
`event_id` with an identical immutable `RuntimeEvent` remains idempotent; reusing it with any
different envelope, kind or payload returns `EventIdentityConflict` before the reducer folds the
second fact. The persistence read-model and `ControlPlane::run_state` paths preserve the stable
`run_event_identity_conflict:<event_id>` mapping.

```text
source_snapshot: current dirty worktree; kiana-core/src/projection.rs; kiana-core/src/persistence_read_model.rs; kiana-core/tests/er08_run_projection.rs; kiana-core/tests/er08_projection_guard.rs
worktree_status: ER-08 row 134 remains ✅ / feature_status=implemented / proof_level=source; selected EventId identity replay fence is locally verified; no EventStore mutation or second execution path
command_argv: RED `cargo test -p kiana-core --test er08_run_projection run_projection_rejects_event_id_content_drift_but_accepts_exact_replay --locked --offline -- --exact --nocapture --test-threads=1`; GREEN `cargo test -p kiana-core --test er08_run_projection --test er08_projection_guard --locked --offline -- --test-threads=1 --nocapture`
cwd·environment: repository root Linux/bash; one Cargo worker/test thread, 32 MiB test stack, stripped/non-incremental test profile; RED 1196652 KiB / 94% CPU; GREEN 1199760 KiB / 93% CPU; all runs reported 0 swaps
fixture·cassette: RED old HashSet dedup returned Ok for same-event-id payload drift; GREEN run projector 6/6 and ER-08 guard 1/1; exact replay remains accepted and drift returns `run_event_identity_conflict:<event_id>`
exit_code: RED 101 expected; GREEN 0
status_change: selected ER-08 EventId replay identity moves source → local_behavior; row remains ✅ / implemented/source
proof-level_change: direct projector replay identity source → local_behavior; no durable/live/physical promotion
limitations: legacy/mixed-stream ordering, full EventLog projector orchestration, checkpoint/restart/resume, cross-process recovery and external/live/physical proof remain open
reviewer: root integration agent with ER-02 projector audit; focused EventId replay reviewer
```

## 7. Current-head legacy append-order fence (2026-10-08)

The run projector no longer treats request-local `sequence` as an aggregate version. It sorts only
when every selected event has the same complete aggregate identity and a `stream_version`; legacy
or mixed-metadata input preserves the EventLog append order supplied to the projector. This keeps
a later legacy `run.prompt` from being reordered before an earlier terminal fact merely because its
request-local sequence is smaller.

```text
source_snapshot: current dirty worktree; kiana-core/src/projection.rs; kiana-core/tests/er08_run_projection.rs; kiana-core/tests/er08_projection_guard.rs
worktree_status: ER-08 row 134 remains ✅ / feature_status=implemented / proof_level=source; legacy/mixed stream ordering boundary is locally verified; no EventStore mutation or second execution path
command_argv: RED `cargo test -p kiana-core --test er08_run_projection legacy_run_projection_preserves_append_order_without_stream_metadata --locked --offline -- --exact --nocapture --test-threads=1`; GREEN `cargo test -p kiana-core --test er08_run_projection --test er08_projection_guard --locked --offline -- --test-threads=1 --nocapture`
cwd·environment: repository root Linux/bash; one Cargo worker/test thread, 32 MiB test stack, stripped/non-incremental test profile; RED 323596 KiB / 110% CPU; GREEN 128256 KiB / 65% CPU; all runs reported 0 swaps
fixture·cassette: RED legacy completed(seq=5) followed by prompt(seq=1) was reordered to Terminal; GREEN run projection 7/7 and Core guard 1/1 preserve append order and return Running/None
exit_code: RED 101 expected; GREEN 0
status_change: selected ER-08 legacy append-order boundary moves source → local_behavior; row remains ✅ / implemented/source
proof-level_change: direct projector ordering source → local_behavior; no durable/live/physical promotion
limitations: invocation/span/model-attempt legacy ordering, full EventLog projector orchestration, checkpoint/restart/resume, cross-process recovery and external/live/physical proof remain open
reviewer: root integration agent with ER-02 projector audit; focused legacy ordering reviewer
```
