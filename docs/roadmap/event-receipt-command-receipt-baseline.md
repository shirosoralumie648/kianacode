# ER-04 CommandReceipt and transition read-set baseline

> 快照日期：2026-09-16。本文记录 ER-04 的 CommandReceipt/read-set/Unknown confirmation boundary；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-04`](event-receipt-recovery.md#step-er-04) |
| source snapshot | `3942623`（ER-03 event redaction 后的干净基线） |
| feature_status | `implemented`（CommandReceipt validation + EventStore read-set source contract） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，CI 执行运行时夹具 |
| canonical path | ControlPlane read dependencies → TransitionBatch validate/CAS → EventStore commit → CommandReceipt → read_command confirmation → Broker/projection |
| this step does | 固化 command digest、所有 authority/resource/aggregate read versions、contiguous stream writes、receipt cursor/event/version binding 和 Unknown 唯一确认入口 |
| this step does not | 不把 receipt 当外部效果证明，不把 Unknown 当成功/拒绝，不允许 partial append、旧 payload 重试或绕过 read-set 的 dispatch |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain journal/registry | `kiana-domain/src/journal.rs`, `kiana-domain/src/contracts.rs` | `1858866a2bcea0d726e7f36bc7e13524e16a828055305d8c3ac2b674cffad669`, `dbbda14e5df8db53d5abbd095aa651143e57d3937164ef8639f8c8602f963cba` |
| Ports/EventStore | `kiana-ports/src/lib.rs`, `kiana-eventlog/src/event_store_core.rs`, `kiana-eventlog/src/journal_core.rs`, `kiana-eventlog/src/memory.rs`, `kiana-eventlog/src/jsonl.rs`, `kiana-eventlog/src/stream.rs` | `a539b96c0813c0f08c043dd89b4f2bcbe9fdb4d6d9f93923443821b54679e6d0`, `506a8222a757a89e6516a12b6260daa7f35af2b3da49c17a23ca7cdbcc9d0b1e`, `84ef64bc8208b2ead45d1d05feb0265e94129b898cd67d04f5ed189d388bba56`, `bcb98daa0a9548384a9dafdd9d5af2aefacca3d01d699056ba86477d3276e9e9`, `f549e5de5bc4e197f268b865327bd1d0d7a4c10208daa3e7c13ceb68188e5c5f`, `bc955e9c583466a97c1fb02fda4bff8af076dd04ee8df342e21777345b9c033a` |
| Core boundary | `kiana-core/src/dispatch.rs` | `464b68ec143743580a201b01d26e7ca8d4f1dc542acb4116edb228a885b65cd0` |
| Fixtures/guard/workflow | `kiana-eventlog/tests/er04_command_receipt.rs`, `kiana-core/tests/er04_command_receipt_guard.rs`, `.github/workflows/er04-command-receipt.yml` | `47477f9234ea4bd27b7e49b9ffa377caa5dce1c79f6200e4c15daa15e4e6a83f`, `4abfa27c4915ac4b92a99c1dbaaf0891a5fb792e3bf710fecda6f36322d68c61`, `71a4f7f079ffaa8a36850d409fcb73358d958d82a16945d0a76e4f80e7c84477` |

这些 hash 是 ER-04 的静态 source 锚点，不是外部效果、durable 或跨主机 exactly-once 证明。

## 2. CommandReceipt contract

`CommandReceipt::validate_against(batch)` 校验：

- command_id 和 immutable command_digest 与 batch 完全一致；
- first_cursor/cursor 有界且与 batch 事件数连续；
- event_ids 按 batch 顺序逐一相等且无重复；
- receipt versions 覆盖 batch 的每个 expected read-set，且不回退版本。

`CommittedTransition::new` 仍在 observer 前验证 batch/receipt identity/cursor/event IDs；Receipt 是 EventStore 提交事实的只读证明，不是权限 token、handler result 或外部业务成功证明。

## 3. Read-set and outcome matrix

| 情况 | 处理 | 禁止的推断 |
|---|---|---|
| 缺少写 aggregate 的 expected version | `journal_write_not_in_read_set`，整批不写 | 不能用 payload 里的 run_id 自动补 read-set |
| 任一 aggregate 当前版本与 expected 不同 | `CommitOutcome::Conflict{changed}`，无 partial append | 不能把 stale Allow 追加到旧 stream |
| 同 command_id + 同 digest | `Replayed{original receipt}` | 不再追加事件/observer 通知 |
| 同 command_id + 不同 digest | structured conflict | 不能按新 payload 覆盖旧命令 |
| receipt cursor/event/version 被伪造 | `CommandReceipt::validate_against`/`CommittedTransition` 拒绝 | 不把手工 JSON 当提交证据 |
| commit 结果不确定 | `CommitOutcome::Unknown`；ControlPlane 仅用 `read_command` 确认 | 不发 permit、不 dispatch、不报告成功 |

所有 authority、resource、approval、budget、lease、run/action aggregate 必须作为同一 transition 的 expected_versions；冲突后由 ControlPlane 重新读取/计算决定。单事件 `append_expected` 仍是兼容能力，不等价多聚合事务。

## 4. CI-only fixture catalog

| Fixture | Purpose |
|---|---|
| `transition_missing_dependency_is_denied` | 写 aggregate 不在 read-set 时 fail-closed 且账本为空 |
| `read_set_conflict_appends_nothing` | stale CAS 返回 changed versions，不产生第二事实 |
| `unknown_commit_never_dispatches` | Unknown 只能等待 `read_command`，不会产生 dispatch authority |
| `command_receipt_and_read_set_are_single_event_store_boundaries` | domain/ports/eventlog/core source guard |

`.github/workflows/er04-command-receipt.yml` 在 GitHub runner 串行执行 eventlog fixtures/source guard、fmt/fetch；不在本地执行测试或 smoke。

## 5. 限制与交接

- CommandReceipt/read-set 只证明本地 EventStore commit boundary；JSONL fsync/lock、掉电、跨主机一致性和完整损坏恢复由 ER-05/PD/DEP 负责。
- `Unknown` 只表示 commit 回执不确定；handler/provider/connector effect 仍需 execution receipt/reconcile，不能用 read_command 缺失推断“未执行”。
- 当前部分 core/legacy path 仍用单事件 append；后续 CP-07/08/13/14、ER-05+ 逐步迁移，不能将兼容路径写成原子授权事实。
- Receipt 不携带完整业务 Outcome、SecretStore、retention/delete 或 cross-process RunSnapshot；这些由 ER/CP/PD/SC 后续步骤验收。
- 本地只做格式、workspace test-target 静态编译和 diff 检查；GitHub CI 结果按用户要求不等待，不提升 durable/live/physical。
