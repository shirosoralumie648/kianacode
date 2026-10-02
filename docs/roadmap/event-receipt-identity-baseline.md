# ER-02 identity, correlation, causation, and ordering baseline

> 快照日期：2026-09-16。本文记录 ER-02 的稳定 ID 关联和 legacy 兼容边界；本地不运行测试，运行时夹具仅在 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-02`](event-receipt-recovery.md#step-er-02) |
| source snapshot | `863b30f`（ER-01 event registry 后的干净基线） |
| feature_status | `partial`（identity-link, EventStore, and ID-aware projection source slices exist; full entrypoint parity and a clean complete ER-02 CI receipt remain unproven） |
| proof_level | `source`；静态编译不提升为 local_behavior/durable/live/physical |
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

EventLog 的 aggregate stream version 是顺序权威；request-local `sequence` 只用于一个请求中的展示/兼容排序。跨请求、跨 run 的结果必须携带 `run_id`/`capability_request_id`/invocation or execution IDs，projection 先校验 run binding，再折叠 terminal/result。

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
| `cross_run_result_cannot_pair_by_sequence` | 同 sequence/同 capability ID 的 foreign run result 不配对 |
| `event_identity_links_and_projection_use_stable_ids_not_request_sequence` | domain/core source guard |
| `direct_capability_events_keep_request_identity_without_run_scope` | direct 成功事件保持 request/correlation/capability request ID，不合成 Run/Turn |
| `wire_approval_proof_retry_resumes_original_run` | 真实 DaemonHost/JournalApprovalStore Harness approval-resume 保留 Run root、Turn、capability request 与 invocation IDs，并核对完整 invocation projection/event IDs |
| `invocation_projection_normalizes_and_validates_terminal_result_receipts` | dispatch/finalizer receipt 忽略边界 ID 差异、校验实际结果 digest 和 identity，并拒绝 digest/request/execution/effect-known/outcome/lifecycle drift |
| `invocation_projection_rejects_uncommitted_terminal_receipts` | 拒绝 `committed=false` 的 `execution.result_committed` 与 `capability.completed` receipt |

统一 `.github/workflows/ci.yml` 按 `scripts/ci/test-shards.json` 执行 domain/core ER-02 targets 和完整 `kiana-eventlog` crate；本地不运行测试，也不等待 CI。

新增 EventStore identity-boundary fixtures 随 `kiana-eventlog` crate target 执行，core
source guard 锁定 storage validator 与幂等 replay 的 identity-link 比较。拒绝路径仍只在
GitHub Actions 验证。

## 6. 限制与交接

- RuntimeEvent identity links 是 optional additive fields；现有大量 legacy events 尚未强制携带 schema/version/command/causation/parent，完整 upcaster 和 writer gate 属于后续 ER/CP-28。
- correlation/trace/span 只做关联和审计投影，不是 authenticated principal、grant、approval 或 permit；外部 traceparent/ID 不能扩权。
- EventStore CAS/duplicate guard 在内存/JSONL 适配器有 source/局部行为证据，但不证明掉电、网络文件系统、跨主机或外部 effect exactly-once。
- Projection 过滤能阻止跨 run 错配，但完整 InvocationLedger、attempt retry/reconcile、result delivery、retention/delete 和 Receipt correctness 仍需 ER-03+、CP-07+、PD/SC。
- 本地只做格式、workspace test-target 静态编译和 diff 检查；不提升 local_behavior/durable/live/physical。

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

The source/fixture slice in §8 was committed as `4ce549ba82ea08244c0f30343b992adaefc9016f` and merged into master by `e70cf9fc`; `10d24378` records its CI rustfmt correction. Current master `4e54356b06817817f120e3a9a2d9c6c601d4f2b3` contains the `committed=false` terminal receipt denial and `invocation_projection_rejects_uncommitted_terminal_receipts`, covering both `execution.result_committed` and `capability.completed`. The worktree previously reported as containing six uncommitted files was already clean at 4ce549ba; no implementation port or duplicate production commit is needed.

Current master CI run `36978883309` targets `4e54356b` and was still in progress when this note was recorded. Core-s3/6 job `110748754394`, domain-s2/4 job `110748754448`, and eventlog job `110748754494` were marked failed at aggregate level; GitHub did not expose their logs until the overall run finishes, so this note does not infer any fixture outcome from those statuses. The last readable per-fixture receipt remains run `36963672912`, which predates §8. ER-02 remains `feature_status=partial`, `proof_level=source`; no complete green shard or new fixture receipt is claimed.
