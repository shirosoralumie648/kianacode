# ER-29 Data governance、retention 和 deletion propagation 基线

> 快照日期：2026-09-24。ER-29 的治理传播契约、失效边界和拒绝夹具由 GitHub Actions 执行；本地不运行测试、构建或检查。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-29`](event-receipt-recovery.md#step-er-29) |
| feature_status | `implemented`（source-level receipt binding、epoch/tombstone propagation、artifact/memory/index/cache fences、immutable event seal） |
| proof_level | `source`；不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | committed EventLog facts → governance snapshot → deletion/tombstone/data epoch → bounded propagation plan → target receipt → read projection |

Receipt 的审计 metadata（project、policy revision、source cursor/event IDs、redaction profile）与 payload refs（对象和内容 digest）是两个独立字段。`redact_payload_refs` 只改变展示引用，`authorize_payload` 仍要求 payload 为 `Available` 且 data epoch 与当前策略一致，因此 receipt redaction 不能替代授权。

撤销、过期和删除都先追加不可变 tombstone/propagation 事实；EventLog 源事件不会被修改或删除。Artifact store、Memory/Index/Cache boundary 以 data epoch 和 tombstone digest 拒绝旧数据；目标回执缺失时保持 `Unknown`。`MemoryDataGovernanceStore` 和 `MemoryRetentionStore` 是 CI 语义适配器，不能宣称跨进程持久删除。

## 2. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `receipt_metadata_and_payload_refs_are_separate_and_epoch_fenced` | metadata 与 payload refs 分离；旧 epoch 和空 refs 不能授权 payload |
| `propagation_plan_preserves_event_facts_and_starts_unknown` | EventProjection/Audit 只保留 metadata，Memory/Index/Cache 等目标初始 Unknown |
| `artifact_invalidation_denies_reads_and_replays_by_tombstone` | artifact revoke/expire 后读取 fail closed；同 tombstone 重放幂等 |
| `memory_propagation_requires_plan_before_target_receipt` | 目标回执必须先有 propagation plan，旧 epoch 不可读 |
| `revoked_or_stale_index_is_not_an_empty_success` | stale/revoked index 明确拒绝，不伪装成空查询成功 |
| `er29_separates_receipt_metadata_from_payload_and_fences_every_store` | source guard 保持单一 EventLog 事实、无第二执行循环和无 redaction 授权旁路 |

## 3. Durable boundary and limitations

本切片只提供 typed、digest-bound、可重放的 source contract 与内存适配器边界。EventLog 仍是事实来源；JSONL/跨进程 projector、物理 artifact/memory 擦除、备份副本删除、索引重建 worker、真实合规证明和外部/live effect 不在本步骤。`ImmutableEventSeal` 仅表达既定密级/封存边界，不实现加密密钥管理。Propagation receipt 未确认的 target 继续为 `Unknown`，不能提升为删除完成。

## 4. Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

