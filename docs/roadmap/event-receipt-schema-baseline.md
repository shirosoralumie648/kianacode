# ER-01 event schema, kind registry, and migration baseline

> 快照日期：2026-10-03。本文记录 ER-01 的 machine-readable event kind/version/migration source
> boundary；RuntimeEvent 旧 envelope 继续兼容读取，运行时 fixtures 只在 GitHub CI 执行，本地不运行测试。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-01`](event-receipt-recovery.md#step-er-01) |
| source snapshot | initial baseline `b597dfe`（CP-06 atomic transition contract 后的干净基线）；后续 source slice 按本页各证据节分别绑定 |
| feature_status | `partial`（registry/validator/unknown policy/migration source; result-field target passed on earlier snapshots; direct `capability.blocked` contract and producer guard are source-only pending fresh CI） |
| proof_level | `source`；静态编译不能提升为 local_behavior/durable/live/physical |
| canonical path | RuntimeEvent envelope → EventKindSpec/version/payload interpretation → EventLog/projector/Receipt |
| this step does | 固定 owner、aggregate、required IDs、terminal/secret policy、allowed fields、schema version 和 legacy migration；unknown opaque event 只读保留，required family unknown fail-closed |
| this step does not | 不修改旧 RuntimeEvent serde 形状，不把 unknown event 当执行权，不凭 registry 存在宣称所有 legacy payload 已迁移或 durable reader 已完成 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Event registry/RuntimeEvent/journal | `kiana-domain/src/event_contracts.rs`, `kiana-domain/src/contracts.rs`, `kiana-domain/src/states.rs`, `kiana-domain/src/journal.rs`, `kiana-domain/src/lib.rs` | `9af3362951ac13d2faf8abde6a6ea2ba38659c94b48e7a789e51d20b28508745`, `cfd802764d31a331eddfeecdd97270af146c39f868513c75a36cdd7e59212954`, `dca28dc71ef75e5dd92a25bed399349ca286bc7c5c0e514e11de20d6d1491ae3`, `4dbd656d03ec12e7821ffac254307227419423cbaf74ccb99a2b819c5eeedd48`, `080b20570e29fedcd062e53b6391d138b2db4e31c81fc91332638578dcd07c83` |
| Protocol boundary | `kiana-protocol/src/lib.rs` | `8fa329b16172f81eda38f0d4ab49f03d35f2cdb524420ecf020dc872727c1254` |
| 2026-10-03 result-event correction | `kiana-domain/src/event_contracts.rs`, `kiana-domain/tests/er01_event_contract.rs` | `e836518c996c409aa25e41540fe09a77fcf99fdb602bc2a12336d6e4bf93b264`, `ef97d89348a559e1230e23f7a6dd0d70e7c57ac1cccf43a4d23d0c04f4092029` |
| 2026-10-03 capability result source follow-up | `kiana-domain/src/event_contracts.rs`, `kiana-domain/tests/er01_event_contract.rs`, `kiana-core/tests/er01_event_contract_guard.rs` | `23e8cb4c25b482ca7160b6d40deda3a67d54214bb721fa88453b1473006d478e`, `13d7f1bcb35218dee04010ff807dd89dfc3c7d7b998f87cded9134da098b67d4`, `da0ba6a3e351d2477f0dd56be44a6dc251d9560364e83a09216d982f0cc7cb10` |
| 2026-10-03 capability result source contract | `kiana-domain/src/event_contracts.rs`, `kiana-domain/tests/er01_event_contract.rs`, `kiana-core/tests/er01_event_contract_guard.rs` | refreshed after integration |
| 2026-10-03 direct capability-blocked producer contract | `kiana-domain/src/event_contracts.rs`, `kiana-domain/tests/er01_event_contract.rs`, `kiana-core/tests/er01_event_contract_guard.rs` | `8bc31b1eff749d4da63b83f62e9af66f09b3219d6b7e64bf628f5e9090d841a6`, `d00effa1e07010ab5570cda089c1857ad6fe2588a854338b2304319a22dfcca6`, `f26ba453efe0c78d6773d46873e8c567a7f9867acb77d66e86ce71755405b30d` |
| Fixtures/core source guard/historical standalone workflow | `kiana-domain/tests/er01_event_contract.rs`, `kiana-core/tests/er01_event_contract_guard.rs`, historical `.github/workflows/er01-event-schema.yml` (removed by workflow consolidation `08552ada`) | `34e924faf84ad1cc9e2ce1c928b165416775591c73d322627d549ac300f8f68c`, `3d78c2ef8538d7b8400e89b8644d5a57fcf3c8b5baee22e3d6eaa8fd837c3645`, `e4b638fb2917a0388cb3824beb0b014d60b61178115f24a23c287826cf4d7bb9` |
| Current CI wiring | `.github/workflows/ci.yml`, `scripts/ci/test-shards.json` | current workflow matrix runs `er01_event_contract` in `kiana-domain-s2/4` and `er01_event_contract_guard` in `kiana-core-s1/6` |

hash 仅固定本步 source snapshot 的解释层边界；不代表 legacy 全量迁移或事件事实已被重写。

## 2. Registry contract

`kiana-domain/src/event_contracts.rs` 提供 `EVENT_KIND_SPECS` 和 `EventKindSpec`。每个登记的 kind 显式声明：

| 字段 | 作用 |
|---|---|
| `schema` / `version` | 当前 RuntimeEvent 解释格式；major 不兼容时拒绝 |
| `owner_crate` / `aggregate_type` | 事实 owner 与 CAS stream 归属；不能由 payload 自报替换 |
| `required_ids` | 进入对应 projector 前必须存在的 run/request/approval/invocation 关联 |
| `allowed_fields` | payload 可接受字段；未知字段不静默丢弃 |
| `terminal` | 仅供投影器解释终态，不直接授予执行权 |
| `secret_policy` | 事件写入必须经过 redaction 或拒绝；原始 secret 不在 registry payload 中 |
| `migration` | legacy reader/upcaster 名称；无确定 migration 的记录只能查询 |

当前 registry 覆盖 request/run/capability/approval/invocation/execution/action/session 关键 kind，并注册统一 `kiana.runtime-event.v1` schema。新增公开 kind 必须同时补 owner、aggregate、IDs、fields、迁移/unknown 语义和 CI fixture；不能只在 `RuntimeEvent.kind` 字符串处追加。

## 3. Unknown and version boundary

| 输入 | 处理 |
|---|---|
| 未登记且非 required family（例如 `future.opaque`） | `PreserveOpaqueWithoutExecution`：EventLog 可保留，history/projection 不猜状态，不产生授权 |
| 未登记但属于 request/run/capability/approval/invocation/execution/action/session family | `unknown_required_event_kind`，在 authority/projection 前拒绝 |
| 已登记 kind + unknown major | `event_schema_version_incompatible`，不能按旧字段猜测 |
| 已登记 payload 缺 required ID | `event_required_id_missing:<field>`，不得配对到另一个 run/sequence |
| 已登记 payload 有未知字段 | `event_payload_unknown_field`，不能 serde drop 后继续 |
| legacy v0 有确定 upcaster | 只按 `EVENT_MIGRATIONS`/spec migration 生成 v1 解释；原始事件不覆盖 |
| legacy v0 无法确定 turn/aggregate/owner | 保留 opaque/query-only，不能派发、审批或恢复 |

`validate_runtime_event` 是解释层显式 validator；为兼容旧 JSONL，RuntimeEvent 的基础构造/读取仍保留旧字段和未知 kind。EventStore/各 projector 在逐步接线前，不能把“registry 有条目”误写成“历史全部已验证”。

## 4. Core/EventLog handoff

`TransitionBatch`/JournalFrame 仍负责 command digest、aggregate read-set、CAS、frame integrity 和 all-or-none；ER-01 registry 只定义事件语义，不创建第二事实源。`RuntimeEvent.event_id`、request sequence、aggregate stream version 和 idempotency key 继续由 EventLog 所有；kind validator 不能重写已提交事实。

Receipt、history、invocation/run projector 应按以下顺序消费：

```text
read committed frame
  -> validate header/writer/version
  -> classify known/opaque/required-unknown kind
  -> validate required IDs/allowed fields/redaction boundary
  -> apply deterministic migration (if registered)
  -> fold projection; otherwise preserve Unknown/query-only
```

旧 adapter 只读兼容不代表支持新 schema 写入；writer format、migration result、projection cursor 和 receipt digest 仍需后续 ER-02..08/PD/CP 验收。

## 5. CI-only fixture catalog

| Fixture | Purpose |
|---|---|
| `event_kind_registry_is_machine_readable_and_bounded` | kind/schema/owner/aggregate/terminal/secret metadata 完整 |
| `unknown_required_kind_and_schema_downgrade_fail_closed` | opaque unknown 可查询；required family unknown 和 major downgrade 拒绝；migration lookup 确定 |
| `payload_unknown_field_is_not_silently_dropped` | required ID/allowed field 检查和 legacy opaque policy |
| `execution_prepared_contract_requires_server_identity_envelope` | execution.prepare 的身份 envelope 字段、cell reservation allowlist 与 required ID |
| `result_event_contracts_accept_only_their_result_fields` | execution.result_committed 精确增加 outcome_state/outcome_ready/result_receipt，capability terminal results 接受 result_receipt/result_source；未知字段仍拒绝 |
| `capability_blocked_contract_matches_direct_deny_producers` | direct `capability.blocked` 以 request 为 aggregate，不要求 Run ID，精确接受当前错误/attempt/effect payload，并拒绝混入 run_id；Run-bound denial 使用独立 `run.capability_blocked` |
| `approval_continuation_unavailable_contract_matches_recovery_producer` | Run-bound approval recovery 的 `approval_id`/`run_id`/bounded error 字段和 terminal 语义；缺 run ID、未知字段拒绝 |
| `capability_requested_and_result_delivery_contracts_match_producers` | `run.capability_requested` 显式保留 request_id 兼容别名并要求 capability_request_id；result delivery 只允许 producer 的五个 delivery 字段和 invocation identity |
| `run_tool_contracts_match_cancel_and_dispatch_producers` | `run.tool_call` 与 `run.tool_result` 各自使用精确字段集；缺 capability_request_id、未知 call/result 字段拒绝 |
| `run_predecessor_contract_matches_lifecycle_producer` | `run.predecessor` 的 run/previous-run/turn/session/semantics 六字段；缺 run_id、未知字段拒绝 |
| `run_resume_prepared_contract_matches_recovery_producer` | `run.resume_prepared` 的 run/session/actor/snapshot-event/turn 六字段；缺 run_id、未知字段拒绝 |
| `run_compacted_contract_matches_lifecycle_producer` | `run.compacted` 的 schema/run/tokens-before/tokens-after/summary 字段；缺 run_id、未知字段拒绝 |
| `run_receipt_contract_bounds_top_level_projection` | `run.receipt` 的 37 个固定顶层 projection 键和可选 work-packet 键；缺 run_id、未知顶层字段拒绝，嵌套值保持各自 projection contract |
| `credential_recovery_contract_matches_typed_producer` | `recovery.credential` 的 run_id/recovery 顶层 envelope；缺 run_id、未知顶层字段拒绝，nested fact 由 CredentialRecoveryFact contract 验证 |
| `communication_lifecycle_contracts_match_producers` | handoff acknowledged/rejected 的同构 lifecycle 字段与 incident escalated 的 evidence_refs 变体；缺 message_id、未知字段拒绝 |
| `communication_send_contracts_match_shared_producer` | 七类 communication send kind 的共享 message/lifecycle/authority/context 八字段；缺 message、未知字段拒绝 |
| `connector_health_and_handshake_contracts_match_daemon_producers` | connector health 的 12 字段和 MCP handshake 的 10 字段；缺 request/connector/binding ID、未知顶层字段拒绝 |
| `session_assignment_contract_matches_producer` | session assignment 的稳定 session_id 与 18 个 assignment 顶层字段；缺 session_id、未知字段拒绝 |
| `action_authority_pinned_contract_matches_dispatch_pin_and_rejects_drift` | dispatch 的 authority pin payload、action aggregate 元数据；缺 request_id/action_digest、未知字段拒绝 |
| `run_lifecycle_contracts_match_real_producers_and_reject_drift` | run.authorized/started/prompt/delta 的 per-kind 精确字段；缺 run_id、未知字段拒绝 |
| `run_input_snapshot_and_clarification_contracts_match_producers` | accepted/claimed input、snapshot、clarification 的 per-kind envelope；缺 required IDs、未知字段拒绝 |
| `event_contract_registry_and_migration_boundary_are_source_owned` | domain/contracts/states/journal/protocol source guard |

## 5.1 2026-10-03 execution result-field matrix correction

GitHub run `37038152395` / job `110941809398` (`Tests (CM-02 memory review denial)`) ran
`model_written_memory_without_evidence_is_rejected_and_stays_unsearchable` and failed at
`kiana-daemon/tests/daemon_host.rs:3781` with the response error
`result_unknown:result_event_persistence_failed`. The workflow run was later cancelled; this named
job's failure is exact, but its log exposes only the final generic error.

Source review found an independent ER-01 contract mismatch: `ControlPlane::dispatch_authorized`
emits `outcome_state`, `outcome_ready`, and `result_receipt` in `execution.result_committed`, while
that kind's allowed-field set omitted all three. Capability terminal events also carry a
`result_receipt`, so their result-only allowlist now includes that field. The fixture keeps the
three execution result fields scoped to `execution.result_committed` and proves unknown fields
remain rejected.

This contract mismatch was not the cause of the CM-02 failure. The observed generic error came from
notification-source validation misreading the capability result's business `source`; that root
cause is separately recorded in the CM-02 baseline. `append_event` does not call the EventKind
payload validator, and the `execution.result_committed` transition is a separate later write. This
slice aligned the execution/capability receipt field matrix but did not wire global runtime
enforcement.

Exact CI receipt for the execution/capability result-matrix fixture: run `37041941851`, head
`302c6b46`, `kiana-domain-s2/4` job `110954472895` passed all 5 tests in
`er01_event_contract.rs`, including `result_event_contracts_accept_only_their_result_fields`.
The workflow was later cancelled and the domain shard had unrelated failures; core guard job
`110954473184` was cancelled before a target result. The later `result_source` matrix then passed
all 5 tests in run `37048582415`, head `8407e1f1`, domain-s2 job `110976530070`; that workflow was
later failed on unrelated domain targets and was superseded; its core guard job `110976530203` was
cancelled before a target result.

## 5.2 Capability result source field

CM-02's memory writer returns its business `source` as result data. The capability event builder
now preserves that value under `result_source` so it cannot populate the notification authority
field. The ER-01 capability result matrix allows `result_source` alongside `result_receipt`, and the
fixture validates both fields on all four capability terminal kinds while continuing to reject an
unknown field. This aligns the producer and schema contract without changing notification source
validation.

The registry validator remains an explicit interpretation helper. It is not globally invoked by
EventStore append because the producer/registry matrix is not yet reconciled across all event
families. The direct `capability.blocked` row is corrected in §5.3; global enforcement still
requires separate producer-by-producer contract reconciliation and deny-first CI coverage. Do not
infer enforcement from registry membership.

Current CI uses `.github/workflows/ci.yml` with `scripts/ci/test-shards.json`: the domain fixture
runs in `kiana-domain-s2/4`, and the core guard runs in `kiana-core-s1/6`. Tests remain GitHub-only;
no local tests/build/check/clippy/smoke are run.

The exact domain target receipt above proves the `result_source` field matrix for that source
snapshot only. It passed 5/5 again in run `37054968622`, head `8d42319c`, domain-s2 job
`110997881165`; that shard failed on unrelated sibling targets. Core-s1 job `110997881116` also
failed on sibling targets, and its log contained no exact `er01_event_contract_guard` target result,
so the guard still has no successful receipt. The registry validator is not connected to generic
EventStore append; no global enforcement is inferred.

## 5.3 Direct capability-blocked producer contract

Source review found that `capability.blocked` is emitted by direct ControlPlane denial paths. One
producer writes only a redacted `error`; the post-approval guard writes `error`, `attempt`, and
effect/stop/fence facts. Neither payload has a `run_id` or `capability_request_id`, and
`aggregate_for_event` therefore assigns the request aggregate. Run-bound denials use the separate
`run.capability_blocked` kind. The registry now records that distinction, accepts exactly the two
current direct payload shapes, and no longer advertises a v0-to-v1 migration that
`event_migration` cannot resolve for this kind. A deny fixture rejects adding `run_id` to the
direct payload. The Core source guard pins both current producers. This remains source-only until
the exact domain and Core targets run in GitHub CI; it does not connect the validator to EventStore.

```text
source_snapshot: source commit `77061e0cc9cc1d48892b77a277e9540a416a1609`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-core/src/approvals.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: `capability.blocked` now describes direct request-level denial with no required Run IDs, an exact direct payload allowlist and no unresolved migration declaration; Run-bound denials remain `run.capability_blocked`; no EventStore enforcement or migration path changed
command_argv: source review of `ControlPlane::authorize_and_execute`, approval-time `guard_company_capability`, and `aggregate_for_event`; `cargo fmt --all`; `cargo fmt --all --check`; `git diff --check`; no local tests/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; GitHub Actions is the only runtime test executor
fixture·cassette: `capability_blocked_contract_matches_direct_deny_producers`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; `.github/workflows/ci.yml` runs the domain target in `kiana-domain-s2/4` and Core guard in `kiana-core-s1/6`; fresh CI receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; one existing direct-denial producer contract now matches its emitted request-level payloads
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; registry reconciliation remains incomplete for other event producers/families; old RuntimeEvent envelopes do not embed a schema version; complete historical migration, durable projection and external-effect evidence remain open
reviewer: source trace verified both direct producer payloads and request aggregate fallback; test/source guard match the emitted field matrix; no local runtime reviewer
```

## 5.4 Capability decision identity shapes

Source tracing found two existing producers for `capability.decision`: direct
`authorize_and_execute` emits policy/gate/effect facts without Run identity, while Run-bound and
approval continuation paths emit both `run_id` and `capability_request_id`. The registry's prior
required-ID pair and field set therefore described neither payload completely. The contract now
accepts exactly two shapes: both identity fields present for a Run-bound decision, or both absent
for a direct request decision. A one-sided identity is rejected with
`event_capability_identity_pair_incomplete`; `policy` and `gate` are explicit allowed fields.
The validator remains an interpretation helper and is still not wired into generic EventStore
append. The static registry keeps `aggregate_type=run` as the canonical Run-bound metadata while
the direct append path falls back to the request aggregate; this compatibility variant remains a
documented limitation of the current static spec.

```text
source_snapshot: source commit `f54747bc100a9b6b65d0ac4a115316b8eb8ea33a`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-core/src/approvals.rs`; `kiana-core/src/capabilities.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: `capability.decision` accepts direct and Run-bound identity shapes with an all-or-none identity rule; invocation fields include the actual policy/gate payload keys; no producer execution path or EventStore wiring changed
command_argv: source trace of direct, Harness and approval continuation producers; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick 2892d555`; no local tests/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; GitHub Actions is the runtime test executor
fixture·cassette: `capability_decision_contract_accepts_policy_and_gate_and_rejects_unknown_fields`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; current CI routes the domain target through `kiana-domain-s2/4` and Core source guard through `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; capability decision payloads now have an explicit dual-shape identity contract
proof-level change: none
limitations: generic EventStore append still does not call `validate_runtime_event`; producer reconciliation remains incomplete for other event families; the direct aggregate fallback is not represented by the static `aggregate_type=run` metadata; no durable/live/physical behavior is established
reviewer: source trace matched direct, Harness and approval producer fields, policy/gate allowlist and one-sided identity denial; no local runtime reviewer
```

## 5.5 Staged approval producer contract

Source tracing found `JournalApprovalStore::stage` committing an `approval.staged` event with
`approval` aggregate metadata, stream version 1, and the exact payload keys `schema`,
`approval_id`, `subject`, `state` and `at_unix_ms`. Because `approval.staged` belongs to a required
family, the missing registry entry would fail closed as an unknown kind before any future
interpretation layer could validate it. The registry now declares the existing approval migration,
requires `approval_id`, and rejects missing IDs or extra fields. No approval staging, EventStore or
ControlPlane behavior changed.

```text
source_snapshot: source commit `1274c631`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-daemon/src/journal_approvals.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: `approval.staged` is registered with approval aggregate, approval_id required, exact producer field allowlist, non-terminal state and legacy approval migration; journal producer and stream version are pinned by the Core source guard; no EventStore wiring changed
command_argv: source trace of `JournalApprovalStore::stage`; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick f20308d0`; no local tests/build/check/clippy/smoke
cwd·environment: isolated producer-audit worktree based on `c76d19b2`; integration repository root; Linux/bash; GitHub Actions is the runtime test executor
fixture·cassette: `approval_staged_contract_matches_journal_producer_and_rejects_unknown_fields`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes the domain target through `kiana-domain-s2/4` and Core guard through `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; the staged approval kind is now explicitly owned by the registry
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; other approval transitions and historical producer families still need reconciliation; this source slice does not establish approval durability, replay, recovery or external effects
reviewer: source trace matched payload keys, approval aggregate, stream version and migration lookup; no local runtime reviewer
```

## 5.6 Activated approval transition contract

The shared approval transition producer emits `approval.activated` with a distinct payload from
the staged record: schema, approval ID, previous/current state, request hash, timestamp and the
activation command ID. Reusing the broad `APPROVAL_FIELDS` would omit real fields and make the
interpretation boundary depend on unrelated transition payloads. The registry now uses an exact
activated allowlist while preserving the approval aggregate, required ID and legacy approval
migration. Missing approval IDs and unknown fields remain deny-first; no transition behavior or
EventStore wiring changed.

```text
source_snapshot: source commit `940ac2edfc5df99330eca366c762915425e73e9c`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-daemon/src/journal_approvals.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: `approval.activated` now has exact transition fields, approval aggregate metadata, approval_id requirement and explicit legacy migration; source guard pins the real transition kind, payload and stream version; no EventStore or approval behavior changed
command_argv: source trace of shared approval `transition_event`; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick 656fca79`; no local tests/build/check/clippy/smoke
cwd·environment: isolated producer-audit worktree based on `81804359`; integration repository root; Linux/bash; GitHub Actions is the runtime test executor
fixture·cassette: `approval_activated_contract_matches_transition_producer_and_rejects_unknown_fields`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes the domain target through `kiana-domain-s2/4` and Core guard through `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; activated approval transition payload is now explicitly covered by the registry
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; `approval.requested` and other approval transitions still require producer-by-producer reconciliation; no approval durability, replay/recovery or external-effect claim
reviewer: source trace matched transition kind, exact fields, approval aggregate, stream version and migration lookup; no local runtime reviewer
```

## 5.7 Requested approval producer contract

The capability approval producer emits `approval.requested` with the shared approval fields plus
the action digest used to bind the requested capability. Its stream metadata is derived from the
Run when present and otherwise from the request event, while the static registry retains approval
as the canonical aggregate owner. The registry now adds `action_digest` to this kind's exact
allowlist and keeps the approval ID/migration contract. Missing IDs and unknown fields remain
deny-first; no approval or EventStore behavior changed.

```text
source_snapshot: source commit `750cef22ee9dbe14d6d5c949ec4e8b134f32a66e`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-core/src/capabilities.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: `approval.requested` now admits the capability producer's action_digest in an exact requested allowlist, keeps approval_id required and legacy migration, and source-pins run/request aggregate branch metadata; no EventStore or approval behavior changed
command_argv: source trace of capability approval request producer; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick 55aaf5ed`; no local tests/build/check/clippy/smoke
cwd·environment: isolated producer-audit worktree based on `ca031859`; integration repository root; Linux/bash; GitHub Actions is the runtime executor
fixture·cassette: `approval_requested_contract_matches_capability_producer_and_rejects_unknown_fields`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; requested approval producer fields are now explicitly covered
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: static registry aggregate metadata remains approval while the producer's stream may be run/request; generic EventStore validation is unwired; other approval transitions still need reconciliation; no approval durability/recovery or external-effect claim
reviewer: source trace matched action_digest, required ID, migration, run/request stream branch and version increment; no local runtime reviewer
```

## 5.8 Approved approval decision contract

The approval decision transition producer emits `approval.approved` with the common transition
fields plus `decision`, `decision_command_id`, `decided_by`, and a server-owned `decision_fact`.
The registry now uses this exact ten-field allowlist rather than the broad approval field set,
while retaining approval aggregate metadata, the required approval ID and legacy migration. The
fixture rejects missing IDs and unknown fields; no approval transition or EventStore behavior
changed.

```text
source_snapshot: source commit `752400e7e60ceb018d8306cd8f94199d786398ac`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-daemon/src/journal_approvals.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: `approval.approved` now has exact decision-transition fields, approval aggregate metadata, approval_id requirement and explicit legacy migration; source guard pins the producer's decision detail and decision_fact; no EventStore or approval behavior changed
command_argv: source trace of `decide_with_proof` and shared approval transition; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick c81e4636`; no local tests/build/check/clippy/smoke
cwd·environment: isolated producer-audit worktree based on `34e31165`; integration repository root; Linux/bash; GitHub Actions is the runtime test executor
fixture·cassette: `approval_approved_contract_matches_decision_producer_and_rejects_unknown_fields`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; approved approval decision payload is now explicitly covered by the registry
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; other approval transitions still require producer reconciliation; no approval durability/replay/recovery or external-effect claim
reviewer: source trace matched decision fields, decision_fact, approval aggregate, stream version and migration lookup; no local runtime reviewer
```

## 5.9 Denied approval decision contract

`approval.denied` reuses the decision transition producer payload used by `approval.approved`,
but its registry semantics are terminal. It now has the same exact ten-field decision allowlist,
approval aggregate and required ID, with `terminal=true` and the existing legacy approval
migration. Missing IDs and unknown fields remain deny-first; no approval transition or EventStore
behavior changed.

```text
source_snapshot: source commit `e748cde265e243e93f3d857bceb2d417e45d9fb7`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-daemon/src/journal_approvals.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: `approval.denied` now has exact decision-transition fields and terminal semantics, approval aggregate metadata, approval_id requirement and explicit legacy migration; source guard pins the Denied transition kind and decision_fact producer; no EventStore or approval behavior changed
command_argv: source trace of shared approval `transition_event` and `ApprovalState::Denied`; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick e9a2abcd`; no local tests/build/check/clippy/smoke
cwd·environment: isolated producer-audit worktree based on `f64b0d81`; integration repository root; Linux/bash; GitHub Actions is the runtime test executor
fixture·cassette: `approval_denied_contract_matches_decision_producer_and_rejects_unknown_fields`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; denied approval decision payload is now explicitly covered with terminal semantics
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; expired/cancelled/consumed transitions and other approval producers still require reconciliation; no approval durability/replay/recovery or external-effect claim
reviewer: source trace matched decision fields, decision_fact, terminal flag, approval aggregate, stream version and migration lookup; no local runtime reviewer
```

## 5.10 Expired approval transition contract

The expiry path emits `approval.expired` with the common transition fields and a bounded
`reason`. The registry now uses the exact expiry allowlist, retains terminal semantics, approval
aggregate metadata, required approval ID and legacy approval migration. Missing IDs and unknown
fields remain deny-first; no expiry or EventStore behavior changed.

```text
source_snapshot: source commit `10c8ac85`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-daemon/src/journal_approvals.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: `approval.expired` now has exact expiry transition fields, terminal semantics, approval aggregate metadata, approval_id requirement and explicit legacy migration; source guard pins the expiry producer reason; no EventStore or approval behavior changed
command_argv: source trace of `require_unexpired` expiry transition; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick 87ddd93e`; no local tests/build/check/clippy/smoke
cwd·environment: isolated producer-audit worktree based on `78766317`; integration repository root; Linux/bash; GitHub Actions is the runtime test executor
fixture·cassette: `approval_expired_contract_matches_expiry_producer_and_rejects_unknown_fields`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; expired approval transition payload is now explicitly covered with terminal semantics
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; cancelled/consumed transitions and other approval producers still require reconciliation; no approval durability/replay/recovery or external-effect claim
reviewer: source trace matched expiry reason, transition fields, terminal flag, approval aggregate, stream version and migration lookup; no local runtime reviewer
```

## 5.11 Cancelled approval producer variants

Approval cancellation has two existing producers. User invalidation writes a transition payload
with `reason` and `revoked_by`; project invalidation writes `reason` and `source`. The registry now
uses one bounded union allowlist covering both variants, retains terminal semantics, approval
aggregate metadata, required approval ID and legacy approval migration. The fixture exercises both
valid forms plus missing IDs and unknown fields; no cancellation or EventStore behavior changed.

```text
source_snapshot: source commit `6f0b06a1`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-daemon/src/journal_approvals.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: `approval.cancelled` now admits exact common transition fields plus `reason` and either producer-specific `revoked_by`/`source`; terminal semantics, approval aggregate, approval_id and migration remain explicit; no EventStore or approval behavior changed
command_argv: source trace of `invalidate` and `invalidate_project`; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick 37cd8ab2`; no local tests/build/check/clippy/smoke
cwd·environment: isolated producer-audit worktree based on `a579359b`; integration repository root; Linux/bash; GitHub Actions is the runtime executor
fixture·cassette: `approval_cancelled_contract_matches_both_cancellation_producers`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; both cancellation producer payloads are now explicitly covered
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; consumed/continuation transitions and other approval producers remain open; no approval durability/replay/recovery or external-effect claim
reviewer: source trace matched both cancellation producer variants, terminal flag, approval aggregate, stream version and migration lookup; no local runtime reviewer
```

## 5.12 Consumed approval transition contract

Approval consumption emits the common transition fields plus dispatch command, decision command,
decider and a server-owned consumption fact. The registry now uses the exact consumption allowlist
and marks the kind terminal, matching `ApprovalState::Consumed::is_terminal()`. Approval aggregate,
required ID and legacy migration remain explicit; missing IDs and unknown fields stay deny-first.
No consumption or EventStore behavior changed.

```text
source_snapshot: source commit `9e3dc2ca`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-daemon/src/journal_approvals.rs`; `kiana-domain/src/states.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: `approval.consumed` now has exact consumption fields, terminal semantics aligned with ApprovalState, approval aggregate metadata, approval_id requirement and explicit legacy migration; source guard pins dispatch/decision producer fields and consumption_fact; no EventStore or approval behavior changed
command_argv: source trace of `prepare_consumption`, `ApprovalState::Consumed::is_terminal` and shared transition; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick 88a81591`; no local tests/build/check/clippy/smoke
cwd·environment: isolated producer-audit worktree based on `5d4af26d`; integration repository root; Linux/bash; GitHub Actions is the runtime executor
fixture·cassette: `approval_consumed_contract_matches_consumption_producer_and_is_terminal`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; consumed approval transition payload and terminal semantics now explicitly covered
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; continuation_unavailable and other historical approval producers remain open; no approval durability/replay/recovery or external-effect claim
reviewer: source trace matched consumption fields, terminal state definition, approval aggregate, stream version and migration lookup; no local runtime reviewer
```

## 5.13 Unavailable approval continuation contract

When a Run-bound approval cannot be resumed, the recovery path records
`approval.continuation_unavailable` with the persisted `approval_id`, `run_id` and bounded
`approval_continuation_unavailable` error. The registry now requires exactly those identifiers and
fields and marks the event terminal; the source guard pins the recovery producer. The actual
recovery append remains a Run-bound event-log write, while the static registry keeps the canonical
approval aggregate metadata, so this slice does not claim a new aggregate stream or global
EventStore validation.

```text
source_snapshot: source commit `8cba0ea6`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-core/src/approvals.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: `approval.continuation_unavailable` now requires approval_id/run_id and only the bounded error field, with terminal semantics and explicit legacy approval migration; source guard pins the persisted recovery payload; no approval recovery behavior or EventStore enforcement changed
command_argv: source trace of Run-bound approval continuation failure; isolated `cargo fmt --all --check`; isolated `git diff --check`; root integration of `8cba0ea6`; no local tests/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; GitHub Actions is the only runtime test executor
fixture·cassette: `approval_continuation_unavailable_contract_matches_recovery_producer`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; the Run-bound continuation failure producer now has an exact registry contract
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: the recovery append still uses the Run event stream while static metadata retains the approval aggregate; generic EventStore append does not call `validate_runtime_event`; other event producers and full historical migration remain open; no approval durability, replay or external-effect claim
reviewer: source trace matched persisted approval/run IDs and the bounded error, terminal flag, migration lookup and source guard; no local runtime reviewer
```

## 5.14 Capability request and result delivery producer contracts

The two `run.capability_requested` producers now write the explicit
`capability_request_id` required by the invocation identity contract while retaining their
historical `request_id` field for compatibility. The `result.delivery_claimed` registry entry now
uses an exact allowlist for `result_digest`, `receipt_digest`, `outcome_state`, `outcome_ready` and
`delivery_policy` in addition to the invocation identity fields emitted by the delivery command.
The fixture rejects a missing capability identity and unknown delivery fields; no EventStore
validation or result-delivery behavior changed.

```text
source_snapshot: source commit `5c3336f0`; `kiana-core/src/capabilities.rs`; `kiana-core/src/dispatch.rs`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: both `run.capability_requested` producers now emit capability_request_id alongside the compatibility request_id, and `result.delivery_claimed` has the exact five-field delivery allowlist; no EventStore validator or delivery behavior changed
command_argv: source trace of preparation-rejection/normal capability request producers and result delivery claim; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick e8f69d0d`; no local tests/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; GitHub Actions is the only runtime test executor
fixture·cassette: `capability_requested_and_result_delivery_contracts_match_producers`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; capability-request identity and result-delivery producer fields now match the registry
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; request_id remains a compatibility alias until downstream projections migrate; complete historical producer reconciliation, delivery crash recovery and external-effect evidence remain open
reviewer: source trace matched both capability-request producers, result delivery claim fields, invocation identity and deny-first fixture/source guard; no local runtime reviewer
```

## 5.15 Run tool-call and tool-result producer contracts

`run.tool_call` and `run.tool_result` are now bounded independently from the broad run-event
allowlist. The call contract admits only its run/invocation identity, call/turn/step identity,
execution scope, tool and operation. The result contract admits its run/invocation identity, call,
result and the cancellation/effect/stop facts written by capability, lifecycle and governance
producers. The fixture rejects an unregistered call/result field and a missing
`capability_request_id`; no projection or EventStore behavior changed.

```text
source_snapshot: source commit `681b6c9a`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-core/src/capabilities.rs`; `kiana-core/src/lifecycle.rs`; `kiana-core/src/data_governance.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: `run.tool_call` and `run.tool_result` now use separate exact allowlists instead of broad RUN_FIELDS; all known capability/lifecycle/governance producer fields are covered, while unknown fields and missing capability identity remain deny-first; no EventStore or projection behavior changed
command_argv: source trace of capability dispatch, cancel, approval invalidation and governance tool-result producers; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick 5fb4ea37`; no local tests/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; GitHub Actions is the only runtime test executor
fixture·cassette: `run_tool_contracts_match_cancel_and_dispatch_producers`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; run tool-call/result producer fields now have bounded per-kind contracts
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; run.rejected/compacted/receipt and model/session producer families remain unreconciled; no durable replay, delivery crash recovery or external-effect claim
reviewer: source trace matched capability dispatch and lifecycle/governance cancellation producers, exact per-kind field sets and deny-first fixture/source guard; no local runtime reviewer
```

## 5.16 Run predecessor producer contract

`run.predecessor` now uses a bounded six-field allowlist matching the lifecycle producer:
`run_id`, `previous_run_id`, `turn_id`, `turn`, `session_id` and `semantics`. The event requires
only the new run identity; the predecessor and turn metadata remain payload facts. The fixture
rejects a missing `run_id` and unknown fields. No run lifecycle, projection or EventStore behavior
changed.

```text
source_snapshot: source commit `e7329d8c`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-core/src/lifecycle.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: `run.predecessor` now uses an exact six-field allowlist instead of broad RUN_FIELDS and keeps only run_id required; no lifecycle or EventStore behavior changed
command_argv: source trace of `start_run_with_id` predecessor event; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick 79814e6d`; no local tests/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; GitHub Actions is the only runtime test executor
fixture·cassette: `run_predecessor_contract_matches_lifecycle_producer`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; predecessor event payload is now bounded to its real lifecycle producer
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; resume_prepared, compacted, receipt, rejected and model/session producer families remain open; no durable replay or external-effect claim
reviewer: source trace matched lifecycle predecessor fields, required run identity and deny-first fixture/source guard; no local runtime reviewer
```

## 5.17 Run resume-prepared producer contract

`run.resume_prepared` now has a bounded six-field allowlist matching the recovery claim producer:
`run_id`, `session_id`, `actor_id`, `snapshot_event_id`, `turn_id` and `turn`. Only the resumed
run identity is required; the snapshot event and new turn remain explicit recovery facts. The
fixture rejects a missing `run_id` and unknown fields. No resume CAS, projection or EventStore
behavior changed.

```text
source_snapshot: source commit `5e1b8ee2`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-core/src/recovery.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: `run.resume_prepared` now uses an exact six-field allowlist instead of broad RUN_FIELDS and keeps only run_id required; no recovery or EventStore behavior changed
command_argv: source trace of the explicit resume snapshot claim; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick 9f06808e`; no local tests/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; GitHub Actions is the only runtime test executor
fixture·cassette: `run_resume_prepared_contract_matches_recovery_producer`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; resume-prepared recovery payload is now bounded to its real producer
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; compacted/receipt/rejected and model/session producer families remain open; no resume durability, cross-process replay or external-effect claim
reviewer: source trace matched recovery snapshot claim fields, required run identity and deny-first fixture/source guard; no local runtime reviewer
```

## 5.18 Run compaction producer contract

`run.compacted` is now registered as a non-terminal run event with the exact fields emitted by the
lifecycle compaction handler: `schema`, `run_id`, `tokens_before`, `tokens_after` and
`summary_present`. It requires the run identity and keeps the explicit legacy run migration. The
fixture rejects a missing `run_id` and unknown fields; no compaction, projection or EventStore
behavior changed.

```text
source_snapshot: source commit `d7513491`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-core/src/lifecycle.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: `run.compacted` is registered with run aggregate, run_id requirement, non-terminal semantics, legacy run migration and an exact five-field allowlist; no lifecycle or EventStore behavior changed
command_argv: source trace of the RunnerEvent::Compacted lifecycle producer; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick cad189f0`; no local tests/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; GitHub Actions is the only runtime test executor
fixture·cassette: `run_compacted_contract_matches_lifecycle_producer`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; compaction event schema is now bounded to its real lifecycle producer
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; run.receipt/rejected and model/session producer families remain open; no durable compaction replay or external-effect claim
reviewer: source trace matched compaction schema/tokens/summary fields, migration metadata, required run identity and deny-first fixture/source guard; no local runtime reviewer
```

## 5.19 Run receipt top-level projection contract

`run.receipt` is a derived read-model snapshot written after the real run terminal event. The
registry now bounds its top-level shape to the 37 fixed keys emitted by `receipt_from_events`, plus
the optional `work_packet_id` and `input` keys added by `with_work_packet`; it remains a
non-terminal run event with the legacy run migration. The fixture checks the complete top-level
shape, missing `run_id` and unknown top-level fields while leaving nested projection contracts to
their owners. No receipt computation, lifecycle behavior or EventStore validation changed.

```text
source_snapshot: source commit `140b0420`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-core/src/lifecycle.rs`; `kiana-core/src/receipts.rs`; `kiana-core/src/events.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: `run.receipt` now uses an exact 39-key top-level allowlist (37 fixed projection keys plus optional work-packet keys), requires run_id, is non-terminal and carries legacy run migration; nested read-model values remain dynamic and no receipt behavior changed
command_argv: source trace of the lifecycle receipt write, receipt_from_events fixed keys and with_work_packet optional keys; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick 59264b3f`; no local tests/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; GitHub Actions is the only runtime test executor
fixture·cassette: `run_receipt_contract_bounds_top_level_projection`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; receipt top-level envelope is now bounded without claiming nested projection closure or business outcome
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; nested receipt/aggregation/model/invocation projections remain owner contracts, and receipt remains a derived shadow copy rather than an additional fact; no durable replay, external delivery or outcome claim
reviewer: source trace matched lifecycle write, receipt_from_events fixed top-level keys, with_work_packet optional keys and deny-first fixture/source guard; no local runtime reviewer
```

## 5.20 Credential recovery producer contract

The existing `recovery.credential` registry entry now has a deny-first producer fixture. The only
typed producer, `CredentialRecoveryFact::to_runtime_event`, emits the exact top-level envelope
`{run_id, recovery}` on the run aggregate; the nested recovery fact remains governed by its own
versioned `CredentialRecoveryFact` validation. The fixture rejects a missing `run_id` and unknown
top-level fields. No recovery projection or EventStore behavior changed.

```text
source_snapshot: source commit `706881f5`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/src/credential_recovery_evidence.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: existing `recovery.credential` registry contract is now pinned to the unique typed producer envelope `{run_id,recovery}` with run aggregate and required run_id; nested fact validation remains owner-scoped; no recovery or EventStore behavior changed
command_argv: source trace of `CredentialRecoveryFact::to_runtime_event`; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick 4e98fb29`; no local tests/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; GitHub Actions is the only runtime test executor
fixture·cassette: `credential_recovery_contract_matches_typed_producer`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; the credential-recovery top-level producer envelope now has an explicit deny-first fixture
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; nested recovery fact validation, projection replay, durable checkpoint and live credential rotation remain separate contracts; no authorization or external-effect claim
reviewer: source trace matched the unique typed recovery producer, registry fields, required run identity and deny-first fixture/source guard; no local runtime reviewer
```

## 5.21 Communication lifecycle producer contracts

The two handoff lifecycle events share one exact top-level allowlist: message, message ID,
lifecycle, accepted, reason, authority/project/actor/session/request context. Incident escalation
uses the same envelope with an additional `evidence_refs` field. All three events remain
communication aggregate, non-terminal and without a migration declaration. The fixture rejects a
missing message ID and unknown fields for each variant; nested message/lifecycle DTOs remain owned
by their communication contracts.

```text
source_snapshot: source commit `cb294092`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-core/src/communication.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: handoff acknowledged/rejected now use an exact shared lifecycle allowlist and incident escalation uses an exact evidence_refs variant; all keep message_id required, communication aggregate and non-terminal semantics; no communication or EventStore behavior changed
command_argv: source trace of acknowledge_communication and escalate_communication producers; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick 24f1caf9`; no local tests/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; GitHub Actions is the only runtime test executor
fixture·cassette: `communication_lifecycle_contracts_match_producers`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; communication lifecycle payload variants are now bounded per kind
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; communication send kinds and nested message/lifecycle schema remain separate contracts, and no durable notification delivery or external channel claim is made
reviewer: source trace matched both handoff branches, incident escalation evidence refs, message identity and deny-first fixture/source guard; no local runtime reviewer
```

## 5.22 Communication send producer contracts

The seven communication send kinds share one producer and now use one exact eight-field top-level
allowlist: `message`, `message_id`, `lifecycle`, `authority_granted`, `project_root`, `actor_id`,
`session_id` and `request_id`. They remain communication aggregate, non-terminal and without a
migration declaration. The fixture covers every kind and rejects a missing message or unknown
field; nested message and lifecycle values remain owned by their DTO contracts.

```text
source_snapshot: source commit `87cafdc3`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-core/src/communication.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: seven communication send kinds now use an exact shared eight-field allowlist matching `send_communication`; message remains required and communication aggregate/non-terminal semantics remain explicit; no communication or EventStore behavior changed
command_argv: source trace of `communication_event_kind` and `send_communication`; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick 73a701c5`; no local tests/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; GitHub Actions is the only runtime test executor
fixture·cassette: `communication_send_contracts_match_shared_producer`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; communication send payload is now bounded across all seven send kinds
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; lifecycle transition and nested DTO contracts remain separate, with no durable notification delivery or external channel claim
reviewer: source trace matched shared send producer, seven event-kind mappings, required message identity and deny-first fixture/source guard; no local runtime reviewer
```

## 5.23 Connector health and MCP handshake producer contracts

The connector health event already had a fixed top-level envelope and now has an explicit ER-01
fixture/source guard. The previously opaque `connector.mcp_handshake` event is registered with its
own exact ten-field envelope. Both events use the connector aggregate, require request/connector/
binding identity, remain non-terminal and have no migration declaration. Nested health and
handshake values remain validated by their existing DTO contracts; no connector behavior or
EventStore validation changed.

```text
source_snapshot: source commit `e92bf2e8`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-daemon/src/connectors.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: connector.health_checked is pinned to its existing 12-field top-level producer and connector.mcp_handshake is newly registered with an exact 10-field allowlist; required IDs, connector aggregate and non-terminal semantics are explicit; nested DTOs remain owner-scoped
command_argv: source trace of daemon health/handshake event producers; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick fd952df1`; no local tests/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; GitHub Actions is the only runtime test executor
fixture·cassette: `connector_health_and_handshake_contracts_match_daemon_producers`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; connector health and previously opaque MCP handshake envelopes are now bounded
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; nested health/handshake schemas and connector registry replay remain owner contracts, with no external transport or credential effect claim
reviewer: source trace matched both daemon producers, required connector identities, exact top-level fields and deny-first fixture/source guard; no local runtime reviewer
```

## 5.24 Session assignment producer contract

`session.assigned` now matches the stable assignment envelope written by
`bind_session_assignment`. Its required identity is `session_id`, which is also the stable
assignment key; the random RuntimeEvent request envelope remains transport metadata. The registry
uses an exact 18-field top-level allowlist, session-assignment aggregate and legacy session
migration. The fixture rejects a missing session ID and unknown fields. No session binding,
idempotency or EventStore behavior changed.

```text
source_snapshot: source commit `fb738736`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-core/src/sessions.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: `session.assigned` now uses session_id as required identity and an exact assignment envelope instead of request fields; existing session binding/idempotency behavior is unchanged
command_argv: source trace of `bind_session_assignment`; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick 1b82ff3c`; no local tests/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; GitHub Actions is the only runtime test executor
fixture·cassette: `session_assignment_contract_matches_producer`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; session assignment identity and top-level payload are now bounded to the real producer
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; nested principal/project/assignment DTO contracts and cross-process session rebuild remain separate, with no external-effect claim
reviewer: source trace matched stable session assignment key, exact producer fields and deny-first fixture/source guard; no local runtime reviewer
```

## 5.25 Action authority pin producer contract

`action.authority_pinned` has one producer in `ControlPlane::pin_action_authority`. It emits
`request_id`, `action_digest`, `authority_version`, `authority_key` and `actor_id`, and commits
to the `action/<request_id>` stream at version 1 while fencing the read `authority/<key>` version.
The existing registry already declares the action aggregate, required request/action digest,
non-terminal semantics and legacy action migration name; this slice adds deny-first fixture and
source coverage without changing producer or registry behavior. The named migration is registry
metadata only here; no legacy payload transformer or EventStore-wide validator wiring is claimed.

```text
source_snapshot: base commit `9cfebfe3`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-core/src/dispatch.rs`; `kiana-core/tests/er01_event_contract_guard.rs`; `docs/roadmap/event-receipt-schema-baseline.md`; `docs/roadmap/event-receipt-recovery.md`; `docs/roadmap.md`; `CURRENT_STATUS.md`
worktree_status: existing `action.authority_pinned` registry and producer are unchanged; domain fixture now checks action aggregate, request_id/action_digest, allowlist, non-terminal/migration metadata, valid payload, missing IDs and unknown-field rejection; core source guard pins its unique producer, emitted fields, action stream version and authority read-set
command_argv: source trace of `ControlPlane::pin_action_authority`; `cargo fmt --all --check`; `git diff --check`; isolated commit; no local tests/build/check/clippy/smoke
cwd·environment: `/tmp/kiana-er01-action-authority-pinned`; branch `step/er01-action-authority-pinned-20261003`; base `origin/master` at `9cfebfe3`; Linux/bash; runtime fixtures remain GitHub CI only
fixture·cassette: `action_authority_pinned_contract_matches_dispatch_pin_and_rejects_drift`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; existing unified CI domain/Core shards; fresh receipt pending
exit_code: formatting and diff checks passed; no local runtime result
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; the existing authority pin event now has deny-first fixture and unique-producer source guard
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: the migration entry names a legacy action family but does not implement a payload transformer; generic EventStore append still does not call `validate_runtime_event`; no durable replay, authority effect or runtime fixture result is established
reviewer: source trace matched the unique dispatch producer, exact payload, action aggregate/version and authority read-set; no local runtime reviewer
```

## 5.26 Run lifecycle producer contracts

The core run lifecycle events now use per-kind allowlists instead of the broad `RUN_FIELDS` set:
`run.authorized` follows its server-owned authorization envelope, `run.started` carries only the
run identity, `run.prompt` carries the shared prompt/turn envelope, and `run.delta` carries only
run identity plus accumulated text. All require `run_id`, remain non-terminal and use the legacy
run migration. The fixture covers the four real producer shapes and rejects missing identity and
unknown fields; no lifecycle or EventStore behavior changed.

```text
source_snapshot: source commit `b986bf4d`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-core/src/lifecycle.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: run.authorized/started/prompt/delta now use separate exact allowlists matching their lifecycle producers; required run_id, non-terminal semantics and legacy migration remain explicit; no lifecycle or EventStore behavior changed
command_argv: source trace of lifecycle authorization, runner start, prompt and delta producers; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick 708653e2`; no local tests/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; GitHub Actions is the only runtime test executor
fixture·cassette: `run_lifecycle_contracts_match_real_producers_and_reject_drift`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; core run lifecycle envelopes are now bounded per kind
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; terminal run events, model turns and rejected/approval branches remain separate contracts; no durable replay or external-effect claim
reviewer: source trace matched authorization/start/prompt/delta producer shapes, required run identities and deny-first fixture/source guard; no local runtime reviewer
```

## 5.27 Run input, snapshot and clarification producer contracts

Input acceptance, input-claim rejection, run snapshots and clarification requests now use
independent top-level allowlists matching the lifecycle/recovery producers. Each retains its
existing required run/input or run/interaction/turn identities, non-terminal run aggregate and
legacy run migration. Nested snapshot and clarification values remain owned by their respective
contracts. The fixture covers all four shapes and rejects missing IDs and unknown fields; no input,
checkpoint, clarification or EventStore behavior changed.

```text
source_snapshot: source commit `5c8abd03`; `kiana-domain/src/event_contracts.rs`; `kiana-domain/tests/er01_event_contract.rs`; `kiana-core/src/lifecycle.rs`; `kiana-core/src/recovery.rs`; `kiana-core/tests/er01_event_contract_guard.rs`
worktree_status: run.input.accepted/claimed, run.snapshot and run.clarification.requested now use exact per-kind envelopes instead of broad RUN_FIELDS; required IDs and non-terminal/migration metadata remain explicit; no lifecycle/recovery or EventStore behavior changed
command_argv: source trace of queue_run_input, checkpoint_run and RunnerEvent::ClarificationRequested; isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick da618835`; no local tests/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; GitHub Actions is the only runtime test executor
fixture·cassette: `run_input_snapshot_and_clarification_contracts_match_producers`; updated `event_contract_registry_and_migration_boundary_are_source_owned`; unified CI routes domain/Core targets through `kiana-domain-s2/4` and `kiana-core-s1/6`; fresh receipt pending after push
exit_code: formatting and diff checks passed; no local runtime result; remote fixtures pending
status_change: ER-01 remains roadmap row 036 `🔄`, `feature_status=partial`, `proof_level=source`; run input/snapshot/clarification envelopes are now bounded per kind
proof-level change: none; no local_behavior, durable, live or physical promotion
limitations: generic EventStore append still does not call `validate_runtime_event`; nested snapshot/clarification DTOs, claim-to-runner atomicity and cross-process recovery remain separate contracts; no durable replay or external-effect claim
reviewer: source trace matched lifecycle/recovery producer shapes, required IDs and deny-first fixture/source guard; no local runtime reviewer
```

## 5.28 Requested approval field isolation and focused GitHub acceptance

Run `37089844345`, head `4682efe5`, domain job `111107689615` executed the complete
ER-01 contract target: 28 passed and the requested-approval fixture failed because it compared
its field count to the separately narrowed approved-decision contract (20 versus 11). The requested
producer has 13 fixed fields and optional run/resume-binding fields. Its allowlist is now exactly
that 15-field union, and the fixture validates direct and Run-bound shapes while refusing the five
unproduced fields that came from the old shared approval template. The Core source guard counts
writer arguments rather than read predicates containing the same kind literal.

The restored `.github/workflows/er01-event-schema.yml` is manual-only. It executes the complete
domain contract target and Core guard in separate jobs, with no push-concurrency cancellation, so
subsequent roadmap pushes can continue while this exact snapshot finishes remotely.

```text
source_snapshot: `2a69bc6f`; kiana-domain/src/event_contracts.rs; kiana-domain/tests/er01_event_contract.rs; kiana-core/tests/er01_event_contract_guard.rs; .github/workflows/er01-event-schema.yml
worktree_status: requested approval fields are scoped to the real producer; Core writer counting excludes query predicates; manual-only acceptance added without changing unified shard coverage
command_argv: gh run view 37089844345 --job 111107689615 --log; source trace of stage_capability_action; cargo fmt --all; cargo fmt --all --check; git diff --check; no local tests/build/check/clippy/smoke
cwd·environment: repository root on Linux/bash; observed runtime result is from GitHub Actions at 4682efe5
fixture·cassette: complete er01_event_contract target previously 28/29; updated requested fixture covers direct/Run-bound input and unknown fields; fresh manual domain/Core jobs pending dispatch after push
exit_code: prior domain target failed; local format/diff checks 0; no current runtime result
status_change: ER-01 remains partial/source and roadmap row 036 remains 🔄; a fixture assumption and excess requested fields are corrected
proof-level change: none
limitations: prior passed cases cover their unchanged source only; the new source has no receipt yet; the Core target was not observed in the inspected unified job log; generic EventStore validation and complete legacy upcasters remain open
reviewer: source review of the real approval request producer and exact remote failure, plus diff review; no local runtime reviewer
```

## 5.29 Manual CI receipt and input-fixture key correction

Manual run `37091006922` at `7fe7da0e` completed. Core job `111111159716` passed its entire
source guard (1/1). Domain job `111111159623` exited 101 before any tests because the input fixture
passed an iterated `&&str` key to `serde_json::Map::remove` (`E0277`, line 1745). Source `821627a9`
destructures the key reference; every required-ID and unknown-field assertion is retained.

```text
source_snapshot: 821627a9; kiana-domain/tests/er01_event_contract.rs; observed head 7fe7da0e
worktree_status: one fixture iteration corrected from &&str to &str; no product/schema behavior changed
command_argv: gh run view 37091006922 --job 111111159623 --log; gh run view 37091006922 --job 111111159716 --log; cargo fmt --all --check; git diff --check
cwd·environment: repository root Linux/bash; GitHub Actions is the only runtime test executor
fixture·cassette: Core er01_event_contract_guard 1/1 success; domain input fixture compile error E0277; fresh domain execution pending
exit_code: Core remote job 0; domain remote job 101 before tests; local format/diff checks 0
status_change: ER-01 remains partial/source; Core guard now has an exact receipt for 7fe7da0e, domain remains unverified
proof-level change: none
limitations: guard success is source-boundary coverage only; corrected domain target and complete EventStore/migration behavior remain unverified
reviewer: exact remote logs and fixture type/source review
```

## 5.30 Run capability request, denial and approval-wait contracts

`run.capability_requested` now admits the exact union of the preparation-denial and normal
request producers. `run.capability_blocked` admits the common attempt facts and their error/reason
variants. Its terminal marker describes the capability attempt, not the enclosing run outcome.
`run.awaiting_approval` includes the actual approval ID and requires all three run, capability
request and approval identities. The previous broad run allowlist omitted approval_id and would
have rejected that producer if used; no generic EventStore validation is enabled by this repair.

```text
source_snapshot: 472eb3b4 plus fixture-key correction 879e2aa6; event_contracts.rs; er01_event_contract.rs; er01_event_contract_guard.rs; producer capabilities.rs
worktree_status: three kind-specific bounded allowlists and Run-bound approval IDs; no producer, policy, dispatch or EventStore behavior changed
command_argv: source trace of both request/denial paths and stage_capability_action; git cherry-pick c8a491f0; cargo fmt --all --check; git diff --check; no local tests/build/check/clippy/smoke
cwd·environment: isolated worktree based on c55b2bb2, integrated at repository root Linux/bash; tests execute only in GitHub
fixture·cassette: run_capability_and_approval_contracts_match_all_producers plus Core producer/spec guards; complete domain/Core manual acceptance available
exit_code: local format/diff 0; fresh remote fixture receipt pending
status_change: ER-01 remains row 036 🔄 / partial / source; approval-wait payload and required identities now match its producer
proof-level change: none
limitations: exact top-level unions only; nested payload validation and generic EventStore enforcement remain open; attempt denial is not a run terminal fact
reviewer: all producer variants traced; domain missing-ID/unknown-field and Core source boundaries reviewed
```

## 6. 限制与交接

- 当前 `RuntimeEvent` 没有强制内嵌 schema/version 字段；registry 是 additive interpretation layer，完整 EventStore/projector 接线由 ER-02+ 完成。
- `allowed_fields` 是关键 kind 的 bounded contract，不宣称覆盖所有 177+ 历史 event literals；未覆盖 kind 仍按 opaque/required-family policy 处理。
- legacy migration map 只声明确定的 v0→v1 family 名称，不会猜测缺失 TurnId、aggregate、owner 或 secret provenance；歧义只能查询。
- payload validator 不替代 redaction、Artifact 引用、CAS、receipt correctness、external effect/reconcile、backup/retention/delete 或 cross-process recovery。
- 本仓库任务不在本地运行测试、build、check、clippy 或 smoke；仅使用 `cargo fmt --all --check` 和 `git diff --check` 做格式/空白检查。GitHub CI 负责运行时验证；后续 ER-02/03 应在新快照刷新 hash。

## Named rejection scenarios

The source guard asserts these scenarios by name, so the baseline records the same vocabulary the
fixtures use:

- `unknown_required_event_kind_fails_closed` — an event whose `kind` is not in the registered
  required set is refused rather than stored as an unknown-kind event.
- `event_schema_version_cannot_downgrade` — a version below the registered schema version is
  refused; a reader never interprets a newer event as an older one.
- `event_payload_unknown_field_is_not_silently_dropped` — a payload field outside `allowed_fields`
  is refused, not quietly discarded on decode.
- `legacy decode` — a legacy frame is admitted only through a named migration, never by a
  best-effort parse.
