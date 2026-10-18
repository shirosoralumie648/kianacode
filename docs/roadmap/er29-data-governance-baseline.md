# ER-29 Data governance、retention 和 deletion propagation 基线

> 快照日期：2026-09-24，2026-10-19 补完成切片。ER-29 的治理传播契约、失效边界和拒绝夹具由 GitHub Actions 执行；本切片在本地离线运行了七个聚焦目标（target）（见 §6）。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-29`](event-receipt-recovery.md#step-er-29) |
| feature_status | `implemented`（source-level receipt binding、epoch/tombstone propagation、artifact/memory/index/cache fences、immutable event seal、typed digest-bound reconciliation evidence） |
| proof_level | `local_behavior`（仅限 §2 矩阵中的聚焦 target 本地实跑通过）；不提升为 `durable`、`live` 或 `physical` |
| canonical path | committed EventLog facts → governance snapshot → deletion/tombstone/data epoch → bounded propagation plan → target receipt → read projection |

Receipt 的审计 metadata（project、policy revision、source cursor/event IDs、redaction profile）与 payload refs（对象和内容 digest）是两个独立字段。`redact_payload_refs` 只改变展示引用，`authorize_payload` 仍要求 payload 为 `Available` 且 data epoch 与当前策略一致，因此 receipt redaction 不能替代授权。

撤销、过期和删除都先追加不可变 tombstone/propagation 事实；EventLog 源事件不会被修改或删除。Artifact store、Memory/Index/Cache boundary 以 data epoch 和 tombstone digest 拒绝旧数据；目标回执缺失时保持 `Unknown`。`MemoryDataGovernanceStore` 和 `MemoryRetentionStore` 是 CI 语义适配器，不能宣称跨进程持久删除。

## 2. Failure-first fixture matrix

先拒绝在前；每个拒绝都断言稳定拒绝码。

| Fixture | Target | 断言 |
|---|---|---|
| `receipt_metadata_and_payload_refs_are_separate_and_epoch_fenced` | `kiana-domain/tests/er29_data_governance.rs` | metadata 与 payload refs 分离；旧 epoch 和空 refs 不能授权 payload |
| `propagation_plan_preserves_event_facts_and_starts_unknown` | 同上 | EventProjection/Audit 只保留 metadata，Memory/Index/Cache 等目标初始 Unknown |
| `immutable_event_seal_rejects_duplicate_source_ids` | 同上 | 封存事实拒绝重复 source id，不能借封存改写历史 |
| `revoked_data_is_not_returned_by_receipt` | `kiana-core/tests/er29_data_governance.rs` | 已撤销 artifact 的 receipt 即使本地标记 `Available`，旧 epoch 仍 `receipt_payload_authorization_denied`；redaction 不授权 |
| `deletion_does_not_leave_untracked_projection_copy` | 同上 | 外项目删除事实不进入本索引；本项目的 tombstone 在四个检索面都拒绝旧 revision |
| `receipt_binding_keeps_payload_unknown_without_governance_authorization` | 同上 | 无治理授权时 payload 保持 `Unknown` |
| `governance_event_seal_has_no_payload_mutation_path` | 同上 | seal 只表达密级/封存边界，无 payload 改写面 |
| `artifact_invalidation_denies_reads_and_replays_by_tombstone` | `kiana-eventlog/tests/er29_data_propagation_store.rs` | artifact revoke/expire 后读取 fail closed；同 tombstone 重放幂等 |
| `memory_propagation_requires_plan_before_target_receipt` | 同上 | 目标回执必须先有 propagation plan，旧 epoch 不可读 |
| `retention_store_source_mentions_manifest_before_propagation` | 同上 | retention 适配器先持久化 manifest，再传播 |
| `revoked_or_stale_index_is_not_an_empty_success` | `kiana-query/tests/er29_data_governance.rs` | stale/revoked index 明确拒绝，不伪装成空查询成功 |
| `governance_plan_keeps_index_unknown_until_rebuild_receipt` | 同上 | 重建回执缺失时索引保持 `Unknown` |
| `revoked_artifact_is_not_returned` | `kiana-daemon/tests/er29_artifact_invalidation.rs` | 生产 artifact store 的撤销读取 fail closed |
| `invalidation_replay_is_idempotent_and_conflicts_fail_closed` | 同上 | 同一 tombstone 重放幂等；冲突的 invalidation 拒绝 |
| `invalidation_boundary_is_validated` | 同上 | 空/超界/坏 digest/坏 epoch 拒绝 |
| `invalidation_requires_a_committed_version` | 同上 | 未提交版本不能撤销 |
| `revocation_survives_reopen_and_blocks_late_commit` | 同上 | 撤销 journal 重开进程后仍生效；迟到 commit 被拒 |
| `caller_authored_provider_label_is_not_reconciliation_evidence` | `kiana-core/tests/er29_reconciliation_evidence.rs` | `provider:`/`os:`/`file:`/`human:` 自报标签 → `failure_reconciliation_evidence_unbound` |
| `an_event_reference_without_a_digest_or_with_a_wrong_digest_is_refused` | 同上 | 仅 id、错误观测 digest、错误源 digest → `unbound` |
| `evidence_must_be_owned_current_and_observational` | 同上 | 他人 run、早于源事件、`*.result_unknown` 复述 → `unbound`；缺源引用 → `required` |
| `a_digest_bound_owned_observation_reconciles_once_and_replays` | 同上 | 合法事件+digest 对账一次并幂等重放，原 run 结果不变 |
| `er29_separates_receipt_metadata_from_payload_and_fences_every_store` | `kiana-core/tests/er29_data_governance_guard.rs` | source guard：单一 EventLog 事实、无第二执行循环、无 redaction 授权旁路 |
| `er29_reconciliation_evidence_is_typed_digest_bound_and_incident_scoped` | 同上 | source guard：旧前缀路径已删除，digest/ownership/ordering/observation-kind 检查在位，拒绝夹具先于成功夹具 |

## 3. 完成切片（2026-10-19）

1. **Artifact store 围栏**：`LocalArtifactStore` 增加追加式 invalidation journal（epoch、tombstone digest、幂等 replay、冲突拒绝）。撤销在 `open` 重开后仍生效，迟到 commit 被拒。
2. **Memory 删除索引跨项目泄漏修复**：`project_memory_deletion_index` 通过 `memory_fact_matches_project` 过滤掉其他项目根的事实；home-scoped collection（company/user）按设计豁免；空 root 视为无归属历史保留。
3. **对账证据改为 typed/digest-bound**：删除 `provider:`/`os:`/`file:`/`human:` 前缀旁路（那只是调用方自报标签）。现在只接受 `event:<id>[#sha256:<hex>]`，独立证据必须已提交、属于本 actor/项目、位于源事件之后、绑定 incident run（无 run 时同 request）、属于结果类观测，且 payload digest 完全匹配。返回 `ReconciliationEvidenceVerdict::{Accepted,Required,Unbound}`，分别映射 `failure_reconciliation_evidence_unbound` 与 `failure_reconciliation_evidence_required`。
4. **Cell retirement 可投影撤销：未实施（明确缺口）**。见 §4。
5. **补齐两个命名拒绝夹具**：`revoked_data_is_not_returned_by_receipt`、`deletion_does_not_leave_untracked_projection_copy`。

## 4. Cell retirement 可投影撤销为何仍未接线

评审记录指出 retire 没有追加 `grant.revoked` / `lease.fenced` 这类 server-owned 事实，`authority_read_model` 可能继续把已退役 Cell 的 grant 当作 active。本切片核对了实际投影规则，结论是**在当前事实源下不能安全接线**：

- `authority_read_model::epoch` 对每个 `cell.*` / `grant.*` / `budget.*` / `lease.*` 事件要求非零 `authority_epoch`；缺失或 0 直接 `EpochStale`，整个投影失败。
- `cell.retired` 的载荷由 `retirement_event_payload` 生成，只有 `cell_id`/`grant_id`/两个 lease id/`released_paths`/`reason`/时间戳，没有 `authority_epoch`；`RetirementRecord` 本身也不携带世代。
- `lease.fenced` 要求同一 lease id 先出现 `lease.issued`，否则投影以 `LeaseInvalid("lease_without_issue")` 失败；仓库中没有生产方追加 `lease.issued`。
- `grant.revoked` 分支只按 `grant_id` 折叠状态，不校验 Cell 关联；在没有签发事实的情况下追加撤销，会得到「撤销一条从未投影过的 grant」这种无法回解的记录。

因此若为了「看起来可投影」而补发这些事件，只会得到要么整表折叠失败、要么凭空发明签发事实的结果——两者都比当前明确的缺口更不诚实。安全的接线需要先有生产方的授权世代来源与租约签发事实；这属于 `authority_read_model` / 事件契约的上游工作，不是 `release_resources` 能补的。该缺口保留在 §5。

## 5. Durable boundary and limitations

本切片提供 typed、digest-bound、可重放的 source contract 与内存/本地文件适配器边界。EventLog 仍是事实来源；JSONL/跨进程 projector、物理 artifact/memory 擦除、备份副本删除、索引重建 worker、真实合规证明和外部/live effect 不在本步骤。`ImmutableEventSeal` 仅表达既定密级/封存边界，不实现加密密钥管理。Propagation receipt 未确认的 target 继续为 `Unknown`，不能提升为删除完成。Cell 退役仍不能投影为 `grant.revoked`/`lease.fenced`（§4）。`failure.recovery` 路径的 run-bound evidence 未扩展（本步只收紧 `failure.reconcile`）。

## 6. Evidence block

```text
source_snapshot: branch step/er29-20261018-1, worktree D:/code/kiana-steps/er29; kiana-core/src/platform.rs, kiana-core/src/data_governance.rs, kiana-daemon/src/local_artifacts.rs, kiana-core/tests/er29_reconciliation_evidence.rs, kiana-core/tests/er29_data_governance.rs, kiana-core/tests/er29_data_governance_guard.rs, kiana-daemon/tests/er29_artifact_invalidation.rs
worktree_status: roadmap row 585 remains 🔄; feature_status=implemented (bounded local contract); proof_level=local_behavior for the focused targets only
command_argv: export CARGO_TARGET_DIR=D:/code/kiana-steps/target-er29; CARGO_BUILD_JOBS=4 RUST_MIN_STACK=33554432 cargo test -p <crate> --test <target> --offline -- --test-threads=1 (domain er29_data_governance, core er29_data_governance, core er29_reconciliation_evidence, core er29_data_governance_guard, daemon er29_artifact_invalidation, eventlog er29_data_propagation_store, query er29_data_governance); rustfmt --edition 2021 on touched files
cwd·environment: Windows 11 / Git Bash; offline; single test thread; no cargo test --workspace, no clippy, no smoke scripts, no remote CI receipt
fixture·cassette: deny-first; see §2 matrix (23 fixtures across 7 targets); all synthetic in-memory/temp-dir data, no provider/network/live effects
exit_code: 0 for all seven focused targets (domain 3, core 4, core reconciliation 4, core guard 2, daemon 5, eventlog 3, query 2)
status_change: reconciliation evidence is now typed/digest-bound; artifact invalidation is journal-persisted; memory deletion index is project-scoped; roadmap row 585 unchanged because Cell retirement projectability and durable/cross-process proof are missing
proof-level_change: source → local_behavior for the focused targets; no durable/live/physical promotion
limitations: see §5; Cell retire cannot emit projectable grant.revoked/lease.fenced without issuance facts and an authority epoch source
reviewer: this slice's agent; no independent runtime review
```
