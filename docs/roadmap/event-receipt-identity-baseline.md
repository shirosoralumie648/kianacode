# ER-02 identity, correlation, causation, and ordering baseline

> 快照日期：2026-10-05。本文记录 ER-02 的稳定 ID 关联和 legacy 兼容边界；当前 head 增加受资源约束的本地 focused receipt，仍不宣称 durable/live/physical。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-02`](event-receipt-recovery.md#step-er-02) |
| source snapshot | current dirty worktree after ER-01 local closure and ER-02 fixture corrections |
| feature_status | `partial`（identity-link/EventStore/projection plus direct and Core approval focused behavior are verified; DaemonHost/Harness parity and complete CI receipt remain unproven） |
| proof_level | `local_behavior` for the focused selectors; no durable/live/physical claim |
| canonical path | command/request → RuntimeEvent correlation/causation/parent links → aggregate stream version → Invocation/Run/Receipt projection |
| this step does | 区分 command/request/session/run/turn/invocation/execution/attempt/event ID owner，新增可选 links，默认 correlation=request_id，拒绝 self-link，投影按 run/request ID 配对而非 sequence |
| this step does not | 不把 correlation 当授权，不用 request-local sequence 代替 aggregate version，不改写 legacy JSONL，不宣称全量 legacy upcast 或跨进程 durable recovery |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| RuntimeEvent/Correlation/Journal | `kiana-domain/src/states.rs`, `kiana-domain/src/correlation.rs`, `kiana-domain/src/contracts.rs`, `kiana-domain/src/journal.rs`, `kiana-domain/src/lib.rs` | `e2bf22e055dc09ff2a54e5bd9880bc86b672db09382e0c260d6f9fe928d1b903`, `6fabe5e7cb8d2c86604738108eebcea6274d36306afac197a6115d13b1bfa951`, `cfd802764d31a331eddfeecdd97270af146c39f868513c75a36cdd7e59212954`, `4dbd656d03ec12e7821ffac254307227419423cbaf74ccb99a2b819c5eeedd48`, `080b20570e29fedcd062e53b6391d138b2db4e31c81fc91332638578dcd07c83` |
| Core projection/links | `kiana-core/src/events.rs`, `kiana-core/src/invocation_projection.rs`, `kiana-core/src/span_projection.rs` | `11312aaff9a78f3fdf5ba90dd03cbd9a2ac7b1d7190b372d01cc103decd7d09a`, `5ff42bc196687aa20c88f97ea79c9c935143a226b20f0d8351995828ab4fa936`, `e28d545faeb51c8ff1c6410aac0f8b96687a8e283641cb6ac6887db15b9fdfec` |
| EventStore/fixtures/workflow | `kiana-eventlog/src/event_store_core.rs`, `kiana-eventlog/src/memory.rs`, `kiana-eventlog/src/journal_core.rs`, `kiana-domain/tests/er02_identity.rs`, `kiana-eventlog/tests/er02_identity.rs`, `kiana-core/tests/er02_identity.rs`, `kiana-core/tests/er02_identity_guard.rs`, `.github/workflows/er02-identity.yml` | `506a8222a757a89e6516a12b6260daa7f35af2b3da49c17a23ca7cdbcc9d0b1e`, `bcb98daa0a9548384a9dafdd9d5af2aefacca3d01d699056ba86477d3276e9e9`, `84ef64bc8208b2ead45d1d05feb0265e94129b898cd67d04f5ed189d388bba56`, `ed8a5db0ff3fc7f959a7ae9953aa582c002b5b8e4d4b12f1866cc93856614bfc`, `021f7139985cb59f7a1a0d0ddecb53a3c0cb6c4881aa31efbc783238f9b6dbf1`, `1230a3817e9bab40faf247a7ab4c6a9b835715ef5e4ed6097e43b93a0f232364`, `31a1fc8a064bce4a4de1c1e47e4648fec8dceee61f350cd04309a5b4a599605c`, `f7518849a3e2937f9b11d0ef74ca474ab45db99f259b52fb225425bc6bc2147f` |

hash 是源码漂移锚点；旧事件的缺失字段保持兼容，不因索引存在而提升 durability。

## 2. ID ownership matrix

| ID | Owner / 生命周期 | 可作为主键？ |
|---|---|---|
| `command_id` | 发起一次状态转移的 ControlPlane command；同 digest 可 replay | command receipt / transition idempotency |
| `request_id` | 一次入口/事件请求关联；兼容旧 API | 事件请求关联，不等于 aggregate version |
| `session_id` | 用户会话及 owner assignment | session scope，不单独授权 |
| `run_id` | 一次 Run 生命周期；新 Turn 创建新 Run，legacy Continue 可复用 | Run aggregate stream |
| `turn_id` | server-derived Start/NewTurn/LegacyContinue/Resume 轮次 | Turn 关联，不单独授权 |
| `invocation_id` | 一次逻辑 capability invocation；通常由 capability request 派生 | invocation projection / ledger key component |
| `execution_id` | 一次 permit/handler attempt；每次重新执行应新建 | execution/permit stream |
| `attempt` | 同 invocation 的单调尝试序号 | 与 execution_id 一起解释重试 |
| `event_id` | 每条不可变 RuntimeEvent 全局 ID | EventStore 去重/证据引用 |
| `correlation_id` | 观察关联根，默认 request_id | 仅关联，不授予权限 |
| `causation_event_id` / `parent_event_id` | 事实因果或异步 parent link | 只读拓扑，不能替代 owner/approval |

`RuntimeEvent` 新建时默认携带 `correlation_id=request_id`；旧事件缺少可选 links 仍可读取。`with_identity_links` 只增加关系，不改变 `event_id`、aggregate 或 stream version；self-causation/self-parent 和 command-without-correlation fail-closed。

## 3. CorrelationContext boundary

现有 `kiana-domain::CorrelationContext` 负责 trace/span、scope、command/request、Run/Turn/Invocation/Execution、Attempt、causation 和 source cursor 的值校验。Trace/foreign parent 只是关联输入；principal、ProjectTrust、policy、grant、approval、budget 和 permit 仍由 ControlPlane/Domain authority 产生。`CausationRef::Attempt` 必须与同一 run/turn/invocation/execution/command scope 匹配，不能按字符串或 sequence 猜测。

EventLog 的 aggregate stream version 是顺序权威；带 aggregate metadata 的新 append 必须同时提供非空 `aggregate_type`、非空 `aggregate_id` 和正数 `stream_version`。三者全缺的 legacy event 保持兼容；部分 tuple 或 version 0 在 EventLog 写入边界拒绝。request-local `sequence` 只用于一个请求中的展示/兼容排序。跨请求、跨 run 的结果必须携带 `run_id`/`capability_request_id`/invocation or execution IDs，projection 先校验 run binding，再折叠 terminal/result。

## 4. Projection and legacy rules

| 情况 | 处理 |
|---|---|
| same event_id 重放 | EventStore 全局拒绝 duplicate；原事件保持不变 |
| same command_id + same digest | `Replayed` 返回原 receipt，不追加事实 |
| same command_id + different digest | structured conflict；不得按最新 payload 覆盖 |
| same sequence but foreign run | Invocation/Run projector 严格过滤 run_id/aggregate；不能配对结果 |
| event missing aggregate metadata | legacy reader 可只读兼容；新 authority/transition path 不把它当 CAS 事实 |
| event missing optional identity links | 以 request/event/aggregate 的确定性兼容边界读取；不能推导更强 owner/approval |
| unknown required family | ER-01 `unknown_required_event_kind`；不进入执行投影 |
| causation/parent self or malformed ID | `validate_identity_links` / parser 拒绝；不改写事件 |

`project_invocations`、`project_capability_attempts`、span/replay projections 继续以稳定 request/run/invocation/execution IDs 和 aggregate stream metadata 为关联依据；UI/transcript/cache 不是事实源。Direct、Harness、approval-resume 的 event shapes 可有入口专属字段，但必须保留同一 ID chain 和 redacted action digest。

## 5. CI-only fixture catalog

| Fixture | Purpose |
|---|---|
| `new_events_have_request_correlation_and_explicit_links_round_trip` | 默认 correlation、command/causation/parent links、serde round trip |
| `legacy_events_without_links_remain_readable_but_self_links_fail_closed` | legacy decode、可选 links 和 self-link 拒绝 |
| `journal_frames_reject_identity_link_drift` | Journal frame validation preserves the same self-link fail-closed boundary |
| `event_id_reuse_is_denied` | EventStore 全局 event_id duplicate fail-closed |
| `same_request_different_command_digest_conflicts` | command digest drift 不覆盖既有事实 |
| `malformed_identity_links_are_denied_before_append` | EventStore append 边界拒绝 self-causation/self-parent 与 command-without-correlation |
| `idempotent_replay_rejects_command_id_drift` | 相同幂等键 replay 拒绝 command ID 单独漂移 |
| `idempotent_replay_rejects_correlation_id_drift` | 相同幂等键 replay 拒绝 correlation ID 单独漂移 |
| `idempotent_replay_rejects_causation_event_id_drift` | 相同幂等键 replay 拒绝 causation event ID 单独漂移 |
| `idempotent_replay_rejects_parent_event_id_drift` | 相同幂等键 replay 拒绝 parent event ID 单独漂移 |
| `idempotent_replay_rejects_redaction_or_artifact_metadata_drift` | 相同幂等键 replay 拒绝脱敏 profile、payload 可恢复性、data epoch 或 artifact 引用漂移 |
| `cross_run_result_cannot_pair_by_sequence` | 同 sequence/同 capability ID 的 foreign run result 不配对 |
| `zero_stream_version_is_denied_before_append` | aggregate stream version 0 在写入前以稳定错误拒绝 |
| `partial_stream_metadata_is_denied_before_append` | 缺失或空白 aggregate type/id 的不完整 tuple 在写入前拒绝 |
| `positive_stream_metadata_is_accepted` | 合法非空 aggregate tuple 与正数 stream version 仍可写入 |
| `event_identity_links_and_projection_use_stable_ids_not_request_sequence` | domain/core source guard |
| `direct_capability_events_keep_request_identity_without_run_scope` | direct 成功事件保持 request/correlation/capability request ID，不合成 Run/Turn |
| `wire_approval_proof_retry_resumes_original_run` | 真实 DaemonHost/JournalApprovalStore Harness approval-resume 保留 Run root、Turn、capability request 与 invocation IDs，并核对完整 invocation projection/event IDs |
| `invocation_projection_normalizes_and_validates_terminal_result_receipts` | dispatch/finalizer receipt 忽略边界 ID 差异、校验实际结果 digest 和 identity，并拒绝 digest/request/execution/effect-known/outcome/lifecycle drift |
| `invocation_projection_rejects_uncommitted_terminal_receipts` | 拒绝 `committed=false` 的 `execution.result_committed` 与 `capability.completed` receipt |
| `invocation_projection_rejects_malformed_identity_links` | projection 在折叠事实前拒绝 self-parent/self-causation 等 malformed identity links |
| `invocation_projection_rejects_request_id_alias_drift` | capability/run/approval/permit payload 中冲突的 request-id aliases fail closed，不按首个字段静默选择 |
| `nil_identity_links_fail_closed_but_legacy_links_remain_optional` | nil identity link values are rejected while absent optional links remain compatible for legacy events |
| `nil_identity_links_are_denied_before_append` | EventLog append boundary rejects nil identity IDs before persistence |

统一 `.github/workflows/ci.yml` 按 `scripts/ci/test-shards.json` 执行 domain/core ER-02 targets 和完整 `kiana-eventlog` crate；本地不运行测试，也不等待 CI。

新增 EventStore identity-boundary fixtures 随 `kiana-eventlog` crate target 执行，core
source guard 锁定 storage validator 与幂等 replay 的 identity-link 比较。拒绝路径仍只在
GitHub Actions 验证。

当前 head 进一步将 `redaction_profile`、`payload_recoverable`、`data_epoch` 与
`artifact_refs` 纳入幂等 replay 的 payload equality。相同业务 payload 携带不同治理或工件
边界时返回 `event_idempotency_key_payload_mismatch`，并保持 EventLog 只有一条事实；fresh
`event_id` 仍允许合法幂等重试，不作为 replay equality 字段。

## 6. 限制与交接

- RuntimeEvent identity links 是 optional additive fields；现有大量 legacy events 尚未强制携带 schema/version/command/causation/parent，完整 upcaster 和 writer gate 属于后续 ER/CP-28。
- correlation/trace/span 只做关联和审计投影，不是 authenticated principal、grant、approval 或 permit；外部 traceparent/ID 不能扩权。
- EventStore CAS/duplicate guard 在内存/JSONL 适配器有 source/局部行为证据，但不证明掉电、网络文件系统、跨主机或外部 effect exactly-once。
- Projection 过滤能阻止跨 run 错配，但完整 InvocationLedger、attempt retry/reconcile、result delivery、retention/delete 和 Receipt correctness 仍需 ER-03+、CP-07+、PD/SC。
- 本地只做格式、workspace test-target 静态编译和 diff 检查；不提升 local_behavior/durable/live/physical。

## 6.1 Current-head focused local receipt (2026-10-05)

```text
source_snapshot: current dirty worktree after ER-01 local closure and ER-02 fixture corrections
worktree_status: ER-02 remains roadmap row 037 🔄; no automatic commit/push/merge/rebase
command_argv: `CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 RUST_MIN_STACK=33554432 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_STRIP=symbols CARGO_INCREMENTAL=0 /usr/bin/time -v -o /tmp/kiana-roadmap-20261005-evidence/er02-core-regression.resources bash -lc 'cargo test -p kiana-core --test control_plane capability_policy_and_broker_failures_preserve_event_order --locked --offline -- --test-threads=1; cargo test -p kiana-core --test control_plane direct_capability_events_keep_request_identity_without_run_scope --locked --offline -- --test-threads=1; cargo test -p kiana-core --test control_plane approval_resumes_the_stored_request_once_with_monotonic_events --locked --offline -- --test-threads=1; cargo test -p kiana-core --test er02_identity --locked --offline -- --test-threads=1; cargo test -p kiana-core --test er02_identity_guard --locked --offline -- --test-threads=1; cargo test -p kiana-eventlog --test er02_identity --locked --offline -- --test-threads=1'`
cwd·environment: repository root Linux/bash; serial Cargo and test threads; 32 MiB test-thread stack; stripped, non-incremental test profile; 4.29s user, 0.86s system, 553284 KiB peak RSS, 0 swaps
fixture·cassette: Core direct identity 1/1; Core approval resume/replay 1/1; broker failure preserves `ResultUnknown`/`capability.result_unknown` 1/1; Core ER-02 identity 3/3; Core guard 1/1; EventLog ER-02 identity 9/9
exit_code: 0
status_change: focused direct/approval/EventLog identity behavior recorded; roadmap card remains 🔄 because cross-surface parity is incomplete
proof-level_change: focused selectors `source` → `local_behavior`; no whole-card durable/live/physical promotion
limitations: real DaemonHost/Harness approval proof roundtrip, complete green CI shard, full legacy upcast/projector terminal reconciliation, durable InvocationLedger/cross-process ordering, and external effect receipts remain open; full workspace test remains non-green/resource-intensive
reviewer: integration agent; focused local identity/approval reviewer
```

## 7. Latest readable CI receipt (2026-10-02)

Run `36961448573` used head `21027ecccca3db0fe42a88ecfcd840728c472eba`; its three relevant jobs completed with failure before the run was cancelled by a newer push. ER-02 targets themselves passed:

- `kiana-domain-s2/4`, job `110695889284`: all 3 `er02_identity` fixtures passed: `journal_frames_reject_identity_link_drift`, `legacy_events_without_links_remain_readable_but_self_links_fail_closed`, and `new_events_have_request_correlation_and_explicit_links_round_trip`. The shard also failed `co08_replay`, `co11_criteria_coverage`, `co17_company_handoff`, `co29_rework_contract`, `connector_conformance`, `connector_notifications`, `cp12_resource_lease`, `cp15_cancellation`, `dep08_deployment_config`, `dep11_health_aggregation`, `dep16_reconcile`, `dep17_capacity`, `dep18_incident`, `dep19_backup_manifest`, `dep20_quiesce_gate`, `dep21_backup_lifecycle`, `dep22_restore_quarantine`, `dep27_migration_registry`, `dep39_supply_chain_release_evidence`, and `dep40_release_uat_evidence`.
- `kiana-core-s3/6`, job `110695889194`: both `er02_identity` and `er02_identity_guard` passed: `cross_run_result_cannot_pair_by_sequence` and `event_identity_links_and_projection_use_stable_ids_not_request_sequence`. The shard also failed `cp26_decision_trace_guard`, `cp27_nonblocking_limits_guard`, `cp28_migration_boundary_guard`, `dep00_deployment_guard`, `dep23_restore_activation`, `dep25_effect_reconcile_route`, `dep25_effect_reconciliation`, `dep26_retention_deletion`, `dep27_migration_registry_guard`, `dep28_migration_preflight_guard`, `dep29_migration_primitives_guard`, `dep30_migration_runner_guard`, `dep31_migration_rebuild_guard`, `dep32_migration_rollback_guard`, `dep33_revision_compatibility_guard`, `dep34_release_preflight_guard`, `dep36_container_adapter_guard`, `dep37_orchestrated_rollout_guard`, `dep38_rollout_lifecycle_guard`, `dep39_supply_chain_guard`, `dep41_release_gate_guard`, `dependency_boundaries`, `eq09_eval_runtime_guard`, `eq10_fake_provider_guard`, `eq11_deny_broker_guard`, `eq13_initial_state_guard`, `eq14_evidence_capture_guard`, `eq15_fault_plan_guard`, `eq16_boundary_evidence_guard`, `er12_receipt_aggregation`, `er18_workspace_checkpoint_guard`, `er19_process_handle_guard`, `er20_restart_projector_guard`, `er21_resume_claim_guard`, `er22_cancel_recovery_guard`, `er25_retry_policy_guard`, and `eval_baseline`.
- `kiana-eventlog`, job `110695889200`: all 4 `er02_identity` fixtures passed: `event_id_reuse_is_denied`, `same_request_different_command_digest_conflicts`, `malformed_identity_links_are_denied_before_append`, and the then-existing `idempotent_replay_rejects_identity_link_drift` (parent-link case). The shard also failed `oa06_commit_observer`, `pd08_integrity_scan`, `pd27_writer_queue`, `pd27_writer_queue_guard`, `pd30_storage_fault_matrix`, and `pd31_adapter_conformance`.

These are exact fixture/job results within failed aggregate shards; none of the listed sibling targets is an ER-02 fixture. The current four-field replay matrix is source/test-fixture coverage only and has no CI receipt yet. `feature_status=partial`, `proof_level=source`; direct/Harness/approval-resume identity-chain parity and a clean complete ER-02 shard receipt remain unproven.

## 8. Entrypoint identity fixture and receipt normalization (2026-10-02)

This source slice is based on `origin/master=78a6d79607ed77e8950b0e4509231dc6bf3e2e31`. The CI-only direct fixture `direct_capability_events_keep_request_identity_without_run_scope` asserts the successful canonical `memory.search` request's request/correlation/capability-request IDs and that direct execution facts contain no fabricated Run/Turn identity. The real daemon fixture `wire_approval_proof_retry_resumes_original_run` retains its `Arc<DaemonHost>`, denies an invalid proof before dispatch, approves with a distinct decision-command ID, and checks persisted Run-root request/correlation, Run/Turn/capability-request/derived invocation/execution IDs, approval binding, full successful `project_invocations` output, and exact invocation event IDs. Existing deny-first fixtures remain `foreign_runner_capability_request_is_unknown_before_broker` and `cross_run_result_cannot_pair_by_sequence`. The unified CI map already includes `er09_invocation_projection`, the `kiana-core` control_plane target, and the full `kiana-daemon` crate; no shard/workflow change is needed.

`terminal_signature` now uses the validated `CapabilityResultReceipt` business digest and lifecycle fields to compare dispatch and capability finalizer terminal facts while excluding execution/invocation/receipt-digest boundary differences. It rejects receipts not marked committed for both execution and capability terminal events, verifies request identity, validates `execution.result_committed` receipt IDs/attempt and recomputes its digest from the actual `CapabilityResult`, and checks top-level `effect_known` plus an explicit `outcome_state` against receipt dimensions because `terminal_state` consumes those fields. Capability terminal kind, success/dimensions, attempt and lifecycle flags are also checked. Receipt-less legacy events retain the previous normalized-output fallback. ER-09 CI-only fixtures cover matching cross-envelope receipts, valid conflicting digests, result/identity mismatch, execution effect-known/outcome-state drift, uncommitted terminal receipts, and capability lifecycle drift.

The latest readable pre-fixture receipt is run `36963672912`, head `ff3fb2fe0b9f5ea68d795070dd471b5c3ea3cf80`, cancelled after the next push. ER-02 targets passed inside its failed aggregate jobs: domain job `110702711128` passed all 3 domain fixtures; core job `110702711140` passed both core fixtures; eventlog job `110702710970` passed all 7 identity fixtures, including the four single-field replay drift cases. Sibling failures were:

- Domain: `co08_replay`, `co11_criteria_coverage`, `co17_company_handoff`, `co29_rework_contract`, `connector_conformance`, `connector_notifications`, `cp12_resource_lease`, `cp15_cancellation`, `dep08_deployment_config`, `dep11_health_aggregation`, `dep16_reconcile`, `dep17_capacity`, `dep18_incident`, `dep19_backup_manifest`, `dep20_quiesce_gate`, `dep21_backup_lifecycle`, `dep22_restore_quarantine`, `dep27_migration_registry`, `dep39_supply_chain_release_evidence`, `dep40_release_uat_evidence`.
- Core: `cp26_decision_trace_guard`, `cp27_nonblocking_limits_guard`, `cp28_migration_boundary_guard`, `dep00_deployment_guard`, `dep23_restore_activation`, `dep25_effect_reconcile_route`, `dep25_effect_reconciliation`, `dep26_retention_deletion`, `dep27_migration_registry_guard`, `dep28_migration_preflight_guard`, `dep29_migration_primitives_guard`, `dep30_migration_runner_guard`, `dep31_migration_rebuild_guard`, `dep32_migration_rollback_guard`, `dep33_revision_compatibility_guard`, `dep34_release_preflight_guard`, `dep36_container_adapter_guard`, `dep37_orchestrated_rollout_guard`, `dep38_rollout_lifecycle_guard`, `dep39_supply_chain_guard`, `dep41_release_gate_guard`, `dependency_boundaries`, `eq09_eval_runtime_guard`, `eq10_fake_provider_guard`, `eq11_deny_broker_guard`, `eq13_initial_state_guard`, `eq14_evidence_capture_guard`, `eq15_fault_plan_guard`, `eq16_boundary_evidence_guard`, `er12_receipt_aggregation`, `er18_workspace_checkpoint_guard`, `er19_process_handle_guard`, `er20_restart_projector_guard`, `er21_resume_claim_guard`, `er22_cancel_recovery_guard`, `er25_retry_policy_guard`, `eval_baseline`.
- Eventlog: `oa06_commit_observer`, `pd08_integrity_scan`, `pd27_writer_queue`, `pd27_writer_queue_guard`, `pd30_storage_fault_matrix`, `pd31_adapter_conformance`.

These sibling failures are outside the ER-02 fixtures. The new direct, receipt-normalization and full daemon projection fixtures have no CI receipt yet. ER-02 remains `feature_status=partial`, `proof_level=source`; no updated code has a GitHub CI receipt and no complete green ER-02 shard is claimed.

## 9. Receipt fix integration reconciliation (2026-10-02)

The source/fixture slice in §8 was committed as `4ce549ba82ea08244c0f30343b992adaefc9016f` and merged into master by `e70cf9fc`; `10d24378` records its CI rustfmt correction. Current source CI head `4e54356b06817817f120e3a9a2d9c6c601d4f2b3` contains the `committed=false` terminal receipt denial and `invocation_projection_rejects_uncommitted_terminal_receipts`, covering both `execution.result_committed` and `capability.completed`. The worktree previously reported as containing six uncommitted files was already clean at 4ce549ba; no implementation port or duplicate production commit was needed. This documentation-only reconciliation is committed as `cb99a9f5` and pushed to master.

CI run `36978883309` targets source head `4e54356b` and remained in progress when this note was recorded. Core-s3/6 job `110748754394`, domain-s2/4 job `110748754448`, and eventlog job `110748754494` were marked failed at aggregate level; GitHub did not expose their logs until the overall run finishes, so this note does not infer any fixture outcome from those statuses. The last readable per-fixture receipt remains run `36963672912`, which predates §8. ER-02 remains `feature_status=partial`, `proof_level=source`; no complete green shard or new fixture receipt is claimed.

## 10. Current identity-target receipts (2026-10-02)

Run `36999753811` targets HEAD `491e6bd78b9527b0cc840f8f3aa615d73eb7c6a0`. Exact ER-02 fixture output is now readable for three relevant jobs:

- Domain-s2/4 job `110814891004`: the 3 `er02_identity` fixtures passed: `journal_frames_reject_identity_link_drift`, `legacy_events_without_links_remain_readable_but_self_links_fail_closed`, and `new_events_have_request_correlation_and_explicit_links_round_trip`.
- Core-s3/6 job `110814891024`: `cross_run_result_cannot_pair_by_sequence`, `event_identity_links_and_projection_use_stable_ids_not_request_sequence`, `invocation_projection_normalizes_and_validates_terminal_result_receipts`, and `invocation_projection_rejects_uncommitted_terminal_receipts` passed.
- Eventlog job `110814890987`: all 7 identity fixtures passed: global event ID reuse, same-request command digest conflict, malformed-link denial, and independent command/correlation/causation/parent replay-drift denials.

At capture time, core-s2/6 job `110814890945` and daemon job `110814891133` were still in progress, so the direct ControlPlane and Harness approval-resume parity fixtures have no receipt in this entry. The three completed jobs failed at aggregate level due other targets; these exact ER-02 target results do not establish green shards. `feature_status=partial`, `proof_level=source`; a complete green ER-02 shard and both remaining entrypoint receipts are still unproven.

## 11. Approval-stage redaction root cause (2026-10-03)

Run `37025517103`, HEAD `b87becc90fa5dd77951f2fc13f0b7899c4afff7e`, daemon job `110899633464`, failed three approval-entry fixtures at their initial `AwaitingApproval` assertions with `port_failed:approval_preview_invalid`: `cancel_after_awaiting_approval_rejects_later_approve` (`daemon_host.rs:1388`), `trusted_read_only_apply_patch_does_not_create_file` (`daemon_host.rs:1366`), and `wire_approval_proof_retry_resumes_original_run` (`daemon_host.rs:1453`). The same job's failure output confirms the stable error on all three; these are failures in the pre-existing stage path, not only in the new identity assertions.

Source tracing identifies the cause: Harness ControlPlane preparation adds the server-derived `ExecutionScope` to the capability request before approval staging. `ExecutionScope.fencing_token` is a required `u64`, while generic redaction intentionally treats every field containing `token` as sensitive, so strict typed preview deserialization rejected its redacted string representation. The fix adds a shared approval-display helper that clones the typed request, removes only its server-owned `execution_scope`, then applies the unchanged generic redactor and Secret preview special case. Original payload digests still bind the full request; requests carrying execution scope use `VolatileProtected` material. `ApprovalPlanPreview` and `JournalApprovalStore::stage` share the helper. Domain CI fixtures assert generic event/log redaction still masks numeric fencing tokens, approval display requests omit scope and round-trip as typed requests, argument sentinels remain redacted, and string fencing-token values remain redacted.

No approval precondition or typed validation was relaxed, and generic event/log redaction semantics are unchanged. Unified CI already routes the domain `oa03_redaction` fixture and the full daemon crate; this correction has not yet been pushed or run in CI. ER-02 remains `feature_status=partial`, `proof_level=source`; the direct identity fixture receipt and successful Harness identity projection receipt remain outstanding.

## 12. Stable authority revision across repeated request contexts (2026-10-03)

Run `37039497859`, daemon job `110946337061`, showed that a second effectful request in the same
project/session was rejected with `port_conflict:session_assignment_mismatch`. The daemon had added
`SecurityContext.context_digest` to the project configuration revision. That digest includes a new
`SecurityContextId` per request as well as the current authority epoch and a policy revision derived
from the preceding authority revision, so it cannot identify stable project configuration.

The configuration revision now uses only stable project inputs already enumerated at the daemon
boundary: project identity, role/department catalogs, local role allowlist, model profile config,
policy revision label, tool catalog, and capability action catalog. Trust remains a separate input
to `synchronize_authority`; request identity and request-scoped context digests remain available for
their own validation and evidence bindings. The existing `wire_approval_proof_retry_resumes_original_run`
Harness fixture asserts that one initial revision remains the only `authority.revised` event across
the initial run, rejected proof, and approved continuation, and that the session has one assignment.
Its failed-response assertion now prints the full response for the next CI receipt.

This source correction is pending GitHub CI. ER-02 stays partial/source until the exact approval,
cancel, direct identity and full shard results are reviewed; no durable recovery or external/live
effect is claimed.

## 13. Legacy history deduplication keeps request identity (2026-10-04)

The model-visible history fold previously used `(run_id, None, sequence)` for legacy events that
lacked stream metadata. Because `sequence` is request-local, two requests in one run could share a
sequence and the later event would be silently dropped. The fold now always includes the event's
`request_id` and uses `stream_version` only when present, preserving distinct legacy events while
leaving modern aggregate ordering unchanged. The Core regression fixture covers two requests with
the same legacy sequence, and the ER-02 source guard pins the request-aware deduplication marker.

```text
source_snapshot: parent `d73aca7c`; source `0b007905`; kiana-core/src/history.rs; kiana-core/tests/control_plane.rs; kiana-core/tests/er02_identity_guard.rs
worktree_status: legacy history deduplication key is `(run_id, request_id, stream_version.unwrap_or(sequence))`; no EventStore, authorization or second execution path changed
command_argv: isolated git diff --check; isolated git diff --cached --check; isolated cargo fmt --all --check; root cherry-pick `ad65d20f`; push-triggered `.github/workflows/ci.yml` includes `er02_identity` and `er02_identity_guard`; no local test/build/check/clippy/smoke
cwd·environment: repository root and isolated worktree `/tmp/kiana-er02-legacy-sequence-20261004`; Linux/bash; GitHub Actions is the runtime test authority
fixture·cassette: `history_same_legacy_sequence_keeps_distinct_requests` plus `event_identity_links_and_projection_use_stable_ids_not_request_sequence` source guard; changed-source workflow pending
exit_code: isolated format/diff checks 0; changed-source CI pending; no local runtime exit code
status_change: ER-02 roadmap row 037 remains `🔄`; feature_status=partial; proof_level=source
proof-level_change: none; no local_behavior, durable, live or physical promotion
limitations: this only corrects history-fold deduplication; full legacy upcast, durable InvocationLedger, cross-process ordering, terminal reconciliation and external effect receipts remain open
reviewer: source review of request-aware legacy deduplication and deny-first regression fixture; no local runtime reviewer
```

## 14. Legacy history sort-key compile correction (2026-10-04)

The first changed-source push exposed a compile regression in the legacy-history slice:
`event_order` was computed while constructing the deduplication key but was also referenced from
the later sort closure, where it was out of scope. The sort closure now recomputes the same
request-local/stream-version order key. This restores compilation without changing the
request-aware deduplication boundary or modern ordering semantics.

```text
source_snapshot: parent `1f1a1a0a`; failing CI head `30a0abbc`; correction `890d9585`; kiana-core/src/history.rs
worktree_status: sort closure recomputes `stream_version.unwrap_or(sequence)` locally; deduplication still includes `request_id`; no EventStore, authorization or execution-path change
command_argv: remote `gh run view 37171766266 --job 111345974212 --log-failed`; targeted rustfmt; `git diff --check`; push-triggered `.github/workflows/ci.yml`; no local test/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; GitHub Actions is the runtime test authority
fixture·cassette: run `37171766266`, PD-01 daemon fixture compile reached `kiana-core/src/history.rs:256` and failed E0425 for `event_order`; correction pushed as `890d9585`; replacement CI run `37172404477` pending
exit_code: remote compile failure observed at the old head; targeted format/diff checks 0; replacement CI pending; no local runtime exit code
status_change: ER-02 roadmap row 037 remains `🔄`; feature_status=partial; proof_level=source
proof-level_change: none; the correction only removes a compile blocker and does not establish runtime behavior
limitations: full legacy migration/upcast, durable InvocationLedger, cross-process ordering, terminal/result reconciliation and external effect receipts remain open; no CI success is claimed for the correction yet
reviewer: source review of the remote compiler diagnostic and minimal sort-key restoration; no local runtime reviewer
```

## 15. Malformed identity aliases and nil-link rejection (2026-10-04)

The invocation projector now validates each event's identity links before folding facts and fails closed on malformed self-links. Request identity candidates from capability, run, approval, and permit payload aliases must agree; conflicting aliases are not resolved by first-match precedence. Domain and EventLog fixtures also reject nil command/correlation/causation/parent IDs while preserving the optional-link compatibility path for legacy events that omit them.

```text
source_snapshot: `b980a180`; kiana-domain/src/states.rs; kiana-domain/tests/er02_identity.rs; kiana-core/src/invocation_projection.rs; kiana-core/tests/er02_identity.rs; kiana-core/tests/er02_identity_guard.rs; kiana-eventlog/tests/er02_identity.rs
worktree_status: projection identity-link validation and request-alias agreement are source-enforced; nil identity links are denied before append; no authorization, EventStore, or second execution path changed
command_argv: source review; `git diff --check`; staged diff check; after push changed-source `.github/workflows/ci.yml`; no local test/build/check/clippy/smoke
cwd·environment: repository root Linux/bash; GitHub Actions Ubuntu runner is the runtime test authority; local runtime commands intentionally not run
fixture·cassette: `invocation_projection_rejects_malformed_identity_links`, `invocation_projection_rejects_request_id_alias_drift`, `nil_identity_links_fail_closed_but_legacy_links_remain_optional`, `nil_identity_links_are_denied_before_append`; changed-source CI pending after push
exit_code: source/diff checks pending at commit time; GitHub runtime jobs pending/unobserved; no local runtime exit code
status_change: ER-02 roadmap row 037 remains `🔄`; feature_status=partial; proof_level=source
proof-level_change: none; no local_behavior, durable, live or physical promotion
limitations: direct/Harness/approval-resume identity-chain parity, complete green ER-02 shard, full legacy upcast, durable InvocationLedger, cross-process ordering, terminal reconciliation and external effect receipts remain open
reviewer: source review of projection alias handling, identity-link validation order, domain/EventLog append guards and deny-first fixtures; no local runtime reviewer
```

## 16. Current-head DaemonHost approval-resume parity receipt (2026-10-05)

The real `DaemonHost` approval path now has a bounded local receipt for both cancellation
and proof-retry continuation. The proof validator reports request-hash and nonce mismatches
independently. The continuation fixture uses the existing `memory.write` cassette in
`instance-scratch`: the original `apply_patch` cassette correctly reaches `ResultUnknown` under
the read-only sandbox, so using it would test the deny path rather than an approval-resume
success. The fixture selects approval facts from the Run aggregate (the journal stream has a
separate command identity), verifies the redacted `ExecutionScope` boundary, and projects the
complete invocation event stream including `result.delivery_claimed`.

```text
source_snapshot: current dirty worktree; kiana-core/src/approvals.rs; kiana-core/src/invocation_projection.rs; kiana-daemon/tests/daemon_host.rs; ER-02 identity fixtures
worktree_status: ER-02 remains roadmap row 037 🔄; no automatic commit/push/merge/rebase; unrelated WIP preserved
command_argv: bounded serial selectors: `cargo test -p kiana-core --test control_plane capability_policy_and_broker_failures_preserve_event_order`; `direct_capability_events_keep_request_identity_without_run_scope`; `approval_resumes_the_stored_request_once_with_monotonic_events`; `cargo test -p kiana-core --test er02_identity`; `cargo test -p kiana-core --test er02_identity_guard`; `cargo test -p kiana-eventlog --test er02_identity`; `cargo test -p kiana-daemon --test daemon_host approval`, all `--locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`, `RUST_MIN_STACK=33554432`, stripped/non-incremental test profile; serial daemon/control-plane execution; per-target max RSS 554540/85632/85740/330096/132596/83472/712920 KiB; all observed swap counters 0
fixture·cassette: Core broker/direct/approval 1/1 each; Core ER-02 identity 3/3; Core guard 1/1; EventLog ER-02 identity 9/9; DaemonHost approval filter 2/2 (`cancel_after_awaiting_approval_rejects_later_approve`, `wire_approval_proof_retry_resumes_original_run`); invalid request hash denied before dispatch; valid proof resumes the original Run/Turn/request/invocation identity; cancellation returns structured `approval_cancelled`; persisted scope fencing token is `[REDACTED]` and is not reconstructed
exit_code: all seven commands 0
status_change: DaemonHost/Harness identity and approval-resume parity is recorded as focused `local_behavior`; ER-02 whole card remains `feature_status=partial`, roadmap row 037 🔄
proof-level_change: focused Core/EventLog/DaemonHost selectors `source` → `local_behavior`; no durable/live/physical promotion
limitations: no complete green CI shard, full workspace test remains non-green/resource-intensive, legacy upcast and terminal reconciliation remain bounded, durable InvocationLedger/cross-process ordering/restart recovery and external effect receipts remain open
reviewer: integration agent; focused Core/EventLog/DaemonHost identity and approval reviewer
```

## 17. Aggregate stream metadata validation receipt (2026-10-07)

The shared EventLog append validator now rejects partial aggregate metadata and zero stream
versions before registry validation, CAS/conflict checks, idempotent replay, or storage mutation.
Legacy events with all three fields absent remain accepted. The RED resource sample was captured
by the audit agent but its stdout log was not retained; the current-head GREEN logs and resource
reports below are reproducible in the repository.

```text
source_snapshot: current dirty worktree; kiana-eventlog/src/event_store_core.rs and tests/er02_identity.rs
worktree_status: ER-02 remains roadmap row 037 🔄 / feature_status=partial; selected aggregate-metadata rejection is local_behavior; no manifest/lockfile or execution path change
command_argv: RED and GREEN `cargo test -p kiana-eventlog --test er02_identity --locked --offline -- --test-threads=1`; GREEN re-run `cargo test -p kiana-core --test er02_identity_guard --locked --offline -- --test-threads=1`; `git diff --check`; `cargo fmt --all --check`
cwd·environment: repository root Linux/bash; one Cargo job/test thread, 32 MiB stack, stripped/non-incremental test profile; RED agent-reported max RSS 471884 KiB; current EventLog/Core re-run max RSS 83560/85568 KiB; all measured runs 0 swaps
fixture·cassette: RED agent reported the zero/partial metadata denial fixtures failed before fix; the RED total is not asserted because its stdout was not retained. Current GREEN EventLog 12/12 includes version zero, missing type/id, blank type/id, valid positive version and legacy no-metadata cases; Core guard 1/1; current GREEN logs/resources `/tmp/kiana-roadmap-20261007-er02-final/{eventlog,core}.{log,resources}`; only RED resource file remains at `/tmp/kiana-roadmap-20261006-er02-stream-red.resources`
exit_code: RED agent-reported 101; current EventLog/Core GREEN re-runs 0; format/diff 0
status_change: selected EventLog aggregate-order validation is local_behavior; ER-02 whole card remains partial/source
proof-level_change: selected EventLog append boundary source → local_behavior; no durable/live/physical promotion
limitations: no complete ER-02 shard, full workspace green, durable InvocationLedger, cross-process recovery, terminal reconciliation or external effect receipt is established
reviewer: root integration agent; read-only ER-02 audit; local EventLog/Core fixture review
```

## 18. Direct append top-level identity denial (2026-10-08)

Memory and JSONL legacy append planning now rejects nil `RuntimeEvent.event_id` and
`request_id` before idempotency/CAS/index mutation, aligning direct append with the existing
TransitionBatch and JournalFrame identity boundaries.

```text
source_snapshot: current dirty worktree; kiana-eventlog/src/journal_core.rs; kiana-eventlog/tests/er02_identity.rs; kiana-core/tests/er02_identity_guard.rs
worktree_status: ER-02 row 037 remains 🔄 / feature_status=partial / proof_level=source; selected direct-append identity denial now local_behavior; no manifest/lockfile or second execution path
command_argv: RED `cargo test -p kiana-eventlog --test er02_identity nil_top_level_event_identity_is_denied_before_append --locked --offline -- --exact --test-threads=1`; GREEN EventLog `cargo test -p kiana-eventlog --test er02_identity --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test er02_identity_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo job/test thread, `RUST_MIN_STACK=33554432`, stripped/non-incremental tests; compile-only RED correction max RSS 4065232 KiB / 87% CPU; behavior RED max RSS 277280 KiB / 44% CPU; EventLog GREEN max RSS 428204 KiB / 96% CPU; Core guard final rerun max RSS 131820 KiB / 68% CPU; all measured runs reported 0 swaps; logs/resources `/tmp/kiana-roadmap-20261008-er02-{red,red2,eventlog,core}.{log,resources}`
fixture·cassette: RED accepted nil event_id/request_id and failed expected `event_identity_invalid`; GREEN EventLog 13/13 and Core guard 1/1; rejected events leave `read_all()` empty
exit_code: initial RED compile 101 (fixture type correction); behavior RED 101 expected; GREEN EventLog/Core 0
status_change: selected direct append identity denial moves source → local_behavior; whole ER-02 remains feature_status=partial, proof_level=source
proof-level_change: direct append top-level identity source → local_behavior; no durable/live/physical promotion
limitations: complete ER-02 shard, legacy upcast, durable InvocationLedger, cross-process ordering/recovery, terminal reconciliation and external effect receipts remain open
reviewer: root integration agent with ER-02 read-only EventLog audit; focused identity reviewer
```

## 19. Entrypoint-to-DaemonHost identity chain receipt (2026-10-13)

`harness_run::run_envelope_on_host` now has a real local fixture that starts from the public
entrypoint wrapper, builds the paired `KianaClient`/`RequestMetadata`, sends through
`LocalDaemonTransport`, and reads the resulting facts from the same `DaemonHost`. The fixture
checks one request and correlation identity across `run.authorized`, `run.prompt`, `run.completed`,
and `run.receipt`, along with the returned `run_id`. It does not use a synthetic projection or a
second execution path.

```text
source_snapshot: current dirty worktree; kiana-entrypoints/src/harness_run.rs; kiana-entrypoints/src/lib.rs; kiana-daemon/src/lib.rs; docs/roadmap/event-receipt-identity-baseline.md
worktree_status: ER-02 row 037 remains 🔄 / feature_status=partial; the supplied-Host Harness wrapper identity chain is local_behavior; no manifest/lockfile or execution path change
command_argv: `cargo test -p kiana-entrypoints --lib harness_run::tests::run_envelope_on_host_preserves_event_identity_chain --locked --offline -- --exact --test-threads=1`; `cargo test -p kiana-entrypoints --lib --locked --offline --no-fail-fast -- --test-threads=1`; `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; one Cargo job and one Rust test thread; `RUST_MIN_STACK=33554432`; stripped/non-incremental locked/offline test profile; serial launcher rejects overlapping Cargo/rustc/rust-lld
fixture·cassette: trusted temporary project, isolated KIANA_HOME and one scripted text response; identity selector 1/1; full entrypoint library 459/459; each selected run/correlation event and response share the returned request_id and run_id
exit_code: identity 0; entrypoint library 0; fmt/diff 0; identity max RSS 4060016 KiB / 89% CPU / 0 swap; library max RSS 4071508 KiB / 85% CPU / 0 swap
status_change: supplied-Host entrypoint identity boundary moves source → local_behavior; ER-02 remains partial and row 037 stays 🔄
proof-level_change: wrapper-to-local-DaemonHost behavior is locally verified; no durable/live/physical promotion
limitations: this fixture uses the in-memory Host EventStore and does not cover pending approval restart, run_owned_harness fresh-Host identity, durable InvocationLedger, cross-process ordering/recovery, complete green ER-02 shard or external effect receipts
reviewer: root integration agent with read-only ER-02 entrypoint audit; focused Harness identity reviewer
```

## 20. Legacy invocation append-order fence (2026-10-13)

Invocation projection no longer treats legacy events with absent aggregate metadata as one
comparable stream. Sorting by `stream_version` is now limited to inputs whose aggregate type, ID,
and stream version are all present and consistent; legacy or mixed inputs retain EventLog append
order, so request-local `sequence` cannot reorder facts across requests.

```text
source_snapshot: current dirty worktree; kiana-core/src/invocation_projection.rs; kiana-core/tests/er02_identity.rs
worktree_status: ER-02 row 037 remains 🔄 / feature_status=partial; selected legacy append-order behavior is local_behavior; no durable ledger, manifest/lockfile or second execution path
command_argv: RED `cargo test -p kiana-core --test er02_identity legacy_invocation_events_preserve_append_order_without_stream_metadata --locked --offline -- --exact --test-threads=1`; GREEN same selector; full `er02_identity`; `er02_identity_guard`; `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; one Cargo/compiler job and one Rust test thread, `RUST_MIN_STACK=33554432`, stripped/non-incremental locked/offline profile; RED max RSS 4100792 KiB / 92% CPU, GREEN 331344 KiB / 105% CPU, full identity 84696 KiB / 28% CPU, guard 135744 KiB / 55% CPU, all observed swap counters 0; receipts under `/tmp/kiana-roadmap-20261010-evidence/er02-legacy-order-{red2,green2}.*` and `er02-{identity-4,guard}.*`
fixture·cassette: five legacy `RuntimeEvent::new` facts for one run and request, including a legal requested→decision→prepared→executing→completed path; append order intentionally conflicts with request-local sequence; RED returned `invocation_request_missing`, GREEN projects `Succeeded`; full identity 4/4 and guard 1/1
exit_code: RED 101 expected; exact GREEN/full/guard 0; `cargo fmt --all --check`, `git diff --check` and workflow validator 0
status_change: legacy invocation append-order protection moves source → local_behavior; whole ER-02 remains feature_status=partial, proof_level=source
proof-level_change: in-memory legacy projector ordering source → local_behavior; no durable/live/physical promotion
limitations: durable InvocationLedger, cross-process ordering/recovery, terminal reconciliation, complete CI shard, legacy upcast coverage and external effect receipts remain open
reviewer: root integration agent with ER-02 read-only projection audit; focused legacy ordering reviewer
```
