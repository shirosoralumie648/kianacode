# CP-07 JSONL 事务帧与恢复 reader 基线

> 快照日期：2026-09-17。本页记录现有 journal/eventlog source contract 与 GitHub CI-only fixture；
> 不把一次同步或 CI 编译写成跨进程整体 durable 证明。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CP-07`](control-plane.md#step-cp-07) |
| feature_status | `implemented`（complete JournalFrame validation/recovery guard） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | EventLog committed frames/CommandReceipt；Core 不新增磁盘后端 |
| this step does | Transition/Event frame body validation, checksum/size/header gates, receipt/read-set all-or-none, torn-tail repair and legacy writer upgrade guard |
| this step does not | 不实现新的存储事实源、自动重试/伪造成功、跨机器共识或绕过 ControlPlane 的写入路径 |

## 1. Contract

`kiana-domain/src/journal.rs` 进一步收紧 `JournalFrame::validate`：完整 frame 先验证 body
checksum/size，再验证 TransitionBatch、CommandReceipt 与 read-set/版本一致性；Event frame
拒绝 nil ID、零 sequence 和空 kind。`logical_events` 只展开完整 frame，尾部不完整记录不会进入
projection。

现有 `kiana-eventlog/src/jsonl.rs` 继续用单 writer lock、header/version gate、bounded line、
checksum、`sync_all`/directory sync、O_NOFOLLOW parent/file identity 和尾部 repair；旧单事件
格式在 v2 header 后禁止继续追加。`kiana-eventlog::JournalState` 只在完整 Transition/Event
frame 通过 body/replay 规划后更新内存索引，CommandReceipt 的 Committed/Replayed/Conflict/
Unknown 语义保持结构化，Core 仍通过 `commit_confirmed` 才能继续。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `complete_journal_frames_validate_body_and_expand_only_committed_events` | complete transition frame 的 batch/receipt/body integrity 与 logical events 一致 |
| `journal_frame_rejects_tampered_receipt_unknown_body_and_nil_event` | checksum/strict serde/nil event fail-closed |
| `legacy_event_frame_remains_single_event_but_never_accepts_zero_sequence` | legacy compatibility 只读单事件且不放行非法序号 |
| `cp07_keeps_complete_frame_recovery_and_does_not_add_a_second_store` | source guard 固定现有 EventLog/recovery path、sync/repair、无第二 store/loop |

## 3. Proof ceiling and handoff

CP-07 proof ceiling 为 `source`：frame/body validation 和已有本地 JSONL recovery boundary 已
被静态编译与 CI fixture 接线，但未在本地运行测试；磁盘/进程 crash、跨进程 writer、真实
power-loss 和 backup durability 仍需 ER/PD/DEP。CP-08 消费 authority epoch/CAS，CP-09/10
消费 command/approval read-set；任何 Unknown 都必须确认原命令后再继续，不能把尾部修复或 receipt
存在解释成现实 effect 成功。
