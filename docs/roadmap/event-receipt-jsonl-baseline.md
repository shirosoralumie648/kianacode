# ER-05 JSONL v2 原子 frame、锁与损坏策略基线

> 快照日期：2026-09-17。本页记录 EventLog 的 source/CI 边界；不把一次本地文件同步
> 证明外推为掉电、网络文件系统或跨主机一致性证明。

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-05`](event-receipt-recovery.md#step-er-05) |
| feature_status | `implemented`（JSONL v2 header/frame、锁、CAS cursor、损坏分类） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 eventlog fixtures |
| authority | `kiana-eventlog::JsonlEventLog` + `kiana-domain::JournalFrame`；Memory adapter 只作兼容合同 |
| this step does | required writer header、checksummed bounded frame、完整 transaction page、flock/dirfd/no-follow、write/flush/sync/identity boundary、legacy upgrade gate、torn-tail repair 与 malformed/checksum fail-closed |
| this step does not | 不声称 power-loss、NFS/跨主机、SQLite、备份/恢复或外部 effect exactly-once；异步 worker/backpressure/shutdown ack 留待 ER-06 |

## 1. Contract

`kiana-domain::JournalHeader` 只接受当前 required writer version；`JournalFrame` 将完整
`TransitionBatch`/`CommandReceipt` 或兼容单事件放入 strict `kiana.transition-frame.v1`，
绑定 canonical body length 和 SHA-256。frame 超限、body digest/shape/receipt/read-set/stream
version 不一致均 fail-closed；`logical_events` 只在整 frame 验证后展开。

`JsonlEventLog` 在 Unix 上以固定 parent dirfd、`openat(O_NOFOLLOW|O_CLOEXEC)` 和单一
`flock` 覆盖刷新、CAS/read-set、写入、flush、file/parent sync、identity 验证和 cursor 发布。
首次 atomic transition 写入 required v2 header；header 后 legacy writer 被拒绝。已知完整前缀
后的未终止 JSON 尾行可截断并同步修复；首条或完整 malformed/checksum/unknown required record
永不当作空 store，磁盘/写入/同步/worker 不确定性返回 Unknown/结构化错误。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `jsonl_v2_transaction_page_is_atomic_and_replays_after_reopen` | 多事件 transition 作为一个 commit boundary 分页，不拆 batch，重开后 receipt/events 一致 |
| `jsonl_v2_rejects_malformed_first_record_and_tampered_frame` | malformed first line 与 body checksum tamper 均返回 corrupt，不暴露空 store |
| `jsonl_v2_repairs_only_a_torn_tail_and_rejects_legacy_after_upgrade` | 已知前缀后的 torn tail 可修复；v2 header 后 legacy record 拒绝 |
| `jsonl_v2_source_contract_keeps_lock_sync_and_bounded_recovery` | source guard 固定 flock/dirfd/no-follow/sync/identity/bounded reader 与 domain frame contract |

## 3. Proof ceiling and handoff

ER-05 proof ceiling 为 `source`：本地 JSONL adapter 已有单 writer、完整 frame 和损坏分类
实现，并由 CI-only integration fixtures 验证；未运行本地测试。真实掉电、跨主机锁、异步
队列/背压/shutdown ack、projector checkpoint、backup/retention 和外部/live/physical effect
仍留待 ER-06+、PD/DEP/SC。
